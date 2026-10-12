//! Speed limiting shared across connections and downloads.

use std::pin::pin;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::numeric::approx_f64;
use crate::units::KIB;

/// Seconds of the limit a limiter that sat idle may release at once: a quarter
/// second smooths chunk-sized jitter without letting a spike through.
const BURST_SECONDS: f64 = 0.25;

/// The least burst, in bytes, so that a low limit still lets one ordinary chunk
/// through without a wait.
const MIN_BURST: u64 = 16 * KIB;

/// Stands in for a wait too long to represent (bytes far beyond any chunk at a
/// limit of a few bytes a second): a year, which the caller never sits out
/// whole, since a pause or a new limit ends it.
const LONGEST_WAIT: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// Caps the combined speed of every connection that shares it.
///
/// Give one limiter to all downloads for a global limit, and another to the
/// downloads of one queue for a per-queue limit; a download honours every
/// limiter in [`DownloadOptions::rate_limiters`](crate::DownloadOptions).
/// The limit can change at any time and takes effect within one chunk.
#[derive(Debug)]
pub struct RateLimiter {
    bucket: Mutex<Bucket>,
    /// Wakes every connection waiting on this limiter when the limit changes,
    /// so it waits by the new limit rather than the old one.
    changed: Notify,
}

#[derive(Debug)]
struct Bucket {
    /// Bytes per second; `None` is unlimited.
    limit: Option<u64>,
    /// May go negative: callers take their bytes at once and sleep off the debt,
    /// so later callers queue up behind them instead of starving.
    tokens: f64,
    last: Instant,
}

impl Bucket {
    /// The tokens at `now`, refilled at `rate` bytes per second since `last`
    /// and capped at the burst that rate allows.
    fn refilled(&self, rate: f64, now: Instant) -> f64 {
        let burst = (rate * BURST_SECONDS).max(approx_f64(MIN_BURST));
        let refill = now.duration_since(self.last).as_secs_f64() * rate;
        (self.tokens + refill).min(burst)
    }
}

impl RateLimiter {
    /// `limit` in bytes per second; `None` is unlimited. A limit of 0 is treated as 1.
    #[must_use]
    pub fn new(limit: Option<u64>) -> Self {
        Self {
            bucket: Mutex::new(Bucket {
                limit: normalize(limit),
                tokens: 0.0,
                last: Instant::now(),
            }),
            changed: Notify::new(),
        }
    }

    /// The current limit in bytes per second; `None` is unlimited.
    pub fn limit(&self) -> Option<u64> {
        self.bucket().limit
    }

    /// Changes the limit (bytes per second; `None` is unlimited). Every
    /// connection sharing this limiter follows the new limit within one chunk.
    pub fn set_limit(&self, limit: Option<u64>) {
        {
            let mut bucket = self.bucket();
            let now = Instant::now();
            let new = normalize(limit);
            bucket.tokens = match (bucket.limit, new) {
                // What the old limit already paid off counts; a burst saved up
                // under it does not carry over to the new one.
                (Some(old), Some(_)) => bucket.refilled(approx_f64(old), now).min(0.0),
                // Without a limit before or after, nothing is owed.
                _ => 0.0,
            };
            bucket.limit = new;
            bucket.last = now;
        }
        self.changed.notify_waiters();
    }

    /// Locks the bucket. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives.
    fn bucket(&self) -> MutexGuard<'_, Bucket> {
        self.bucket.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Accounts for `bytes` just received, waiting as long as needed to stay
    /// under the limit. Not reading from the socket meanwhile lets TCP flow
    /// control slow the sender down.
    ///
    /// The wait ends early when `cancel` is cancelled, and is worked out again
    /// whenever the limit changes: the bytes still owed are paid at the new
    /// limit, or not at all once it is lifted.
    pub(crate) async fn consume(&self, bytes: u64, cancel: &CancellationToken) -> Paced {
        // Registered before the debt is read, so a change in between is not missed.
        let mut changed = pin!(self.changed.notified());
        let Some(mut debt) = self.take(bytes) else {
            return Paced::Done;
        };
        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Paced::Cancelled,
                () = tokio::time::sleep_until(debt.until) => return Paced::Done,
                () = changed.as_mut() => {}
            }
            changed.set(self.changed.notified());
            match self.limit() {
                Some(rate) => debt = debt.at(approx_f64(rate)),
                None => return Paced::Done,
            }
        }
    }

    /// Takes `bytes` from the bucket; the wait that pays off the debt it leaves,
    /// if any.
    fn take(&self, bytes: u64) -> Option<Debt> {
        let mut bucket = self.bucket();
        let rate = approx_f64(bucket.limit?);
        let now = Instant::now();
        bucket.tokens = bucket.refilled(rate, now) - approx_f64(bytes);
        bucket.last = now;
        if bucket.tokens >= 0.0 {
            return None;
        }
        Some(Debt {
            until: after(now, -bucket.tokens / rate),
            rate,
        })
    }
}

/// Accounts for `bytes` just received with every limiter in `limiters`, one
/// after the other; see [`RateLimiter::consume`].
pub(crate) async fn pace(
    limiters: &[std::sync::Arc<RateLimiter>],
    bytes: u64,
    cancel: &CancellationToken,
) -> Paced {
    for limiter in limiters {
        if limiter.consume(bytes, cancel).await == Paced::Cancelled {
            return Paced::Cancelled;
        }
    }
    Paced::Done
}

/// How waiting for the speed limit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Paced {
    /// The bytes are paid for; carry on.
    Done,
    /// The download was cancelled while waiting.
    Cancelled,
}

/// A wait for the limit: until when, at which rate in bytes per second.
#[derive(Debug, Clone, Copy)]
struct Debt {
    until: Instant,
    rate: f64,
}

impl Debt {
    /// The same bytes still owed, paid at `rate` from now on.
    fn at(self, rate: f64) -> Self {
        let now = Instant::now();
        let owed = self.until.saturating_duration_since(now).as_secs_f64() * self.rate;
        Self {
            until: after(now, owed / rate),
            rate,
        }
    }
}

/// `secs` seconds after `now`, or [`LONGEST_WAIT`] after it for a wait that
/// cannot be represented.
fn after(now: Instant, secs: f64) -> Instant {
    let wait = match Duration::try_from_secs_f64(secs) {
        Ok(wait) => wait.min(LONGEST_WAIT),
        Err(_) => LONGEST_WAIT,
    };
    now + wait
}

fn normalize(limit: Option<u64>) -> Option<u64> {
    limit.map(|l| l.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn holds_throughput_to_the_limit() {
        let limiter = RateLimiter::new(Some(100_000));
        let start = Instant::now();
        let cancel = CancellationToken::new();
        for _ in 0..100 {
            assert_eq!(limiter.consume(10_000, &cancel).await, Paced::Done);
        }
        // 1 MB at 100 kB/s is 10 s, minus at most the burst allowance.
        let elapsed = start.elapsed().as_secs_f64();
        assert!((9.5..=10.1).contains(&elapsed), "took {elapsed} s");
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_never_waits_and_limit_can_change() {
        let limiter = RateLimiter::new(None);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        limiter.consume(u64::MAX / 2, &cancel).await;
        assert_eq!(start.elapsed(), Duration::ZERO);

        limiter.set_limit(Some(1_000));
        limiter.consume(2_000, &cancel).await;
        assert!(start.elapsed() >= Duration::from_millis(1_900));
        assert_eq!(limiter.limit(), Some(1_000));
    }

    /// A limiter `secs` seconds in debt at 1 kB/s, and the task paying it off.
    fn in_debt(
        secs: u64,
    ) -> (
        std::sync::Arc<RateLimiter>,
        CancellationToken,
        tokio::task::JoinHandle<Paced>,
    ) {
        let limiter = std::sync::Arc::new(RateLimiter::new(Some(1_000)));
        let cancel = CancellationToken::new();
        let paying = {
            let (limiter, cancel) = (limiter.clone(), cancel.clone());
            tokio::spawn(async move { limiter.consume(secs * 1_000, &cancel).await })
        };
        (limiter, cancel, paying)
    }

    #[tokio::test(start_paused = true)]
    async fn a_lifted_limit_leaves_no_debt_behind() {
        let (limiter, _cancel, paying) = in_debt(60);
        tokio::time::sleep(Duration::from_secs(1)).await;
        limiter.set_limit(None);
        assert_eq!(paying.await.unwrap(), Paced::Done);
        limiter.set_limit(Some(1_000));
        let start = Instant::now();
        let cancel = CancellationToken::new();
        assert_eq!(limiter.consume(1, &cancel).await, Paced::Done);
        // One byte at 1 kB/s takes a millisecond; the lifted debt was a minute.
        assert!(
            start.elapsed() <= Duration::from_millis(1),
            "the lifted debt was charged: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_raised_limit_keeps_what_the_old_one_already_paid() {
        // 2 s owed at 1 kB/s; after 1 s, 1 kB is still owed, 0.5 s at 2 kB/s.
        let limiter = RateLimiter::new(Some(1_000));
        let cancel = CancellationToken::new();
        let start = Instant::now();
        let paying = limiter.consume(2_000, &cancel);
        let raising = async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            limiter.set_limit(Some(2_000));
        };
        let (paced, ()) = tokio::join!(paying, raising);
        assert_eq!(paced, Paced::Done);
        let waited = start.elapsed().as_secs_f64();
        assert!((1.4..=1.6).contains(&waited), "waited {waited} s");
        // The next 1 kB is owed after that debt, not on top of it twice.
        let start = Instant::now();
        limiter.consume(1_000, &cancel).await;
        let waited = start.elapsed().as_secs_f64();
        assert!(waited <= 0.6, "waited {waited} s");
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_ends_the_wait_at_once() {
        let (_limiter, cancel, paying) = in_debt(60);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let start = Instant::now();
        cancel.cancel();
        assert_eq!(paying.await.unwrap(), Paced::Cancelled);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn lifting_the_limit_ends_the_wait_and_raising_it_shortens_it() {
        let (limiter, _cancel, paying) = in_debt(60);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let start = Instant::now();
        limiter.set_limit(None);
        assert_eq!(paying.await.unwrap(), Paced::Done);
        assert_eq!(start.elapsed(), Duration::ZERO);

        // 59 s left at 1 kB/s is 5.9 s at 10 kB/s.
        let (limiter, _cancel, paying) = in_debt(60);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let start = Instant::now();
        limiter.set_limit(Some(10_000));
        assert_eq!(paying.await.unwrap(), Paced::Done);
        let waited = start.elapsed().as_secs_f64();
        assert!((5.8..=6.0).contains(&waited), "waited {waited} s");
    }
}
