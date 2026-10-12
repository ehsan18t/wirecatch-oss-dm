//! The protocol server (D14): serves one client connection over any byte stream.
//!
//! The first call must be `hello` with this app's protocol version; otherwise the
//! client gets a clear error and the connection closes. After that every request
//! gets a response with its id, and events are pushed as they happen. A malformed or
//! oversized frame gets an error response and the connection carries on. An error
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
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame::{self, FrameError, MAX_FRAME_LEN};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Event, Hello, HelloReply, Message, Outcome, ProtocolError, Reply,
    UNKNOWN_REQUEST_ID,
};

use crate::manager::Manager;

/// Most requests from one connection that may be handled at once. A request beyond
/// this gets an error at once, so a client cannot pile up unbounded work.
pub const MAX_REQUESTS_IN_FLIGHT: usize = 64;

/// How much to read from the stream at a time.
const READ_CHUNK: usize = 16 * 1024;

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
    let Some(hello) = handshake(&mut frames, &mut stream).await? else {
        return Ok(());
    };
    debug!(client = ?hello.client, version = %hello.client_version, "client connected");

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
                    Some(Frame::Body(body)) => match parse_request(&body) {
                        Ok((id, call)) => start_request(&manager, &slots, &responder, id, call),
                        Err(reply) => Some(*reply),
                    },
                };
                if let Some(reply) = reply {
                    send(&mut stream, &reply).await?;
                }
            }
            Some(response) = responses.recv() => send(&mut stream, &response).await?,
            event = events.recv() => match event {
                Ok(event) => send(&mut stream, &Message::Event { event }).await?,
                Err(RecvError::Lagged(missed)) => {
                    // Events were dropped; send the current state so the client is correct again.
                    warn!(missed, "client fell behind on events; resending current state");
                    for event in snapshot(&manager) {
                        send(&mut stream, &Message::Event { event }).await?;
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
        send(&mut stream, &response).await?;
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
    id: u64,
    call: Call,
) -> Option<Message> {
    let Ok(slot) = slots.clone().try_acquire_owned() else {
        return Some(error(
            id,
            ErrorCode::InvalidRequest,
            format!(
                "The app is already working on {MAX_REQUESTS_IN_FLIGHT} requests from this connection at once. Wait for some answers, then send this again."
            ),
        ));
    };
    let (manager, responder) = (manager.clone(), responder.clone());
    // Not tied to the connection: a request the client stops waiting for still
    // finishes, so the manager is never left halfway through a change.
    tokio::spawn(async move {
        let outcome = handle(&manager, call).await;
        // The client may be gone; then nobody is waiting for the answer.
        let _ = responder.send(Message::Response { id, outcome });
        drop(slot);
    });
    None
}

/// Reads the first frame and checks it is a `hello` this app understands. Returns
/// `None` after telling the client why it is refused.
async fn handshake<S>(frames: &mut FrameReader, stream: &mut S) -> io::Result<Option<Hello>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let refusal = match frames.next(stream).await? {
        None => return Ok(None),
        Some(Frame::TooLarge(len)) => too_large(len),
        Some(Frame::Body(body)) => match parse_request(&body) {
            Ok((id, Call::Hello(hello))) if hello.protocol_version == PROTOCOL_VERSION => {
                let reply = Reply::Hello(HelloReply {
                    app_version: env!("CARGO_PKG_VERSION").to_string(),
                    protocol_version: PROTOCOL_VERSION,
                });
                send(stream, &ok(id, reply)).await?;
                return Ok(Some(hello));
            }
            Ok((id, Call::Hello(hello))) => error(
                id,
                ErrorCode::UnsupportedVersion,
                version_mismatch(&hello),
            ),
            Ok((id, _)) => error(
                id,
                ErrorCode::InvalidRequest,
                "The first message must be hello. Update the extension or the app so both use the same version.".into(),
            ),
            Err(reply) => *reply,
        },
    };
    send(stream, &refusal).await?;
    Ok(None)
}

async fn handle(manager: &Manager, call: Call) -> Outcome {
    let result = match call {
        Call::Hello(_) => Ok(Reply::Hello(HelloReply {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        })),
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

/// The request in a frame body, or the error response to send instead.
fn parse_request(body: &[u8]) -> Result<(u64, Call), Box<Message>> {
    match frame::decode::<Message>(body) {
        Ok(Message::Request { id, call }) => Ok((id, call)),
        Ok(Message::Response { id, .. }) => Err(Box::new(not_a_request(id))),
        Ok(Message::Event { .. }) => Err(Box::new(not_a_request(UNKNOWN_REQUEST_ID))),
        Err(e) => {
            // Answer with the request's id if at least that much is readable.
            let id = serde_json::from_slice::<Value>(body)
                .ok()
                .and_then(|v| v.get("id").and_then(Value::as_u64))
                .unwrap_or(UNKNOWN_REQUEST_ID);
            Err(Box::new(error(
                id,
                ErrorCode::InvalidRequest,
                format!(
                    "The app did not understand this message ({e}). Update the extension or the app so both use the same version."
                ),
            )))
        }
    }
}

fn snapshot(manager: &Manager) -> [Event; 2] {
    [
        Event::ExtensionSettingsChanged(manager.extension_settings()),
        Event::QueuesChanged(manager.queue_summaries()),
    ]
}

fn version_mismatch(hello: &Hello) -> String {
    let client = match hello.client {
        ClientKind::Extension => "browser extension",
        ClientKind::Bridge => "browser connector",
        ClientKind::Cli => "command-line tool",
    };
    let outdated = if hello.protocol_version > PROTOCOL_VERSION {
        "Wirecatch app"
    } else {
        client
    };
    format!(
        "The Wirecatch app speaks protocol version {PROTOCOL_VERSION}, but the {client} speaks version {}. Update the {outdated} so both match.",
        hello.protocol_version,
    )
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

async fn send<S: AsyncWrite + Unpin>(stream: &mut S, message: &Message) -> io::Result<()> {
    frame::write_async(stream, message)
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

    #[test]
    fn version_mismatch_says_what_to_update() {
        let hello = |v| Hello {
            client: ClientKind::Extension,
            client_version: "0.1.0".into(),
            protocol_version: v,
            browser: None,
            start_app: true,
        };
        assert!(
            version_mismatch(&hello(PROTOCOL_VERSION + 1)).contains("Update the Wirecatch app")
        );
        assert!(version_mismatch(&hello(0)).contains("Update the browser extension"));
    }
}
