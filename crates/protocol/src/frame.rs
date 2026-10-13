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
    // Written out rather than with the engine's units: this crate must not depend on the engine.
    pub const MAX_FRAME_LEN: usize = 1024 * 1024;
}

/// Length of the header that starts every frame.
pub const HEADER_LEN: usize = 4;

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

/// The header that announces a body of `len` bytes, built with the size rule of
/// [`MAX_FRAME_LEN`] so a writer outside this module need not repeat it.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when `len` is over [`MAX_FRAME_LEN`].
pub fn length_prefix(len: usize) -> Result<[u8; HEADER_LEN], FrameError> {
    u32::try_from(len)
        .ok()
        .filter(|_| len <= MAX_FRAME_LEN)
        .map(u32::to_le_bytes)
        .ok_or(FrameError::TooLarge(len))
}

/// Serializes `value` into one frame (length prefix included), written straight after a
/// placeholder for the prefix so the body is never copied.
///
/// # Errors
///
/// [`FrameError::Json`] when `value` cannot be serialized, and [`FrameError::TooLarge`]
/// when its JSON is over [`MAX_FRAME_LEN`].
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let mut out = vec![0u8; HEADER_LEN];
    serde_json::to_writer(&mut out, value)?;
    let prefix = length_prefix(out.len() - HEADER_LEN)?;
    out[..HEADER_LEN].copy_from_slice(&prefix);
    Ok(out)
}

/// Reads the body length from a frame header, rejecting oversized frames before
/// anything is allocated.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when the header announces more than [`MAX_FRAME_LEN`] bytes.
pub fn body_len(header: [u8; HEADER_LEN]) -> Result<usize, FrameError> {
    // Saturating where `usize` is narrower than 32 bits: that is over the limit either way.
    let len = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
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

/// What filling a header has got to after one read.
enum HeaderProgress {
    /// The stream ended before the first byte of a frame: a clean end.
    Ended,
    /// More bytes are needed.
    Partial,
    /// The header is complete.
    Done,
}

/// Records one read of `got` bytes into a header of which `filled` bytes were already there.
fn header_progress(filled: &mut usize, got: usize) -> io::Result<HeaderProgress> {
    if got == 0 {
        // A stream that ends after part of a header ends inside a frame.
        return if *filled == 0 {
            Ok(HeaderProgress::Ended)
        } else {
            Err(io::ErrorKind::UnexpectedEof.into())
        };
    }
    *filled += got;
    Ok(if *filled == HEADER_LEN {
        HeaderProgress::Done
    } else {
        HeaderProgress::Partial
    })
}

/// Blocking read of one frame header. Returns `Ok(None)` when the stream ends before a new
/// frame starts.
///
/// # Errors
///
/// [`FrameError::Io`] when reading fails, or with [`io::ErrorKind::UnexpectedEof`] when the
/// stream ends after part of the header.
pub fn read_header(reader: &mut impl Read) -> Result<Option<[u8; HEADER_LEN]>, FrameError> {
    let mut header = [0u8; HEADER_LEN];
    let mut filled = 0;
    loop {
        let got = match reader.read(&mut header[filled..]) {
            Ok(got) => got,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        match header_progress(&mut filled, got)? {
            HeaderProgress::Ended => return Ok(None),
            HeaderProgress::Partial => {}
            HeaderProgress::Done => return Ok(Some(header)),
        }
    }
}

/// Blocking read of one frame's body, checked against [`MAX_FRAME_LEN`] before anything is
/// allocated. Returns `Ok(None)` when the stream ends before a new frame starts.
///
/// # Errors
///
/// The errors of [`read_header`] and [`body_len`], and [`FrameError::Io`] when the stream
/// ends inside the body.
pub fn read_body(reader: &mut impl Read) -> Result<Option<Vec<u8>>, FrameError> {
    let Some(header) = read_header(reader)? else {
        return Ok(None);
    };
    let mut body = vec![0u8; body_len(header)?];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Blocking read of one frame. Returns `Ok(None)` on a clean end of stream.
///
/// # Errors
///
/// The errors of [`read_body`] and [`decode`].
pub fn read<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, FrameError> {
    read_body(reader)?.map(|body| decode(&body)).transpose()
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

/// Async read of one frame header, with the rules of [`read_header`].
///
/// # Errors
///
/// As [`read_header`].
#[cfg(feature = "tokio")]
pub async fn read_header_async(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<Option<[u8; HEADER_LEN]>, FrameError> {
    use tokio::io::AsyncReadExt;
    let mut header = [0u8; HEADER_LEN];
    let mut filled = 0;
    loop {
        let got = match reader.read(&mut header[filled..]).await {
            Ok(got) => got,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        match header_progress(&mut filled, got)? {
            HeaderProgress::Ended => return Ok(None),
            HeaderProgress::Partial => {}
            HeaderProgress::Done => return Ok(Some(header)),
        }
    }
}

/// Async read of one frame's body, with the rules of [`read_body`].
///
/// # Errors
///
/// As [`read_body`].
#[cfg(feature = "tokio")]
pub async fn read_body_async(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<Option<Vec<u8>>, FrameError> {
    use tokio::io::AsyncReadExt;
    let Some(header) = read_header_async(reader).await? else {
        return Ok(None);
    };
    let mut body = vec![0u8; body_len(header)?];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
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
    read_body_async(reader)
        .await?
        .map(|body| decode(&body))
        .transpose()
}

/// Async write of one frame, flushed.
///
/// # Errors
///
/// As [`write()`].
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

    /// A stream that ends after one to three bytes of a header was cut off inside a frame,
    /// which is an error; only a stream that ends between frames ends cleanly.
    #[test]
    fn a_header_cut_short_is_an_error_not_a_clean_end() {
        for cut in 1..4 {
            let mut cursor = io::Cursor::new(vec![1u8; cut]);
            let read = read::<Message>(&mut cursor);
            assert!(
                matches!(&read, Err(FrameError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof),
                "{cut} header bytes: {read:?}"
            );
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn a_header_cut_short_is_an_error_when_reading_async_too() {
        for cut in 1..4 {
            let bytes = vec![1u8; cut];
            let read = read_async::<Message>(&mut bytes.as_slice()).await;
            assert!(
                matches!(&read, Err(FrameError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof),
                "{cut} header bytes: {read:?}"
            );
        }
    }

    #[test]
    fn a_frame_is_its_length_then_its_json() {
        let msg = Message::Request {
            id: 5,
            call: Call::PauseAll,
        };
        let json = serde_json::to_vec(&msg).unwrap();
        let framed = encode(&msg).unwrap();
        assert_eq!(framed[..HEADER_LEN], length_prefix(json.len()).unwrap());
        assert_eq!(framed[HEADER_LEN..], json[..]);
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
