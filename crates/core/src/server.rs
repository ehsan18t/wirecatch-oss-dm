//! The protocol server (D14): serves one client connection over any byte stream.
//!
//! The first call must be `hello`, naming protocol versions this app speaks too;
//! otherwise the client gets a clear error and the connection closes. The connection
//! then speaks the newest version both sides have (D32), and the app sends it only
//! the events and error codes that version has. After that every request gets a
//! response with its id, and events are pushed as they happen. A malformed or
//! oversized frame gets an error response, a call the connection's version does not
//! have gets `UnsupportedCall`, and either way the connection carries on. An error
//! that cannot be tied to a request (the frame was unreadable) uses id 0.
//!
//! Requests are handled side by side, each in its own task, so a slow one (a
//! capture waiting up to the capture timeout for its server) never holds up the
//! others, such as the extension withdrawing that very capture. Responses are
//! matched to requests by id only and may arrive in any order. One task writes
//! every response and event, so frames never interleave.

use std::io;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, warn};
use wirecatch_protocol::MIN_PROTOCOL_VERSION;
use wirecatch_protocol::frame::{self, FrameError, MAX_FRAME_LEN};
use wirecatch_protocol::version::{VersionError, VersionRange};
use wirecatch_protocol::wire::{
    Call, CallMethod, ClientKind, ErrorCode, Event, Hello, HelloReply, Message, Outcome,
    ProtocolError, Reply, RequestHead, UNKNOWN_REQUEST_ID,
};

use crate::manager::Manager;
use wirecatch_engine::units::KIB_USIZE;

/// Most requests from one connection that may be handled at once. A request beyond
/// this gets an error at once, so a client cannot pile up unbounded work.
pub const MAX_REQUESTS_IN_FLIGHT: usize = 64;

/// How much to read from the stream at a time.
const READ_CHUNK: usize = 16 * KIB_USIZE;

/// Serves one client until it disconnects, refuses the handshake, or the stream fails.
///
/// Returns `Ok(())` when the client goes away or is refused, and an error only when
/// reading from or writing to the stream fails.
///
/// # Errors
///
/// The error reading from or writing to `stream`, which ends the connection.
pub async fn serve_connection<S>(manager: Manager, mut stream: S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut frames = FrameReader::default();
    let Some((hello, version)) = handshake(&mut frames, &mut stream).await? else {
        return Ok(());
    };
    debug!(
        client = ?hello.client,
        client_version = %hello.client_version,
        protocol_version = version,
        "client connected"
    );

    let mut events = manager.subscribe_events();
    // Request tasks hand their responses back here, to be written by this task.
    let (responder, mut responses) = mpsc::unbounded_channel::<Message>();
    let slots = Arc::new(Semaphore::new(MAX_REQUESTS_IN_FLIGHT));
    loop {
        tokio::select! {
            frame = frames.next(&mut stream) => {
                let reply = match frame? {
                    None => break,
                    Some(Frame::TooLarge(len)) => Some(too_large(len)),
                    Some(Frame::Body(body)) => match parse_request(&body, version) {
                        Ok((id, call)) => {
                            start_request(&manager, &slots, &responder, version, id, call)
                        }
                        Err(reply) => Some(*reply),
                    },
                };
                if let Some(reply) = reply {
                    send(&mut stream, reply, version).await?;
                }
            }
            Some(response) = responses.recv() => send(&mut stream, response, version).await?,
            event = events.recv() => match event {
                Ok(event) => send(&mut stream, Message::Event { event }, version).await?,
                Err(RecvError::Lagged(missed)) => {
                    // Events were dropped; send the current state so the client is correct again.
                    warn!(missed, "client fell behind on events; resending current state");
                    for event in snapshot(&manager) {
                        send(&mut stream, Message::Event { event }, version).await?;
                    }
                }
                // The manager is gone; nothing more will happen.
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
    // The client has stopped sending; answer what it already asked, then close.
    drop(responder);
    while let Some(response) = responses.recv().await {
        send(&mut stream, response, version).await?;
    }
    Ok(())
}

/// Starts handling a request in its own task, which sends the response to
/// `responder`. Returns the error response to send instead when the connection
/// already has the most requests in flight it may have.
fn start_request(
    manager: &Manager,
    slots: &Arc<Semaphore>,
    responder: &mpsc::UnboundedSender<Message>,
    version: u32,
    id: u64,
    call: Call,
) -> Option<Message> {
    let Ok(slot) = slots.clone().try_acquire_owned() else {
        return Some(error(
            id,
            ErrorCode::LimitReached,
            format!(
                "The app is already working on {MAX_REQUESTS_IN_FLIGHT} requests from this connection at once. Wait for some answers, then send this again."
            ),
        ));
    };
    let (manager, responder) = (manager.clone(), responder.clone());
    // Not tied to the connection: a request the client stops waiting for still
    // finishes, so the manager is never left halfway through a change.
    tokio::spawn(async move {
        let outcome = handle(&manager, version, call).await;
        if responder.send(Message::Response { id, outcome }).is_err() {
            debug!(id, "the client left before its answer was ready");
        }
        drop(slot);
    });
    None
}

/// Reads the first frame and checks it is a `hello` naming protocol versions this app
/// speaks too. Returns the hello and the version the connection speaks, or `None` after
/// telling the client why it is refused.
async fn handshake<S>(frames: &mut FrameReader, stream: &mut S) -> io::Result<Option<(Hello, u32)>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let refusal = match frames.next(stream).await? {
        None => return Ok(None),
        Some(Frame::TooLarge(len)) => too_large(len),
        Some(Frame::Body(body)) => match parse_hello(&body) {
            Ok((id, hello)) => match hello
                .versions()
                .and_then(|theirs| VersionRange::THIS_BUILD.agree(theirs))
            {
                Ok(version) => {
                    send(stream, ok(id, hello_reply(version)), version).await?;
                    return Ok(Some((hello, version)));
                }
                Err(refused) => version_refusal(id, &hello, refused),
            },
            Err(refusal) => *refusal,
        },
    };
    // Until the versions are agreed, the client is sent only what the oldest version has.
    send(stream, refusal, MIN_PROTOCOL_VERSION).await?;
    Ok(None)
}

/// The hello in the first frame body, or the refusal to send instead: any other request,
/// whether the app has its call or not, is told that hello comes first.
fn parse_hello(body: &[u8]) -> Result<(u64, Hello), Box<Message>> {
    let first_must_be_hello = |id| {
        Box::new(error(
            id,
            ErrorCode::InvalidRequest,
            "The first message must be hello. Update the extension or the app so both use the same version.".into(),
        ))
    };
    match frame::decode::<Message>(body) {
        Ok(Message::Request {
            id,
            call: Call::Hello(hello),
        }) => Ok((id, hello)),
        Ok(Message::Request { id, .. }) => Err(first_must_be_hello(id)),
        Ok(Message::Response { id, .. }) => Err(Box::new(not_a_request(id))),
        Ok(Message::Event { .. }) => Err(Box::new(not_a_request(UNKNOWN_REQUEST_ID))),
        Err(e) => match RequestHead::read(body) {
            Some(head) if head.method != CallMethod::Hello => Err(first_must_be_hello(head.id)),
            head => Err(Box::new(not_understood(
                head.map_or_else(|| readable_id(body), |head| head.id),
                &e,
            ))),
        },
    }
}

/// Carries out a call, answering in the connection's protocol `version`.
async fn handle(manager: &Manager, version: u32, call: Call) -> Outcome {
    let result = match call {
        // A connection keeps the version its first hello agreed on.
        Call::Hello(_) => Ok(hello_reply(version)),
        Call::AddDownload(capture) => manager.add_capture(capture).await.map(Reply::DownloadAdded),
        Call::WithdrawDownload(request) => manager
            .withdraw_capture(&request.handover_id)
            .map(Reply::Withdrawn),
        Call::GetExtensionSettings => Ok(Reply::ExtensionSettings(manager.extension_settings())),
        Call::UpdateExtensionSettings(patch) => manager
            .update_extension_settings(patch)
            .map(Reply::ExtensionSettings),
        Call::ListQueues => Ok(Reply::Queues(manager.queue_summaries())),
        Call::PauseAll => manager.pause_all().map(|()| Reply::Done),
        Call::ResumeAll => manager.resume_all().map(|()| Reply::Done),
        Call::ShowWindow => {
            manager.request_window();
            Ok(Reply::Done)
        }
    };
    match result {
        Ok(reply) => Outcome::Ok(reply),
        Err(e) => Outcome::Err(e.to_protocol()),
    }
}

fn hello_reply(version: u32) -> Reply {
    Reply::Hello(HelloReply {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: version,
    })
}

/// The request in a frame body, when the connection's protocol `version` has its call, or
/// the error response to send instead.
fn parse_request(body: &[u8], version: u32) -> Result<(u64, Call), Box<Message>> {
    match frame::decode::<Message>(body) {
        Ok(Message::Request { id, call }) if call.method().is_in(version) => Ok((id, call)),
        Ok(Message::Request { id, call }) => {
            let method = call.method();
            debug!(
                id,
                ?method,
                version,
                "refused a call newer than the connection"
            );
            Err(Box::new(unsupported_call(id, method)))
        }
        Ok(Message::Response { id, .. }) => Err(Box::new(not_a_request(id))),
        Ok(Message::Event { .. }) => Err(Box::new(not_a_request(UNKNOWN_REQUEST_ID))),
        Err(e) => match RequestHead::read(body) {
            Some(head) if !head.method.is_in(version) => {
                debug!(
                    id = head.id,
                    version, "refused a call this app does not have: {e}"
                );
                Err(Box::new(unsupported_call(head.id, head.method)))
            }
            // Answer with the request's id if at least that much is readable.
            head => Err(Box::new(not_understood(
                head.map_or_else(|| readable_id(body), |head| head.id),
                &e,
            ))),
        },
    }
}

fn not_understood(id: u64, e: &FrameError) -> Message {
    error(
        id,
        ErrorCode::InvalidRequest,
        format!(
            "The app did not understand this message ({e}). Update the extension or the app so both use the same version."
        ),
    )
}

/// The id of a message that is not a readable request, when at least that much is readable.
fn readable_id(body: &[u8]) -> u64 {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_u64))
        .unwrap_or(UNKNOWN_REQUEST_ID)
}

/// The answer to a call the connection does not have: one this app lacks, which a newer
/// app may have, or one newer than the version the client agreed to, which it should not
/// have sent.
fn unsupported_call(id: u64, method: CallMethod) -> Message {
    let message = if method == CallMethod::Unknown {
        "This version of the Wirecatch app cannot do what was asked, so it did nothing. Everything else still works, and updating the Wirecatch app may add it."
    } else {
        "The connection to the Wirecatch app speaks an older protocol version, which cannot do what was asked, so nothing was done. Everything else still works."
    };
    error(id, ErrorCode::UnsupportedCall, message.into())
}

fn snapshot(manager: &Manager) -> [Event; 2] {
    [
        Event::ExtensionSettingsChanged(manager.extension_settings()),
        Event::QueuesChanged(manager.queue_summaries()),
    ]
}

/// The answer to a hello whose protocol versions the app cannot agree to, saying which
/// side to update.
fn version_refusal(id: u64, hello: &Hello, refused: VersionError) -> Message {
    let client = match hello.client {
        ClientKind::Extension => "browser extension",
        ClientKind::Bridge => "browser connector",
        ClientKind::Cli => "command-line tool",
    };
    let (code, message) = match refused {
        VersionError::Empty { oldest, newest } => (
            ErrorCode::InvalidRequest,
            format!(
                "The {client} named protocol versions {oldest} to {newest}, which is no range. Update the {client}."
            ),
        ),
        VersionError::PeerTooOld { peer, ours } => (
            ErrorCode::UnsupportedVersion,
            format!(
                "The {client} is too old for this Wirecatch app: it speaks protocol versions up to {}, and the app needs {} or newer. Update the {client}.",
                peer.newest(),
                ours.oldest(),
            ),
        ),
        VersionError::PeerTooNew { peer, ours } => (
            ErrorCode::UnsupportedVersion,
            format!(
                "The {client} is newer than this Wirecatch app: it needs protocol version {} or newer, and the app speaks up to {}. Update the Wirecatch app.",
                peer.oldest(),
                ours.newest(),
            ),
        ),
    };
    error(id, code, message)
}

fn too_large(len: usize) -> Message {
    error(
        UNKNOWN_REQUEST_ID,
        ErrorCode::InvalidRequest,
        format!(
            "A message of {len} bytes is larger than the {MAX_FRAME_LEN} byte limit, so it was skipped. Send less data in one message."
        ),
    )
}

fn not_a_request(id: u64) -> Message {
    error(
        id,
        ErrorCode::InvalidRequest,
        "Only requests can be sent to the app.".into(),
    )
}

fn ok(id: u64, reply: Reply) -> Message {
    Message::Response {
        id,
        outcome: Outcome::Ok(reply),
    }
}

fn error(id: u64, code: ErrorCode, message: String) -> Message {
    Message::Response {
        id,
        outcome: Outcome::Err(ProtocolError { code, message }),
    }
}

/// Writes `message` as the connection's protocol `version` has it (D32): every message to
/// the client passes here, so none carries an event or error code newer than the client.
async fn send<S: AsyncWrite + Unpin>(
    stream: &mut S,
    message: Message,
    version: u32,
) -> io::Result<()> {
    let Some(message) = message.as_of(version) else {
        return Ok(());
    };
    frame::write_async(stream, &message)
        .await
        .map_err(|e| match e {
            FrameError::Io(e) => e,
            other => io::Error::other(other),
        })
}

/// What one frame turned out to be.
#[derive(Debug, PartialEq, Eq)]
enum Frame {
    Body(Vec<u8>),
    /// Announced a body over the limit; the body is skipped without being stored.
    TooLarge(usize),
}

/// Splits a byte stream into frames. Its only await point is a cancel-safe read,
/// so it can sit in a `select!` next to the event stream without losing data.
///
/// Frames are taken from the buffer by moving a cursor; the unread rest (at most
/// one partial frame) is moved to the front only when more data must be read.
#[derive(Debug, Default)]
struct FrameReader {
    buf: Vec<u8>,
    /// Where the unread part of `buf` starts.
    start: usize,
    /// Bytes of an oversized body still to throw away.
    skip: usize,
}

impl FrameReader {
    /// The next frame, or `None` at the end of the stream.
    async fn next<R: AsyncRead + Unpin>(&mut self, reader: &mut R) -> io::Result<Option<Frame>> {
        loop {
            if let Some(frame) = self.take_frame() {
                return Ok(Some(frame));
            }
            self.compact();
            self.buf.reserve(READ_CHUNK);
            if reader.read_buf(&mut self.buf).await? == 0 {
                if !self.buf.is_empty() {
                    debug!(bytes = self.buf.len(), "stream ended inside a frame");
                }
                return Ok(None);
            }
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        if self.skip > 0 {
            let n = self.skip.min(self.buf.len() - self.start);
            self.start += n;
            self.skip -= n;
            if self.skip > 0 {
                return None;
            }
        }
        let unread = &self.buf[self.start..];
        let header: [u8; 4] = unread.get(..4)?.try_into().ok()?;
        match frame::body_len(header) {
            Ok(len) if unread.len() >= 4 + len => {
                let body = unread[4..4 + len].to_vec();
                self.start += 4 + len;
                Some(Frame::Body(body))
            }
            Ok(_) => None,
            Err(_) => {
                let len = u32::from_le_bytes(header) as usize;
                self.start += 4;
                self.skip = len;
                Some(Frame::TooLarge(len))
            }
        }
    }

    /// Moves the unread bytes to the front of the buffer.
    fn compact(&mut self) {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = frame::length_prefix(body.len()).unwrap().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn frames_split_across_reads_are_joined() {
        let mut bytes = framed(b"{\"a\":1}");
        bytes.extend(framed(b"[]"));
        let (mut client, mut server) = tokio::io::duplex(3);
        let writer = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            client.write_all(&bytes).await.unwrap();
        });
        let mut frames = FrameReader::default();
        assert_eq!(
            frames.next(&mut server).await.unwrap(),
            Some(Frame::Body(b"{\"a\":1}".to_vec()))
        );
        assert_eq!(
            frames.next(&mut server).await.unwrap(),
            Some(Frame::Body(b"[]".to_vec()))
        );
        writer.await.unwrap();
        assert_eq!(frames.next(&mut server).await.unwrap(), None);
    }

    #[tokio::test]
    async fn many_frames_from_one_read_are_taken_in_order() {
        let bytes: Vec<u8> = (0..2000u32)
            .flat_map(|i| framed(i.to_string().as_bytes()))
            .collect();
        let mut reader = bytes.as_slice();
        let mut frames = FrameReader::default();
        for i in 0..2000u32 {
            assert_eq!(
                frames.next(&mut reader).await.unwrap(),
                Some(Frame::Body(i.to_string().into_bytes()))
            );
        }
        assert_eq!(frames.next(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn oversized_frames_are_skipped_and_reading_continues() {
        let len = MAX_FRAME_LEN + 10;
        // Over the limit, so `length_prefix` rightly refuses to write it.
        let mut bytes = u32::try_from(len).unwrap().to_le_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(b'x', len));
        bytes.extend(framed(b"{}"));
        let mut reader = bytes.as_slice();
        let mut frames = FrameReader::default();
        assert_eq!(
            frames.next(&mut reader).await.unwrap(),
            Some(Frame::TooLarge(len))
        );
        assert_eq!(
            frames.next(&mut reader).await.unwrap(),
            Some(Frame::Body(b"{}".to_vec()))
        );
        assert!(frames.buf.capacity() < 2 * MAX_FRAME_LEN);
    }

    /// Every message to the client goes through `send`, which leaves out what the connection's
    /// version does not have.
    #[tokio::test]
    async fn sending_writes_only_what_the_connections_version_has() {
        let mut written = Vec::new();
        let event = Message::Event {
            event: Event::QueuesChanged(Vec::new()),
        };
        send(&mut written, event, 0).await.unwrap();
        assert!(
            written.is_empty(),
            "an event older clients cannot read was sent"
        );

        send(&mut written, unsupported_call(3, CallMethod::Unknown), 1)
            .await
            .unwrap();
        let sent: Message = frame::read_async(&mut written.as_slice())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                sent,
                Message::Response {
                    id: 3,
                    outcome: Outcome::Err(ProtocolError {
                        code: ErrorCode::InvalidRequest,
                        ..
                    })
                }
            ),
            "{sent:?}"
        );
    }

    /// A call the app lacks may come in a newer app; one newer than the connection should not
    /// have been sent. The answer says which.
    #[test]
    fn an_unsupported_call_says_which_side_lacks_it() {
        let message = |method| match unsupported_call(1, method) {
            Message::Response {
                outcome: Outcome::Err(error),
                ..
            } => {
                assert_eq!(error.code, ErrorCode::UnsupportedCall);
                error.message
            }
            other => panic!("not an error: {other:?}"),
        };
        assert!(message(CallMethod::Unknown).contains("updating the Wirecatch app"));
        assert!(message(CallMethod::ListQueues).contains("older protocol version"));
    }

    #[test]
    fn a_version_refusal_says_what_to_update() {
        let hello = Hello {
            client: ClientKind::Extension,
            client_version: "0.1.0".into(),
            protocol_version: 0,
            max_protocol_version: None,
            browser: None,
            start_app: true,
        };
        let refusal = |oldest, newest| {
            let refused = VersionRange::new(oldest, newest)
                .and_then(|theirs| VersionRange::THIS_BUILD.agree(theirs))
                .unwrap_err();
            match version_refusal(7, &hello, refused) {
                Message::Response {
                    id: 7,
                    outcome: Outcome::Err(error),
                } => error,
                other => panic!("not a refusal: {other:?}"),
            }
        };
        let newest = wirecatch_protocol::PROTOCOL_VERSION;
        let too_new = refusal(newest + 1, newest + 3);
        assert_eq!(too_new.code, ErrorCode::UnsupportedVersion);
        assert!(
            too_new.message.ends_with("Update the Wirecatch app."),
            "{}",
            too_new.message
        );
        let too_old = refusal(0, MIN_PROTOCOL_VERSION - 1);
        assert_eq!(too_old.code, ErrorCode::UnsupportedVersion);
        assert!(
            too_old.message.ends_with("Update the browser extension."),
            "{}",
            too_old.message
        );
        assert_eq!(refusal(2, 1).code, ErrorCode::InvalidRequest);
    }
}
