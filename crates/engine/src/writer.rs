//! Writing a connection's data into the part file.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::error::{Error, IoAction, IoContext, Result};
use crate::file_work::FileWork;
use crate::segment::{ByteRange, SharedTable};
use crate::storage;

/// What a finished write hands back.
struct Written {
    /// The emptied buffer, reused for the next write.
    buf: Vec<u8>,
    /// The segment the bytes belong to.
    index: usize,
    /// The file range that is now on disk.
    range: ByteRange,
}

type WriteJob = JoinHandle<std::io::Result<Written>>;

/// Collects incoming bytes into a large buffer and writes it with one positional
/// write on the blocking pool, while the next buffer fills (double buffering).
/// The segment's `flushed` offset only moves once a write has completed.
///
/// A failed write poisons the writer for the rest of the segment: the bytes already buffered after the failure are dropped, never written, because writing them would leave a hole below them that the resume state could count as done.
pub(crate) struct Writer {
    file: Arc<File>,
    /// Where `file` is, for errors.
    path: PathBuf,
    capacity: usize,
    buf: Vec<u8>,
    spare: Option<Vec<u8>>,
    index: usize,
    /// File offset of `buf[0]`.
    offset: u64,
    in_flight: Option<WriteJob>,
    /// Set when a write of this segment failed; cleared by [`Writer::start`].
    failed: bool,
    /// Bytes taken by [`Writer::push`] that are not known to be on disk yet: buffered or in flight.
    unwritten: u64,
    /// Bytes taken and then dropped because a write failed; see [`Writer::take_dropped`].
    dropped: u64,
    file_work: FileWork,
}

impl Writer {
    /// Writes into `file`, the part file at `path`, `capacity` bytes at a time.
    pub fn new(file: Arc<File>, path: PathBuf, capacity: usize, file_work: FileWork) -> Self {
        Self {
            file,
            path,
            capacity,
            buf: Vec::with_capacity(capacity),
            spare: None,
            index: 0,
            offset: 0,
            in_flight: None,
            failed: false,
            unwritten: 0,
            dropped: 0,
            file_work,
        }
    }

    pub fn start(&mut self, index: usize, offset: u64) {
        debug_assert!(self.buf.is_empty() && self.in_flight.is_none());
        self.index = index;
        self.offset = offset;
        self.failed = false;
    }

    /// Takes `data` to write. Once a write has failed, data is dropped (and counted in [`Writer::take_dropped`]) rather than buffered, since nothing more of this segment may be written.
    pub async fn push(&mut self, mut data: &[u8], table: &SharedTable) -> Result<()> {
        if self.failed {
            self.dropped += data.len() as u64;
            return Ok(());
        }
        self.unwritten += data.len() as u64;
        while !data.is_empty() {
            let n = (self.capacity - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..n]);
            data = &data[n..];
            if self.buf.len() >= self.capacity {
                self.flush(table).await?;
            }
        }
        Ok(())
    }

    /// Starts writing the current buffer; waits for the previous write first. Does nothing once a write has failed.
    async fn flush(&mut self, table: &SharedTable) -> Result<()> {
        self.wait(table).await?;
        if self.failed || self.buf.is_empty() {
            return Ok(());
        }
        let next = self
            .spare
            .take()
            .unwrap_or_else(|| Vec::with_capacity(self.capacity));
        let mut full = std::mem::replace(&mut self.buf, next);
        let (file, index) = (self.file.clone(), self.index);
        let range = ByteRange {
            start: self.offset,
            end: self.offset + full.len() as u64,
        };
        self.offset = range.end;
        self.in_flight = Some(self.file_work.spawn_blocking(move || {
            storage::write_all_at(&file, &full, range.start)?;
            full.clear();
            Ok(Written {
                buf: full,
                index,
                range,
            })
        }));
        Ok(())
    }

    /// Waits for the write in flight and records it. On failure the writer drops what it has buffered and stops writing this segment.
    async fn wait(&mut self, table: &SharedTable) -> Result<()> {
        let Some(handle) = self.in_flight.take() else {
            return Ok(());
        };
        match handle
            .await
            .map_err(|e| Error::task(&e))
            .and_then(|r| r.context(IoAction::Write, &self.path))
        {
            Ok(written) => {
                self.unwritten -= written.range.end - written.range.start;
                table.lock().mark_flushed(written.index, written.range);
                self.spare = Some(written.buf);
                Ok(())
            }
            Err(error) => {
                self.failed = true;
                self.buf.clear();
                self.dropped += std::mem::take(&mut self.unwritten);
                Err(error)
            }
        }
    }

    /// Bytes dropped since the last call because a write failed: received and counted, but never written.
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    /// Writes everything buffered and waits until it is on disk.
    pub async fn finish(&mut self, table: &SharedTable) -> Result<()> {
        self.flush(table).await?;
        self.wait(table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::SegmentTable;

    /// A write fails while the next buffer is already full. That buffer must not be written on the way out: landing above the hole, it would count the hole as flushed, and a resumed or finished download would keep a range of zeros.
    #[tokio::test]
    async fn a_failed_write_is_never_covered_by_the_next_one() {
        const CAPACITY: usize = 4;
        let dir = wirecatch_test_support::private_dir("writer").unwrap();
        let path = dir.path().join("f.bin.wcpart");
        let file = Arc::new(File::create(&path).unwrap());
        let table = SharedTable::new(SegmentTable::new(Some(16), 1));
        table.lock().acquire(0).unwrap();
        table.lock().claim(0, 8);

        let mut writer = Writer::new(Arc::clone(&file), path.clone(), CAPACITY, FileWork::new());
        writer.start(0, 0);
        // The first buffer, [0, 4), went out and its write fails.
        writer.offset = 4;
        writer.in_flight = Some(tokio::task::spawn_blocking(|| {
            Err(std::io::Error::other("disk full"))
        }));

        // The first buffer was taken by push before it went out.
        writer.unwritten = 4;

        let pushed = writer.push(b"BBBB", &table).await;
        assert!(pushed.is_err(), "the failed write is reported");
        writer.finish(&table).await.unwrap();
        assert_eq!(
            writer.take_dropped(),
            8,
            "both buffers were counted and dropped"
        );

        // A push after the failure must not loop on a full buffer that is never emptied.
        writer.push(b"CCCCCCCC", &table).await.unwrap();
        assert_eq!(writer.take_dropped(), 8);

        assert_eq!(table.lock().remaining()[0].start, 0);
        assert_eq!(
            file.metadata().unwrap().len(),
            0,
            "nothing lands above the hole"
        );
        drop(file);
        dir.remove().unwrap();
    }
}
