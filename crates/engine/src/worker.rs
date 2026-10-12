//! One connection's loop: take a range, stream it to disk, take the next.

use std::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::{debug, info, warn};

use crate::download::Shared;
use crate::error::{Error, Result};
use crate::http::Failure;
use crate::limiter::{Paced, pace};
use crate::numeric::clamp_to_len;
use crate::receive::{Received, receive, skip_start};
use crate::retry::{Attempts, Next, Refusals};
use crate::segment::{Assignment, UNKNOWN_END};
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
                    Next::StepBackOrRetry(_) if step_back(shared, index) => {
                        info!(error = %f.error, "server refuses another connection; continuing with the open ones");
                        return Ok(Ended::SteppedBack);
                    }
                    Next::Retry(delay) | Next::StepBackOrRetry(delay) => delay,
                };
                warn!(error = %f.error, retry_in = ?delay, "connection failed, retrying");
                tokio::select! {
                    () = shared.request.cancel.cancelled() => return Ok(Ended::Cancelled),
                    () = tokio::time::sleep(delay) => {}
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
    let _receiving = Receiving::new(&shared.receiving);
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
