//! Framing: a 4-byte little-endian length followed by that many bytes of UTF-8 JSON.
//!
//! This matches Chrome and Firefox native messaging on every platform Wirecatch
//! targets (they use native byte order, which is little-endian on x86 and ARM), so
//! the bridge can use one codec on both of its sides.

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

crate::exported::exported_consts! {
    /// Largest message body in bytes. Browsers refuse messages from a native host larger
    /// than 1 MiB; the same cap applies to every local connection so no side can be made to
    /// allocate more.
    pub const MAX_FRAME_LEN: usize = 1024 * 1024;
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_LEN} byte limit")]
    TooLarge(usize),
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Serializes `value` into one frame (length prefix included).
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(body.len()));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Reads the body length from a frame header, rejecting oversized frames before
/// anything is allocated.
pub fn body_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    Ok(len)
}

/// Parses a frame body (without the length prefix).
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    Ok(serde_json::from_slice(body)?)
}

/// Blocking read of one frame. Returns `Ok(None)` on a clean end of stream.
pub fn read<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, FrameError> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut body = vec![0u8; body_len(header)?];
    reader.read_exact(&mut body)?;
    decode(&body).map(Some)
}

/// Blocking write of one frame, flushed.
pub fn write<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), FrameError> {
    writer.write_all(&encode(value)?)?;
    writer.flush()?;
    Ok(())
}

/// Async read of one frame. Returns `Ok(None)` on a clean end of stream.
#[cfg(feature = "tokio")]
pub async fn read_async<T: DeserializeOwned>(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<Option<T>, FrameError> {
    use tokio::io::AsyncReadExt;
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut body = vec![0u8; body_len(header)?];
    reader.read_exact(&mut body).await?;
    decode(&body).map(Some)
}

/// Async write of one frame, flushed.
#[cfg(feature = "tokio")]
pub async fn write_async<T: Serialize>(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &T,
) -> Result<(), FrameError> {
    use tokio::io::AsyncWriteExt;
    writer.write_all(&encode(value)?).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Call, Message};

    #[test]
    fn round_trip_through_a_stream() {
        let msg = Message::Request {
            id: 7,
            call: Call::PauseAll,
        };
        let mut buf = Vec::new();
        write(&mut buf, &msg).unwrap();
        write(&mut buf, &msg).unwrap();
        let mut cursor = io::Cursor::new(buf);
        assert_eq!(read::<Message>(&mut cursor).unwrap(), Some(msg.clone()));
        assert_eq!(read::<Message>(&mut cursor).unwrap(), Some(msg));
        assert_eq!(read::<Message>(&mut cursor).unwrap(), None);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn async_round_trip_over_a_duplex_stream() {
        let (mut a, mut b) = tokio::io::duplex(64);
        let msg = Message::Request {
            id: 3,
            call: Call::ResumeAll,
        };
        let sent = msg.clone();
        let writer = tokio::spawn(async move { write_async(&mut a, &sent).await });
        assert_eq!(read_async::<Message>(&mut b).await.unwrap(), Some(msg));
        writer.await.unwrap().unwrap();
        assert_eq!(read_async::<Message>(&mut b).await.unwrap(), None);
    }

    #[test]
    fn oversized_header_is_rejected_without_allocating() {
        let header = ((MAX_FRAME_LEN + 1) as u32).to_le_bytes();
        assert!(matches!(body_len(header), Err(FrameError::TooLarge(_))));
    }
}
