//! One connection's loop: take a range, stream it to disk, take the next.

use std::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::download::Shared;
use crate::error::{Chain, Error, Result};
use crate::http::Failure;
use crate::limiter::{Paced, pace};
use crate::numeric::clamp_to_len;
use crate::receive::{Received, receive, skip_start};
use crate::retry::{Attempts, Next, Refusals};
use crate::segment::{Assignment, SharedTable, UNKNOWN_END};
use crate::transport::{Body, Connection};
use crate::writer::Writer;

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
    let mut writer = Writer::new(
        Arc::new(file),
        shared.part_path.clone(),
        shared.opts.buffer_size,
        shared.file_work.clone(),
    );

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
/// others of the same download are receiving data, or start to while it waits,
/// the connection steps back instead of retrying: the server's connection limit
/// is reached, and the remaining connections finish the work.
async fn run_assignment(
    shared: &Shared,
    connection: &mut Connection,
    writer: &mut Writer,
    index: usize,
    mut response: Option<Body>,
) -> Result<Ended> {
    let mut attempts = Attempts::new(Refusals::StepBack, shared.opts.max_retries);
    loop {
        if shared.request.cancel.is_cancelled() {
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
                None => {
                    connection
                        .open(&shared.remote, &shared.request, from, end)
                        .await?
                }
            };
            let ended = stream(shared, writer, index, from, end, &mut body, skip).await;
            connection.finish(body, &shared.request.cancel).await;
            Ok::<_, Failure>(ended?)
        };

        match attempt.await {
            Ok(Ended::SwitchedProtocol) => {}
            // Progress: failures before it no longer count.
            Ok(Ended::RequestDone) => {
                attempts = Attempts::new(Refusals::StepBack, shared.opts.max_retries);
            }
            Ok(ended) => return Ok(ended),
            // Waiting for an answer was cut short by a pause.
            Err(_) if shared.request.cancel.is_cancelled() => return Ok(Ended::Cancelled),
            Err(f) => {
                let (now, _) = shared.table.lock().position(index);
                let next = attempts.after_failure(&f.error, f.retry_after, now > from);
                let delay = match next {
                    Next::GiveUp => return Err(f.error),
                    Next::StepBackOrRetry(_)
                        if step_back(&shared.table, &shared.receiving, index) =>
                    {
                        info!(error = %Chain(&f.error), "server refuses another connection; continuing with the open ones");
                        return Ok(Ended::SteppedBack);
                    }
                    Next::Retry(delay) | Next::StepBackOrRetry(delay) => delay,
                };
                warn!(error = %Chain(&f.error), retry_in = ?delay, "connection failed, retrying");
                let cancel = &shared.request.cancel;
                let waited = match next {
                    Next::StepBackOrRetry(_) => {
                        wait_or_step_back(&shared.table, &shared.receiving, cancel, index, delay)
                            .await
                    }
                    _ => wait(cancel, delay).await,
                };
                match waited {
                    Waited::Out => {}
                    Waited::Cancelled => return Ok(Ended::Cancelled),
                    Waited::SteppedBack => {
                        info!("another connection is receiving now; continuing with the open ones");
                        return Ok(Ended::SteppedBack);
                    }
                }
            }
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
    let _receiving = shared.receiving.start();
    writer.start(index, from);
    let stall = shared.request.stall_timeout;
    // Whether receiving failed (rather than, say, writing to disk).
    let mut body_failed = false;

    let outcome: Result<Ended> = loop {
        let data = match receive(&shared.request.cancel, stall, body.next()).await {
            Ok(Received::Data(data)) => skip_start(data, &mut skip),
            Ok(Received::End) => break Ok(Ended::Eof),
            Ok(Received::Cancelled) => break Ok(Ended::Cancelled),
            Err(e) => {
                body_failed = true;
                break Err(e);
            }
        };
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
            if pace(&shared.opts.rate_limiters, allowed, &shared.request.cancel).await
                == Paced::Cancelled
            {
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

/// How a wait before the next attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Waited {
    /// The delay is over: try again.
    Out,
    Cancelled,
    /// Another connection started receiving, and the segment went back to the
    /// table for it.
    SteppedBack,
}

/// Waits `delay` before the next attempt, unless the download is cancelled first.
async fn wait(cancel: &CancellationToken, delay: Duration) -> Waited {
    tokio::select! {
        () = cancel.cancelled() => Waited::Cancelled,
        () = tokio::time::sleep(delay) => Waited::Out,
    }
}

/// Waits `delay` before connection `index` asks again, as a connection the
/// server keeps refusing: it steps back for the others as soon as one of them
/// is receiving data, not only if one is at the moment of the refusal. The
/// connections the server does serve may all be waiting for a response just
/// then, and the delay can be minutes (`Retry-After`), so waiting it out would
/// hold the segment while they could fetch it.
async fn wait_or_step_back(
    table: &SharedTable,
    receiving: &Receivers,
    cancel: &CancellationToken,
    index: usize,
    delay: Duration,
) -> Waited {
    let deadline = Instant::now() + delay;
    loop {
        // Registered before the check, so a connection starting in between is not missed.
        let started = receiving.started.notified();
        if step_back(table, receiving, index) {
            return Waited::SteppedBack;
        }
        tokio::select! {
            () = cancel.cancelled() => return Waited::Cancelled,
            () = tokio::time::sleep_until(deadline) => return Waited::Out,
            () = started => {}
        }
    }
}

/// Hands segment `index` back for the other connections if at least one is
/// receiving data and another segment's owner still has bytes to fetch (so it
/// will come back for more work and pick this segment up). Decided under the
/// table lock, so the segment can never be left without a connection to take it.
fn step_back(table: &SharedTable, receiving: &Receivers, index: usize) -> bool {
    let mut table = table.lock();
    let others_working = receiving.any() && table.others_active(index);
    if others_working {
        table.release(index);
    }
    others_working
}

/// The connections of one download that are receiving data.
#[derive(Debug, Default)]
pub(crate) struct Receivers {
    count: AtomicUsize,
    /// Wakes the connections waiting to step back when one starts receiving.
    started: Notify,
}

impl Receivers {
    /// Counts a connection as receiving until the guard is dropped.
    fn start(&self) -> Receiving<'_> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.started.notify_waiters();
        Receiving(self)
    }

    fn any(&self) -> bool {
        self.count.load(Ordering::SeqCst) > 0
    }
}

/// Counts a connection as receiving while it streams a body.
struct Receiving<'a>(&'a Receivers);

impl Drop for Receiving<'_> {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::SegmentTable;
    use crate::units::MIB;

    /// A wait as long as a refusal's `Retry-After` may ask for.
    const LONG_WAIT: Duration = Duration::from_secs(300);

    /// When the other connection starts receiving, on the paused clock.
    const ANOTHER_STARTS_AFTER: Duration = Duration::from_secs(1);

    /// The connection that owns segment `mine` waits as a refused one would,
    /// while another connection starts receiving a second in. How the wait
    /// ended, and how long it took.
    async fn refused_while_another_starts(table: &SharedTable, mine: usize) -> (Waited, Duration) {
        let receiving = Receivers::default();
        let cancel = CancellationToken::new();
        let started = Instant::now();
        let another_starts = async {
            tokio::time::sleep(ANOTHER_STARTS_AFTER).await;
            receiving.start()
        };
        let (waited, _receiving) = tokio::join!(
            wait_or_step_back(table, &receiving, &cancel, mine, LONG_WAIT),
            another_starts,
        );
        (waited, started.elapsed())
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_connection_steps_back_once_another_starts_receiving() {
        // Another connection owns a segment with bytes left, but is not
        // receiving when the refusal comes: it waits for its response.
        let mut table = SegmentTable::new(Some(8 * MIB), MIB);
        let mine = table.acquire(0).unwrap().index;
        table.acquire(1).unwrap();
        let table = SharedTable::new(table);

        let (waited, took) = refused_while_another_starts(&table, mine).await;

        assert_eq!(waited, Waited::SteppedBack);
        assert_eq!(took, ANOTHER_STARTS_AFTER, "held its segment for the wait");
        let next = table.lock().acquire(1).map(|a| a.index);
        assert_eq!(next, Some(mine), "the segment was not handed back");
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_connection_keeps_the_last_segment_and_waits() {
        // No other segment has bytes left: the connection receiving will not
        // come back for more work, so this one keeps its segment.
        let mut table = SegmentTable::new(Some(8 * MIB), MIB);
        let mine = table.acquire(0).unwrap().index;
        let table = SharedTable::new(table);

        let (waited, took) = refused_while_another_starts(&table, mine).await;

        assert_eq!(waited, Waited::Out);
        assert!(took >= LONG_WAIT, "retried after {took:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_ends_a_refused_connections_wait() {
        let mut table = SegmentTable::new(Some(8 * MIB), MIB);
        let mine = table.acquire(0).unwrap().index;
        let table = SharedTable::new(table);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let waited =
            wait_or_step_back(&table, &Receivers::default(), &cancel, mine, LONG_WAIT).await;

        assert_eq!(waited, Waited::Cancelled);
    }
}
