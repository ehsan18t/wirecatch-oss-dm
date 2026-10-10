//! The protocol server (D14): serves one client connection over any byte stream.
//!
//! The first call must be `hello` with this app's protocol version; otherwise the
//! client gets a clear error and the connection closes. After that every request
//! gets a response with its id, and events are pushed as they happen. A malformed or
//! oversized frame gets an error response and the connection carries on. An error
//! that cannot be tied to a request (the frame was unreadable) uses id 0.

use std::io;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, warn};
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame::{self, FrameError, MAX_FRAME_LEN};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Event, Hello, HelloReply, Message, Outcome, ProtocolError, Reply,
    UNKNOWN_REQUEST_ID,
};

use crate::manager::Manager;

/// How much to read from the stream at a time.
const READ_CHUNK: usize = 16 * 1024;

/// Serves one client until it disconnects, refuses the handshake, or the stream fails.
///
/// Returns `Ok(())` when the client goes away or is refused, and an error only when
/// reading from or writing to the stream fails.
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
    loop {
        tokio::select! {
            frame = frames.next(&mut stream) => {
                let reply = match frame? {
                    None => return Ok(()),
                    Some(Frame::TooLarge(len)) => too_large(len),
                    Some(Frame::Body(body)) => match parse_request(&body) {
                        Ok((id, call)) => Message::Response { id, outcome: handle(&manager, call).await },
                        Err(reply) => *reply,
                    },
                };
                send(&mut stream, &reply).await?;
            }
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
            .await
            .map(Reply::Withdrawn),
        Call::GetExtensionSettings => Ok(Reply::ExtensionSettings(manager.extension_settings())),
        Call::UpdateExtensionSettings(patch) => manager
            .update_extension_settings(patch)
            .map(Reply::ExtensionSettings),
        Call::ListQueues => Ok(Reply::Queues(manager.queue_summaries())),
        Call::PauseAll => manager.pause_all().map(|()| Reply::Done),
        Call::ResumeAll => manager.resume_all().map(|()| Reply::Done),
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
#[derive(Debug, Default)]
struct FrameReader {
    buf: Vec<u8>,
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
            let n = self.skip.min(self.buf.len());
            self.buf.drain(..n);
            self.skip -= n;
            if self.skip > 0 {
                return None;
            }
        }
        let header: [u8; 4] = self.buf.get(..4)?.try_into().ok()?;
        match frame::body_len(header) {
            Ok(len) if self.buf.len() >= 4 + len => {
                let body = self.buf[4..4 + len].to_vec();
                self.buf.drain(..4 + len);
                Some(Frame::Body(body))
            }
            Ok(_) => None,
            Err(_) => {
                let len = u32::from_le_bytes(header) as usize;
                self.buf.drain(..4);
                self.skip = len;
                Some(Frame::TooLarge(len))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
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
    async fn oversized_frames_are_skipped_and_reading_continues() {
        let len = MAX_FRAME_LEN + 10;
        let mut bytes = (len as u32).to_le_bytes().to_vec();
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
        };
        assert!(
            version_mismatch(&hello(PROTOCOL_VERSION + 1)).contains("Update the Wirecatch app")
        );
        assert!(version_mismatch(&hello(0)).contains("Update the browser extension"));
    }
}
