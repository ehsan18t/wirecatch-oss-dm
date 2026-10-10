//! One connection's loop: take a range, stream it to disk, take the next.

use std::fs::File;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, Response};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::download::Shared;
use crate::error::{Error, Result};
use crate::segment::{Assignment, SegmentTable, UNKNOWN_END};
use crate::{http, storage};

/// How a stretch of streaming ended.
enum Ended {
    /// The segment is fully claimed.
    Done,
    Cancelled,
    /// The server closed the body.
    Eof,
}

pub(crate) async fn run(
    id: usize,
    shared: Arc<Shared>,
    client: Client,
    mut first: Option<(Assignment, Response)>,
) -> Result<()> {
    let file = Arc::new(storage::open_for_write(&shared.part_path)?);
    let mut writer = Writer::new(file, shared.opts.buffer_size);

    loop {
        let (assignment, response) = match first.take() {
            Some((assignment, response)) => (assignment, Some(response)),
            None => match shared.table.lock().unwrap().acquire(id) {
                Some(assignment) => (assignment, None),
                None => return Ok(()),
            },
        };
        debug!(
            worker = id,
            from = assignment.from,
            to = assignment.to,
            "assigned"
        );

        let result =
            run_assignment(&shared, &client, &mut writer, assignment.index, response).await;
        match result {
            Ok(Ended::Done) => continue,
            Ok(Ended::Cancelled) | Ok(Ended::Eof) => {
                shared.table.lock().unwrap().release(assignment.index);
                return Ok(());
            }
            Err(e) => {
                shared.table.lock().unwrap().release(assignment.index);
                return Err(e);
            }
        }
    }
}

/// Downloads one segment, retrying transient failures from wherever it got to.
async fn run_assignment(
    shared: &Shared,
    client: &Client,
    writer: &mut Writer,
    index: usize,
    mut response: Option<Response>,
) -> Result<Ended> {
    let mut failures = 0u32;
    loop {
        if shared.cancel.is_cancelled() {
            return Ok(Ended::Cancelled);
        }
        let (from, to) = shared.table.lock().unwrap().position(index);
        if from >= to {
            return Ok(Ended::Done);
        }

        let attempt = async {
            let (resp, skip) = match response.take() {
                Some(resp) => (resp, 0),
                None => {
                    http::fetch(client, &shared.url, from, to, shared.ranged, shared.total).await?
                }
            };
            stream(shared, writer, index, from, resp, skip).await
        };

        match attempt.await {
            Ok(ended) => return Ok(ended),
            Err(e) if e.is_retryable() && failures < shared.opts.max_retries => {
                let (now, _) = shared.table.lock().unwrap().position(index);
                failures = if now > from { 0 } else { failures + 1 };
                let delay = backoff(failures);
                warn!(error = %e, retry_in = ?delay, "connection failed, retrying");
                tokio::select! {
                    _ = shared.cancel.cancelled() => return Ok(Ended::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// Streams a response body into the segment until it is fully claimed, the body
/// ends, or the download is cancelled. Whatever arrived is written before returning.
async fn stream(
    shared: &Shared,
    writer: &mut Writer,
    index: usize,
    from: u64,
    response: Response,
    mut skip: u64,
) -> Result<Ended> {
    let _connection = shared.progress.connection_opened();
    writer.start(index, from);
    let mut body = response.bytes_stream();
    let stall = shared.opts.stall_timeout;

    let outcome: Result<Ended> = loop {
        let next = tokio::select! {
            biased;
            _ = shared.cancel.cancelled() => break Ok(Ended::Cancelled),
            next = tokio::time::timeout(stall, body.next()) => next,
        };
        let chunk = match next {
            Err(_) => break Err(Error::Stalled(stall.as_secs())),
            Ok(None) => break Ok(Ended::Eof),
            Ok(Some(Err(e))) => break Err(e.into()),
            Ok(Some(Ok(chunk))) => chunk,
        };

        let mut data = &chunk[..];
        if skip > 0 {
            let n = skip.min(data.len() as u64);
            data = &data[n as usize..];
            skip -= n;
        }
        if data.is_empty() {
            continue;
        }

        let (allowed, done) = shared.table.lock().unwrap().claim(index, data.len() as u64);
        if allowed > 0 {
            if let Err(e) = writer.push(&data[..allowed as usize], &shared.table).await {
                break Err(e);
            }
            shared.progress.add(allowed);
            for limiter in &shared.opts.rate_limiters {
                limiter.consume(allowed).await;
            }
        }
        if done {
            break Ok(Ended::Done);
        }
    };

    writer.finish(&shared.table).await?;

    match outcome? {
        Ended::Eof => {
            let mut table = shared.table.lock().unwrap();
            let (at, end) = table.position(index);
            if end == UNKNOWN_END {
                table.close_unknown(index);
                Ok(Ended::Done)
            } else if at >= end {
                Ok(Ended::Done)
            } else {
                Err(Error::PrematureEof { at, end })
            }
        }
        other => Ok(other),
    }
}

/// 0.5 s, 1 s, 2 s ... capped at 10 s, with a little jitter so workers that
/// failed together do not retry in lockstep.
pub(crate) fn backoff(failures: u32) -> Duration {
    let base = 500u64
        .saturating_mul(1 << failures.saturating_sub(1).min(5))
        .min(10_000);
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_millis()) % 250)
        .unwrap_or(0);
    Duration::from_millis(base + jitter)
}

/// A finished write hands back its emptied buffer, the segment and the new flushed offset.
type WriteJob = JoinHandle<std::io::Result<(Vec<u8>, usize, u64)>>;

/// Collects incoming bytes into a large buffer and writes it with one positional
/// write on the blocking pool, while the next buffer fills (double buffering).
/// The segment's `flushed` offset only moves once a write has completed.
struct Writer {
    file: Arc<File>,
    capacity: usize,
    buf: Vec<u8>,
    spare: Option<Vec<u8>>,
    index: usize,
    /// File offset of `buf[0]`.
    offset: u64,
    in_flight: Option<WriteJob>,
}

impl Writer {
    fn new(file: Arc<File>, capacity: usize) -> Self {
        Self {
            file,
            capacity,
            buf: Vec::with_capacity(capacity),
            spare: None,
            index: 0,
            offset: 0,
            in_flight: None,
        }
    }

    fn start(&mut self, index: usize, offset: u64) {
        debug_assert!(self.buf.is_empty() && self.in_flight.is_none());
        self.index = index;
        self.offset = offset;
    }

    async fn push(&mut self, mut data: &[u8], table: &Mutex<SegmentTable>) -> Result<()> {
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

    /// Starts writing the current buffer; waits for the previous write first.
    async fn flush(&mut self, table: &Mutex<SegmentTable>) -> Result<()> {
        self.wait(table).await?;
        if self.buf.is_empty() {
            return Ok(());
        }
        let next = self
            .spare
            .take()
            .unwrap_or_else(|| Vec::with_capacity(self.capacity));
        let mut full = std::mem::replace(&mut self.buf, next);
        let (file, offset, index) = (self.file.clone(), self.offset, self.index);
        let end = offset + full.len() as u64;
        self.offset = end;
        self.in_flight = Some(tokio::task::spawn_blocking(move || {
            storage::write_all_at(&file, &full, offset)?;
            full.clear();
            Ok((full, index, end))
        }));
        Ok(())
    }

    async fn wait(&mut self, table: &Mutex<SegmentTable>) -> Result<()> {
        if let Some(handle) = self.in_flight.take() {
            let (buf, index, end) = handle.await.map_err(std::io::Error::other)??;
            table.lock().unwrap().mark_flushed(index, end);
            self.spare = Some(buf);
        }
        Ok(())
    }

    /// Writes everything buffered and waits until it is on disk.
    async fn finish(&mut self, table: &Mutex<SegmentTable>) -> Result<()> {
        self.flush(table).await?;
        self.wait(table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) < Duration::from_millis(800));
        assert!(backoff(3) >= Duration::from_secs(2));
        assert!(backoff(30) <= Duration::from_millis(10_250));
    }
}
