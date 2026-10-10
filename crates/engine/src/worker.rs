//! One connection's loop: take a range, stream it to disk, take the next.

use std::fs::File;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::download::Shared;
use crate::error::{Error, Result};
use crate::http::Failure;
use crate::segment::{Assignment, SegmentTable, UNKNOWN_END};
use crate::storage;
use crate::transport::{Body, Connection};

/// Consecutive refused attempts, while other connections of the same download
/// are receiving data, after which a connection gives up its part to them.
/// One refusal can be a fluke; two in a row mean the server allows no more
/// connections than are already open. A refusal that asks for a long wait
/// (`Retry-After`, or a delay beyond the normal backoff) steps back at once,
/// so the segment is not held hostage while the others could finish it.
const REFUSALS_BEFORE_STEPPING_BACK: u32 = 2;

/// The longest normal backoff; see [`backoff`].
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// How a stretch of streaming ended.
enum Ended {
    /// The segment is fully claimed.
    Done,
    Cancelled,
    /// The server closed the body.
    Eof,
    /// The server refuses this extra connection; the segment was handed back
    /// to the connections that are working.
    SteppedBack,
}

pub(crate) async fn run(
    id: usize,
    shared: Arc<Shared>,
    mut connection: Connection,
    mut first: Option<(Assignment, Body)>,
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

        let result = run_assignment(
            &shared,
            &mut connection,
            &mut writer,
            assignment.index,
            response,
        )
        .await;
        match result {
            Ok(Ended::Done) => continue,
            Ok(Ended::SteppedBack) => return Ok(()),
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
///
/// A 429 or 503 with `Retry-After` is retried no sooner than the server asked.
/// When the server keeps refusing this connection (or asks it to wait) while
/// others of the same download are receiving data, the connection steps back
/// instead of retrying: the server's connection limit is reached, and the
/// remaining connections finish the work.
async fn run_assignment(
    shared: &Shared,
    connection: &mut Connection,
    writer: &mut Writer,
    index: usize,
    mut response: Option<Body>,
) -> Result<Ended> {
    let mut failures = 0u32;
    let mut refusals = 0u32;
    loop {
        if shared.cancel.is_cancelled() {
            return Ok(Ended::Cancelled);
        }
        let (from, to) = shared.table.lock().unwrap().position(index);
        if from >= to {
            return Ok(Ended::Done);
        }

        let attempt = async {
            let (mut body, skip) = match response.take() {
                Some(body) => (body, 0),
                None => connection.open(shared, from, to).await?,
            };
            let ended = stream(shared, writer, index, from, &mut body, skip).await;
            connection.finish(body, &shared.cancel).await;
            Ok::<_, Failure>(ended?)
        };

        match attempt.await {
            Ok(ended) => return Ok(ended),
            // Waiting for an answer was cut short by a pause.
            Err(_) if shared.cancel.is_cancelled() => return Ok(Ended::Cancelled),
            // A refusal is retried even when the error alone would not be: an
            // FTP server turning away one more login already accepted the
            // same login on another connection.
            Err(f)
                if (f.error.is_retryable() || is_refusal(&f.error))
                    && failures < shared.opts.max_retries =>
            {
                let (now, _) = shared.table.lock().unwrap().position(index);
                let progressed = now > from;
                failures = if progressed { 0 } else { failures + 1 };
                refusals = if !progressed && is_refusal(&f.error) {
                    refusals + 1
                } else {
                    0
                };
                let delay = retry_delay(failures, f.retry_after);
                let long_wait = f.retry_after.is_some() || delay > MAX_BACKOFF;
                if refusals > 0
                    && (refusals >= REFUSALS_BEFORE_STEPPING_BACK || long_wait)
                    && step_back(shared, index)
                {
                    info!(error = %f.error, "server refuses another connection; continuing with the open ones");
                    return Ok(Ended::SteppedBack);
                }
                warn!(error = %f.error, retry_in = ?delay, "connection failed, retrying");
                tokio::select! {
                    _ = shared.cancel.cancelled() => return Ok(Ended::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(f) => return Err(f.error),
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
    body: &mut Body,
    mut skip: u64,
) -> Result<Ended> {
    let _connection = shared.progress.connection_opened();
    let _receiving = Receiving::new(&shared.receiving);
    writer.start(index, from);
    let stall = shared.opts.stall_timeout;

    let outcome: Result<Ended> = loop {
        let next = tokio::select! {
            biased;
            _ = shared.cancel.cancelled() => break Ok(Ended::Cancelled),
            next = tokio::time::timeout(stall, body.next()) => next,
        };
        let mut data = match next {
            Err(_) => break Err(Error::Stalled(stall.as_secs())),
            Ok(Ok(None)) => break Ok(Ended::Eof),
            Ok(Err(e)) => break Err(e),
            Ok(Ok(Some(chunk))) => chunk,
        };

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

/// Whether a failed attempt that received nothing looks like the server turning
/// this connection away: busy statuses, or a connection that was accepted and
/// then closed or reset. Timeouts are not refusals; a slow network is not a limit.
fn is_refusal(error: &Error) -> bool {
    match error {
        Error::HttpStatus { status, .. } => matches!(status, 429 | 503),
        Error::Network(e) => !e.is_timeout(),
        // No answer in time is not a refusal either.
        Error::Stalled(_) => false,
        Error::PrematureEof { .. } => true,
        // 421: too many connections. 530 on a later connection: this login
        // worked on the first one, so the server limits sessions. 425: the
        // server would not open another data connection.
        Error::FtpReply { code, .. } => matches!(code, 421 | 425 | 530),
        _ => false,
    }
}

/// Hands segment `index` back for the other connections if at least one is
/// receiving data and another segment's owner still has bytes to fetch (so it
/// will come back for more work and pick this segment up). Decided under the
/// table lock, so the segment can never be left without a connection to take it.
fn step_back(shared: &Shared, index: usize) -> bool {
    let mut table = shared.table.lock().unwrap();
    let others_working = shared.receiving.load(Ordering::SeqCst) > 0 && table.others_active(index);
    if others_working {
        table.release(index);
    }
    others_working
}

/// Counts a connection as receiving while it streams a body.
struct Receiving<'a>(&'a AtomicUsize);

impl<'a> Receiving<'a> {
    fn new(count: &'a AtomicUsize) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

impl Drop for Receiving<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How long to wait before the next attempt: what the server asked for in
/// `Retry-After` if it said, otherwise [`backoff`]. Jitter is only ever added,
/// so a server's delay is never cut short.
pub(crate) fn retry_delay(failures: u32, retry_after: Option<Duration>) -> Duration {
    match retry_after {
        Some(delay) => delay + jitter(),
        None => backoff(failures),
    }
}

/// 0.5 s, 1 s, 2 s ... capped at 10 s, with a little jitter so workers that
/// failed together do not retry in lockstep.
pub(crate) fn backoff(failures: u32) -> Duration {
    let base = 500u64
        .saturating_mul(1 << failures.saturating_sub(1).min(5))
        .min(MAX_BACKOFF.as_millis() as u64);
    Duration::from_millis(base) + jitter()
}

/// 0 to 249 ms.
fn jitter() -> Duration {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_millis()) % 250)
        .unwrap_or(0);
    Duration::from_millis(ms)
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
            let (buf, index, end) = handle.await.map_err(Error::task)??;
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

    #[test]
    fn retry_after_is_never_shortened() {
        let asked = Duration::from_secs(3);
        let delay = retry_delay(1, Some(asked));
        assert!(delay >= asked && delay < asked + Duration::from_millis(250));
        assert!(retry_delay(1, None) < Duration::from_millis(800));
    }
}
