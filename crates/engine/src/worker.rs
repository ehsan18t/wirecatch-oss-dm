//! One connection's loop: take a range, stream it to disk, take the next.

use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::download::Shared;
use crate::error::{Error, Result};
use crate::http::Failure;
use crate::limiter::{Paced, pace};
use crate::numeric::{clamp_to_len, saturating_cast};
use crate::segment::{Assignment, ByteRange, SharedTable, UNKNOWN_END};
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

/// The pause after the first failure; it doubles with each failure after it.
const FIRST_BACKOFF: Duration = Duration::from_millis(500);

/// The most random delay added to a backoff, so connections that failed
/// together spread their next attempts out.
const MAX_JITTER: Duration = Duration::from_millis(250);

/// Most times the backoff doubles, which keeps the shift small; [`MAX_BACKOFF`]
/// caps it sooner anyway.
const MAX_DOUBLINGS: u32 = 5;

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
    /// HTTP/3 failed and was given up for the download; the segment carries
    /// on over TCP from where it got to, without counting as a failed attempt.
    SwitchedProtocol,
    /// Everything one request asked for arrived, but the segment goes on past
    /// it ([`DownloadOptions::max_request_bytes`](crate::DownloadOptions::max_request_bytes)):
    /// the next request continues from there on the same connection.
    RequestDone,
}

/// Runs connection `id` of a download, writing through its own handle `file`
/// on the part file, until no work is left, the download is cancelled or the
/// connection gives up. `first` is work already under way: a range and the
/// response streaming it.
pub(crate) async fn run(
    id: usize,
    shared: Arc<Shared>,
    file: File,
    mut connection: Connection,
    mut first: Option<(Assignment, Body)>,
) -> Result<()> {
    let mut writer = Writer::new(Arc::new(file), shared.opts.buffer_size);

    loop {
        let (assignment, response) = match first.take() {
            Some((assignment, response)) => (assignment, Some(response)),
            None => match shared.table.lock().acquire(id) {
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
            // `run_assignment` deals with a protocol switch and the end of one
            // request itself.
            Ok(Ended::Done | Ended::SwitchedProtocol | Ended::RequestDone) => {}
            Ok(Ended::SteppedBack) => return Ok(()),
            Ok(Ended::Cancelled | Ended::Eof) => {
                shared.table.lock().release(assignment.index);
                return Ok(());
            }
            Err(e) => {
                shared.table.lock().release(assignment.index);
                return Err(e);
            }
        }
    }
}

/// Downloads one segment, retrying transient failures from wherever it got to.
/// With a limit on the size of one request, the segment is fetched as a series
/// of requests, each sent once the one before has arrived in full.
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
    let mut attempts = Attempts::default();
    loop {
        if shared.cancel.is_cancelled() {
            return Ok(Ended::Cancelled);
        }
        let (from, to) = shared.table.lock().position(index);
        if from >= to {
            return Ok(Ended::Done);
        }
        // A response handed over with the assignment (the first request's)
        // asked for the same: it started at `from` with the same limit.
        let end = shared.request_end(from, to);

        let attempt = async {
            let (mut body, skip) = match response.take() {
                Some(body) => (body, 0),
                None => connection.open(shared, from, end).await?,
            };
            let ended = stream(shared, writer, index, from, end, &mut body, skip).await;
            connection.finish(body, &shared.cancel).await;
            Ok::<_, Failure>(ended?)
        };

        match attempt.await {
            Ok(Ended::SwitchedProtocol) => {}
            // Progress: failures before it no longer count.
            Ok(Ended::RequestDone) => attempts = Attempts::default(),
            Ok(ended) => return Ok(ended),
            // Waiting for an answer was cut short by a pause.
            Err(_) if shared.cancel.is_cancelled() => return Ok(Ended::Cancelled),
            Err(f) => {
                let (now, _) = shared.table.lock().position(index);
                let next = attempts.after_failure(
                    &f.error,
                    f.retry_after,
                    now > from,
                    shared.opts.max_retries,
                );
                let delay = match next {
                    Next::GiveUp => return Err(f.error),
                    Next::StepBackOrRetry(_) if step_back(shared, index) => {
                        info!(error = %f.error, "server refuses another connection; continuing with the open ones");
                        return Ok(Ended::SteppedBack);
                    }
                    Next::Retry(delay) | Next::StepBackOrRetry(delay) => delay,
                };
                warn!(error = %f.error, retry_in = ?delay, "connection failed, retrying");
                tokio::select! {
                    () = shared.cancel.cancelled() => return Ok(Ended::Cancelled),
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

/// What a connection does after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Try again after this long.
    Retry(Duration),
    /// The server keeps refusing this connection: hand the segment to the
    /// connections that are working if there are any, else retry after this
    /// long.
    StepBackOrRetry(Duration),
    GiveUp,
}

/// A connection's failed attempts in a row.
#[derive(Debug, Default)]
struct Attempts {
    /// Attempts in a row that received nothing.
    failures: u32,
    /// Of those, attempts the server turned away.
    refusals: u32,
}

impl Attempts {
    /// Decides what follows a failed attempt. `progressed` says whether it
    /// received anything, which resets the count.
    ///
    /// A refusal is retried even when the error alone would not be: an FTP
    /// server turning away one more login (530) already accepted the same
    /// login on another connection.
    fn after_failure(
        &mut self,
        error: &Error,
        retry_after: Option<Duration>,
        progressed: bool,
        max_retries: u32,
    ) -> Next {
        let refused = is_refusal(error);
        if !(error.is_retryable() || refused) {
            return Next::GiveUp;
        }
        // Only attempts that received nothing count against the budget
        // (`DownloadOptions::max_retries`).
        if progressed {
            self.failures = 0;
        } else if self.failures >= max_retries {
            return Next::GiveUp;
        } else {
            self.failures += 1;
        }
        self.refusals = if !progressed && refused {
            self.refusals + 1
        } else {
            0
        };
        let delay = retry_delay(self.failures, retry_after);
        let long_wait = retry_after.is_some() || delay > MAX_BACKOFF;
        if self.refusals > 0 && (self.refusals >= REFUSALS_BEFORE_STEPPING_BACK || long_wait) {
            Next::StepBackOrRetry(delay)
        } else {
            Next::Retry(delay)
        }
    }
}

/// Streams a response body into the segment until it is fully claimed, the body
/// ends, or the download is cancelled. Whatever arrived is written before returning.
/// `request_end` is where the request for the body ended; a body that ends there
/// before the segment does is complete ([`Ended::RequestDone`]), not cut short.
async fn stream(
    shared: &Shared,
    writer: &mut Writer,
    index: usize,
    from: u64,
    request_end: u64,
    body: &mut Body,
    mut skip: u64,
) -> Result<Ended> {
    let _connection = shared.progress.connection_opened();
    let _receiving = Receiving::new(&shared.receiving);
    writer.start(index, from);
    let stall = shared.opts.stall_timeout;
    // Whether receiving failed (rather than, say, writing to disk).
    let mut body_failed = false;

    let outcome: Result<Ended> = loop {
        let next = tokio::select! {
            biased;
            () = shared.cancel.cancelled() => break Ok(Ended::Cancelled),
            next = tokio::time::timeout(stall, body.next()) => next,
        };
        let mut data = match next {
            Err(_) => {
                body_failed = true;
                break Err(Error::Stalled(stall.as_secs()));
            }
            Ok(Ok(None)) => break Ok(Ended::Eof),
            Ok(Err(e)) => {
                body_failed = true;
                break Err(e);
            }
            Ok(Ok(Some(chunk))) => chunk,
        };

        if skip > 0 {
            let n = clamp_to_len(skip, data.len());
            data = &data[n..];
            skip -= n as u64;
        }
        if data.is_empty() {
            continue;
        }

        let (allowed, done) = shared.table.lock().claim(index, data.len() as u64);
        if allowed > 0 {
            let allowed_len = clamp_to_len(allowed, data.len());
            if let Err(e) = writer.push(&data[..allowed_len], &shared.table).await {
                break Err(e);
            }
            shared.progress.add(allowed);
            if pace(&shared.opts.rate_limiters, allowed, &shared.cancel).await == Paced::Cancelled {
                break Ok(Ended::Cancelled);
            }
        }
        if done {
            break Ok(Ended::Done);
        }
    };

    let finished = writer.finish(&shared.table).await;
    // Bytes counted as received but dropped by a failed write are fetched again by whoever resumes.
    shared.progress.take_back(writer.take_dropped());
    finished?;

    // A failure of HTTP/3 itself: the range carries on over TCP.
    let outcome = match outcome {
        Err(e) if body_failed && body.abandon_http3(&e) => Ok(Ended::SwitchedProtocol),
        other => other,
    };
    match outcome? {
        Ended::Eof => {
            let mut table = shared.table.lock();
            let (at, end) = table.position(index);
            if end == UNKNOWN_END {
                table.close_unknown(index);
                Ok(Ended::Done)
            } else if at >= end {
                Ok(Ended::Done)
            } else if at >= request_end {
                Ok(Ended::RequestDone)
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
        // No answer in time (`Stalled`) is not a refusal either.
        Error::Network(e) => !e.is_timeout(),
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
    let mut table = shared.table.lock();
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

/// 0.5 s, 1 s, 2 s ... capped at 10 s, plus [`jitter`].
pub(crate) fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(MAX_DOUBLINGS);
    FIRST_BACKOFF
        .saturating_mul(1 << doublings)
        .min(MAX_BACKOFF)
        + jitter()
}

/// A random delay below [`MAX_JITTER`], different on every call: connections
/// that fail at the same moment must not come back at the same moment.
fn jitter() -> Duration {
    // Every `RandomState` is keyed apart from the last one, so a hash made with a
    // new one is a fresh random number, without a dependency just for this.
    let random = RandomState::new().build_hasher().finish();
    let span: u64 = saturating_cast(MAX_JITTER.as_nanos());
    Duration::from_nanos(random % span)
}

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
struct Writer {
    file: Arc<File>,
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
            failed: false,
            unwritten: 0,
            dropped: 0,
        }
    }

    fn start(&mut self, index: usize, offset: u64) {
        debug_assert!(self.buf.is_empty() && self.in_flight.is_none());
        self.index = index;
        self.offset = offset;
        self.failed = false;
    }

    /// Takes `data` to write. Once a write has failed, data is dropped (and counted in [`Writer::take_dropped`]) rather than buffered, since nothing more of this segment may be written.
    async fn push(&mut self, mut data: &[u8], table: &SharedTable) -> Result<()> {
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
        self.in_flight = Some(tokio::task::spawn_blocking(move || {
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
            .and_then(|r| r.map_err(Error::from))
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
    fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    /// Writes everything buffered and waits until it is on disk.
    async fn finish(&mut self, table: &SharedTable) -> Result<()> {
        self.flush(table).await?;
        self.wait(table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::SegmentTable;

    /// A write failed while the next buffer was already full. Writing that buffer on the way out used to land it above the hole and count the hole as flushed, so a resumed or finished download could keep a range of zeros.
    #[tokio::test]
    async fn a_failed_write_is_never_covered_by_the_next_one() {
        const CAPACITY: usize = 4;
        let dir = wirecatch_test_support::private_dir("writer").unwrap();
        let path = dir.path().join("f.bin.wcpart");
        let file = Arc::new(File::create(&path).unwrap());
        let table = SharedTable::new(SegmentTable::new(Some(16), 1));
        table.lock().acquire(0).unwrap();
        table.lock().claim(0, 8);

        let mut writer = Writer::new(Arc::clone(&file), CAPACITY);
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

        // A push after the failure used to loop for ever: the full buffer was never emptied.
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

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) < FIRST_BACKOFF + MAX_JITTER);
        assert!(backoff(3) >= Duration::from_secs(2));
        assert!(backoff(30) < MAX_BACKOFF + MAX_JITTER);
    }

    fn ftp(code: u16) -> Error {
        Error::FtpReply {
            code,
            message: "test".into(),
            url: "ftp://h/f".into(),
        }
    }

    /// What two failed attempts in a row, receiving nothing, lead to.
    fn refused_twice(error: &Error) -> (Next, Next) {
        let mut attempts = Attempts::default();
        let first = attempts.after_failure(error, None, false, 6);
        let second = attempts.after_failure(error, None, false, 6);
        (first, second)
    }

    #[test]
    fn ftp_session_limits_count_as_refusals() {
        // 421 too many connections, 425 no data connection, and 530 for a
        // login that worked on another connection: the second in a row steps
        // back, so the connections that are working finish the file.
        for code in [421, 425, 530] {
            let (first, second) = refused_twice(&ftp(code));
            assert!(matches!(first, Next::Retry(_)), "{code}: {first:?}");
            assert!(
                matches!(second, Next::StepBackOrRetry(_)),
                "{code}: {second:?}"
            );
        }
        // 530 alone is not retryable; only the refusal rule retries it.
        assert!(!ftp(530).is_retryable());
        // Other 4xx replies are retried but are not refusals.
        let (_, second) = refused_twice(&ftp(450));
        assert!(matches!(second, Next::Retry(_)), "{second:?}");
        // A permanent failure ends at once.
        let mut attempts = Attempts::default();
        assert_eq!(
            attempts.after_failure(&ftp(550), None, false, 6),
            Next::GiveUp
        );
    }

    #[test]
    fn the_retry_budget_and_progress() {
        let mut attempts = Attempts::default();
        assert_eq!(
            attempts.after_failure(&ftp(421), None, false, 0),
            Next::GiveUp
        );
        let mut attempts = Attempts::default();
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, false, 1),
            Next::Retry(_)
        ));
        assert_eq!(
            attempts.after_failure(&ftp(450), None, false, 1),
            Next::GiveUp
        );
        // An attempt that received something does not count, and starts the
        // count again.
        let mut attempts = Attempts::default();
        attempts.after_failure(&ftp(450), None, false, 1);
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, true, 1),
            Next::Retry(_)
        ));
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, false, 1),
            Next::Retry(_)
        ));
        // A refusal that asks for a long wait steps back at once.
        let mut attempts = Attempts::default();
        let busy = Error::HttpStatus {
            status: 503,
            url: "https://h/f".into(),
        };
        assert!(matches!(
            attempts.after_failure(&busy, Some(Duration::from_secs(5)), false, 6),
            Next::StepBackOrRetry(_)
        ));
    }

    /// Connections that fail together ask for their delay within the same
    /// millisecond; they must still not all retry at the same moment.
    #[test]
    fn connections_failing_together_get_different_delays() {
        let delays: std::collections::HashSet<Duration> = (0..16).map(|_| backoff(1)).collect();
        assert!(
            delays.len() >= 8,
            "16 delays asked for at once, only {} different",
            delays.len()
        );
        assert!(
            delays
                .iter()
                .all(|d| *d >= FIRST_BACKOFF && *d < FIRST_BACKOFF + MAX_JITTER)
        );
    }

    #[test]
    fn retry_after_is_never_shortened() {
        let asked = Duration::from_secs(3);
        let delay = retry_delay(1, Some(asked));
        assert!(delay >= asked && delay < asked + MAX_JITTER);
        assert!(retry_delay(1, None) < Duration::from_millis(800));
    }
}
