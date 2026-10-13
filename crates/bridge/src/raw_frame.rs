//! Frames kept as raw bytes.
//!
//! The bridge relays frames without re-encoding them, so the app and the extension
//! see exactly the bytes the other side sent, including fields this build of the
//! bridge does not know about. It only peeks at the envelope (`kind` and `id`) to
//! track which requests are waiting for an answer.

use std::io::{self, Read};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use wirecatch_protocol::frame::{self, FrameError, HEADER_LEN};

/// One whole frame: the 4-byte little-endian length followed by the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawFrame(Vec<u8>);

impl RawFrame {
    /// Encodes a message into a new frame.
    pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Self, FrameError> {
        frame::encode(value).map(Self)
    }

    /// The whole frame, length prefix included, ready to write.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The JSON body.
    pub(crate) fn body(&self) -> &[u8] {
        &self.0[HEADER_LEN..]
    }

    /// Whether a `hello` in this frame allows the bridge to start the app (`Hello::start_app`). Anything else, and a `hello` from a client that does not say, allows it, as before the field existed.
    pub(crate) fn allows_starting_the_app(&self) -> bool {
        #[derive(Deserialize)]
        struct Frame {
            call: Option<Call>,
        }
        #[derive(Deserialize)]
        struct Call {
            method: String,
            #[serde(default)]
            params: Option<Params>,
        }
        #[derive(Deserialize)]
        struct Params {
            start_app: Option<bool>,
        }
        match serde_json::from_slice::<Frame>(self.body()) {
            Ok(Frame {
                call: Some(Call { method, params }),
            }) if method == "hello" => params.and_then(|p| p.start_app).unwrap_or(true),
            _ => true,
        }
    }

    /// Reads the envelope fields the bridge needs.
    pub(crate) fn envelope(&self) -> Envelope {
        #[derive(Deserialize)]
        struct Fields {
            kind: String,
            #[serde(default)]
            id: Option<u64>,
        }
        match serde_json::from_slice::<Fields>(self.body()) {
            Ok(fields) => match fields.kind.as_str() {
                "request" => Envelope::Request(fields.id),
                "response" => Envelope::Response(fields.id),
                _ => Envelope::Other,
            },
            Err(_) => Envelope::Other,
        }
    }

    /// A frame holding `body` byte for byte, for tests that need JSON no message
    /// type produces (unknown fields, odd spacing, broken envelopes).
    #[cfg(test)]
    pub(crate) fn from_body(body: &[u8]) -> Self {
        let mut bytes = frame::length_prefix(body.len())
            .expect("a test body fits in one frame")
            .to_vec();
        bytes.extend_from_slice(body);
        Self(bytes)
    }

    fn with_header(header: [u8; HEADER_LEN]) -> Result<Self, FrameError> {
        let len = frame::body_len(header)?;
        let mut bytes = vec![0u8; HEADER_LEN + len];
        bytes[..HEADER_LEN].copy_from_slice(&header);
        Ok(Self(bytes))
    }
}

/// What a frame claims to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Envelope {
    /// A request, with its id when it has a usable one.
    Request(Option<u64>),
    /// A response, with its id when it has a usable one.
    Response(Option<u64>),
    /// An event, or anything that does not parse as an envelope.
    Other,
}

/// One read from the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Incoming {
    /// A frame within the size limit.
    Frame(RawFrame),
    /// A frame over the size limit, read and thrown away without being stored.
    Oversized {
        /// Its declared body length.
        len: usize,
    },
}

/// Blocking read of one frame from the browser. `Ok(None)` means the stream ended
/// cleanly before a new frame started.
///
/// The length is checked before anything is allocated. An oversized frame is
/// streamed into nothing (so memory stays bounded) and reported as
/// [`Incoming::Oversized`], which keeps the stream in step so the session can go
/// on.
pub(crate) fn read_blocking(reader: &mut impl Read) -> Result<Option<Incoming>, FrameError> {
    let Some(header) = frame::read_header(reader)? else {
        return Ok(None);
    };
    let mut frame = match RawFrame::with_header(header) {
        Ok(frame) => frame,
        Err(FrameError::TooLarge(len)) => {
            let skipped = io::copy(&mut reader.by_ref().take(len as u64), &mut io::sink())?;
            if skipped < len as u64 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            return Ok(Some(Incoming::Oversized { len }));
        }
        Err(e) => return Err(e),
    };
    reader.read_exact(&mut frame.0[HEADER_LEN..])?;
    Ok(Some(Incoming::Frame(frame)))
}

/// Async read of one frame, with the same rules as [`read_blocking`].
pub(crate) async fn read_async(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<Option<RawFrame>, FrameError> {
    let Some(header) = frame::read_header_async(reader).await? else {
        return Ok(None);
    };
    let mut frame = RawFrame::with_header(header)?;
    reader.read_exact(&mut frame.0[HEADER_LEN..]).await?;
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::frame::MAX_FRAME_LEN;

    fn raw(body: &str) -> RawFrame {
        RawFrame::from_body(body.as_bytes())
    }

    #[test]
    fn a_hello_says_whether_the_app_may_be_started() {
        let hello = |params: &str| {
            raw(&format!(
                r#"{{"kind":"request","id":1,"call":{{"method":"hello","params":{params}}}}}"#
            ))
        };
        assert!(!hello(r#"{"start_app":false}"#).allows_starting_the_app());
        assert!(hello(r#"{"start_app":true}"#).allows_starting_the_app());
        // Clients from before the field, and every other call, allow it as before.
        assert!(hello(r#"{"client":"cli"}"#).allows_starting_the_app());
        assert!(
            raw(r#"{"kind":"request","id":2,"call":{"method":"pause_all"}}"#)
                .allows_starting_the_app()
        );
        assert!(raw("not json").allows_starting_the_app());
    }

    fn frame_of(body: &[u8]) -> Vec<u8> {
        RawFrame::from_body(body).0
    }

    /// The header of a frame over the size limit, which `length_prefix` refuses
    /// to write.
    fn oversized_header(len: usize) -> [u8; 4] {
        u32::try_from(len).unwrap().to_le_bytes()
    }

    fn next_frame(cursor: &mut io::Cursor<Vec<u8>>) -> RawFrame {
        match read_blocking(cursor).unwrap().unwrap() {
            Incoming::Frame(frame) => frame,
            oversized @ Incoming::Oversized { .. } => {
                panic!("expected a frame, got {oversized:?}")
            }
        }
    }

    #[test]
    fn frames_are_read_byte_for_byte() {
        let body =
            br#"{ "kind" : "request", "id": 9, "call": {"method": "pause_all"}, "future": [1,2] }"#;
        let mut stream = frame_of(body);
        stream.extend(frame_of(b"{}"));
        let mut cursor = io::Cursor::new(stream.clone());
        let first = next_frame(&mut cursor);
        assert_eq!(first.as_bytes(), &stream[..4 + body.len()]);
        assert_eq!(first.envelope(), Envelope::Request(Some(9)));
        let second = next_frame(&mut cursor);
        assert_eq!(second.envelope(), Envelope::Other);
        assert!(read_blocking(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn an_oversized_frame_is_skipped_and_the_stream_stays_in_step() {
        let len = MAX_FRAME_LEN + 1;
        let mut stream = oversized_header(len).to_vec();
        stream.resize(4 + len, b'x');
        stream.extend(frame_of(br#"{"kind":"request","id":3}"#));
        let mut cursor = io::Cursor::new(stream);
        assert_eq!(
            read_blocking(&mut cursor).unwrap(),
            Some(Incoming::Oversized { len })
        );
        assert_eq!(
            next_frame(&mut cursor).envelope(),
            Envelope::Request(Some(3))
        );
        assert!(read_blocking(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn an_oversized_frame_cut_short_is_an_error() {
        let mut stream = oversized_header(MAX_FRAME_LEN + 1).to_vec();
        stream.extend_from_slice(b"only a little");
        assert!(read_blocking(&mut io::Cursor::new(stream)).is_err());
    }

    #[tokio::test]
    async fn the_app_side_still_refuses_oversized_frames() {
        let header = oversized_header(MAX_FRAME_LEN + 1);
        let mut slice = &header[..];
        assert!(matches!(
            read_async(&mut slice).await,
            Err(FrameError::TooLarge(_))
        ));
    }

    #[tokio::test]
    async fn a_header_cut_short_is_an_error_on_both_sides() {
        let bytes = frame_of(b"{}");
        let cut = &bytes[..2];
        assert!(read_blocking(&mut io::Cursor::new(cut.to_vec())).is_err());
        assert!(read_async(&mut &cut[..]).await.is_err());
    }

    #[test]
    fn a_frame_cut_short_is_an_error() {
        let mut bytes = frame_of(b"{\"kind\":\"request\"}");
        bytes.truncate(10);
        assert!(read_blocking(&mut io::Cursor::new(bytes)).is_err());
    }

    #[test]
    fn envelopes_are_recognized() {
        let cases: &[(&[u8], Envelope)] = &[
            (
                br#"{"kind":"response","id":4,"outcome":{}}"#,
                Envelope::Response(Some(4)),
            ),
            (br#"{"kind":"request","call":{}}"#, Envelope::Request(None)),
            (br#"{"kind":"request","id":-1}"#, Envelope::Other),
            (br#"{"kind":"event","event":{}}"#, Envelope::Other),
            (b"not json", Envelope::Other),
        ];
        for (body, expected) in cases {
            let mut cursor = io::Cursor::new(frame_of(body));
            let frame = next_frame(&mut cursor);
            assert_eq!(
                frame.envelope(),
                *expected,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[tokio::test]
    async fn async_and_blocking_reads_agree() {
        let bytes = frame_of(br#"{"kind":"event"}"#);
        let mut slice = &bytes[..];
        let frame = read_async(&mut slice).await.unwrap().unwrap();
        assert_eq!(frame.as_bytes(), &bytes[..]);
        assert!(read_async(&mut slice).await.unwrap().is_none());
    }
}
