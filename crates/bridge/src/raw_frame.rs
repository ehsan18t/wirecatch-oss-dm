//! Frames kept as raw bytes.
//!
//! The bridge relays frames without re-encoding them, so the app and the extension
//! see exactly the bytes the other side sent, including fields this build of the
//! bridge does not know about. It only peeks at the envelope (`kind` and `id`) to
//! track which requests are waiting for an answer.

use std::io::{self, Read};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use wirecatch_protocol::frame::{self, FrameError};

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
        &self.0[4..]
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

    fn with_header(header: [u8; 4]) -> Result<Self, FrameError> {
        let len = frame::body_len(header)?;
        let mut bytes = vec![0u8; 4 + len];
        bytes[..4].copy_from_slice(&header);
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

/// Blocking read of one frame. `Ok(None)` means the stream ended cleanly before a
/// new frame started. The length is checked before anything is allocated.
pub(crate) fn read_blocking(reader: &mut impl Read) -> Result<Option<RawFrame>, FrameError> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut frame = RawFrame::with_header(header)?;
    reader.read_exact(&mut frame.0[4..])?;
    Ok(Some(frame))
}

/// Async read of one frame, with the same rules as [`read_blocking`].
pub(crate) async fn read_async(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<Option<RawFrame>, FrameError> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut frame = RawFrame::with_header(header)?;
    reader.read_exact(&mut frame.0[4..]).await?;
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::frame::MAX_FRAME_LEN;

    fn frame_of(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn frames_are_read_byte_for_byte() {
        let body =
            br#"{ "kind" : "request", "id": 9, "call": {"method": "pause_all"}, "future": [1,2] }"#;
        let mut stream = frame_of(body);
        stream.extend(frame_of(b"{}"));
        let mut cursor = io::Cursor::new(stream.clone());
        let first = read_blocking(&mut cursor).unwrap().unwrap();
        assert_eq!(first.as_bytes(), &stream[..4 + body.len()]);
        assert_eq!(first.envelope(), Envelope::Request(Some(9)));
        let second = read_blocking(&mut cursor).unwrap().unwrap();
        assert_eq!(second.envelope(), Envelope::Other);
        assert!(read_blocking(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn oversized_frames_are_refused_before_allocating() {
        let header = ((MAX_FRAME_LEN + 1) as u32).to_le_bytes();
        let mut cursor = io::Cursor::new(header.to_vec());
        assert!(matches!(
            read_blocking(&mut cursor),
            Err(FrameError::TooLarge(_))
        ));
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
            let frame = read_blocking(&mut cursor).unwrap().unwrap();
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
