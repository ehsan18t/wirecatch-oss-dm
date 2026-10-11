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

/// Why a frame could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The body is longer than [`MAX_FRAME_LEN`]; holds its length in bytes. On reading, the
    /// header announced it, and nothing was allocated for it.
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_LEN} byte limit")]
    TooLarge(usize),
    /// The body is not the JSON of the expected message, or the value could not be written
    /// as JSON.
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
    /// Reading from or writing to the connection failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The 4-byte header that announces a body of `len` bytes. Everything that writes frames
/// builds the header here, so the length rule has one home.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when `len` is over [`MAX_FRAME_LEN`].
pub fn length_prefix(len: usize) -> Result<[u8; 4], FrameError> {
    u32::try_from(len)
        .ok()
        .filter(|_| len <= MAX_FRAME_LEN)
        .map(u32::to_le_bytes)
        .ok_or(FrameError::TooLarge(len))
}

/// Serializes `value` into one frame (length prefix included).
///
/// # Errors
///
/// [`FrameError::Json`] when `value` cannot be serialized, and [`FrameError::TooLarge`]
/// when its JSON is over [`MAX_FRAME_LEN`].
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value)?;
    let prefix = length_prefix(body.len())?;
    let mut out = Vec::with_capacity(prefix.len() + body.len());
    out.extend_from_slice(&prefix);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Reads the body length from a frame header, rejecting oversized frames before
/// anything is allocated.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when the header announces more than [`MAX_FRAME_LEN`] bytes.
pub fn body_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    Ok(len)
}

/// Parses a frame body (without the length prefix).
///
/// # Errors
///
/// [`FrameError::Json`] when the body is not the JSON of a `T`.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    Ok(serde_json::from_slice(body)?)
}

/// Blocking read of one frame. Returns `Ok(None)` on a clean end of stream.
///
/// # Errors
///
/// [`FrameError::Io`] when reading fails or the stream ends inside a frame, and the errors
/// of [`body_len`] and [`decode`].
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
///
/// # Errors
///
/// The errors of [`encode`], and [`FrameError::Io`] when writing or flushing fails.
pub fn write<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), FrameError> {
    writer.write_all(&encode(value)?)?;
    writer.flush()?;
    Ok(())
}

/// Async read of one frame. Returns `Ok(None)` on a clean end of stream.
///
/// # Errors
///
/// As [`read`].
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
///
/// # Errors
///
/// As [`write`].
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
        let header = u32::try_from(MAX_FRAME_LEN + 1).unwrap().to_le_bytes();
        assert!(matches!(body_len(header), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn the_length_prefix_is_little_endian_and_bounded() {
        assert_eq!(length_prefix(258).unwrap(), [2, 1, 0, 0]);
        assert_eq!(
            length_prefix(MAX_FRAME_LEN).unwrap(),
            u32::try_from(MAX_FRAME_LEN).unwrap().to_le_bytes()
        );
        assert!(matches!(
            length_prefix(MAX_FRAME_LEN + 1),
            Err(FrameError::TooLarge(len)) if len == MAX_FRAME_LEN + 1
        ));
    }
}
