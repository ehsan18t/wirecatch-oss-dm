//! Speed limiting shared across connections and downloads.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use crate::numeric::approx_f64;

/// Caps the combined speed of every connection that shares it.
///
/// Give one limiter to all downloads for a global limit, and another to the
/// downloads of one queue for a per-queue limit; a download honours every
/// limiter in [`DownloadOptions::rate_limiters`](crate::DownloadOptions).
/// The limit can change at any time and takes effect within one chunk.
#[derive(Debug)]
pub struct RateLimiter {
    bucket: Mutex<Bucket>,
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
        }
    }

    /// The current limit in bytes per second; `None` is unlimited.
    pub fn limit(&self) -> Option<u64> {
        self.bucket().limit
    }

    /// Changes the limit (bytes per second; `None` is unlimited). Every
    /// connection sharing this limiter follows the new limit within one chunk.
    pub fn set_limit(&self, limit: Option<u64>) {
        let mut bucket = self.bucket();
        bucket.limit = normalize(limit);
        bucket.tokens = bucket.tokens.min(0.0);
        bucket.last = Instant::now();
    }

    /// Locks the bucket. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives.
    fn bucket(&self) -> MutexGuard<'_, Bucket> {
        self.bucket.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Accounts for `bytes` just received, sleeping as long as needed to stay
    /// under the limit. Not reading from the socket meanwhile lets TCP flow
    /// control slow the sender down.
    pub(crate) async fn consume(&self, bytes: u64) {
        let wait = {
            let mut bucket = self.bucket();
            let Some(rate) = bucket.limit else { return };
            let rate = approx_f64(rate);
            let now = Instant::now();
            // A quarter second of burst smooths chunk-sized jitter without letting
            // an idle limiter release a large spike.
            let burst = (rate / 4.0).max(16.0 * 1024.0);
            let refill = now.duration_since(bucket.last).as_secs_f64() * rate;
            bucket.tokens = (bucket.tokens + refill).min(burst) - approx_f64(bytes);
            bucket.last = now;
            if bucket.tokens >= 0.0 {
                return;
            }
            Duration::from_secs_f64(-bucket.tokens / rate)
        };
        tokio::time::sleep(wait).await;
    }
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
        for _ in 0..100 {
            limiter.consume(10_000).await;
        }
        // 1 MB at 100 kB/s is 10 s, minus at most the burst allowance.
        let elapsed = start.elapsed().as_secs_f64();
        assert!((9.5..=10.1).contains(&elapsed), "took {elapsed} s");
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_never_waits_and_limit_can_change() {
        let limiter = RateLimiter::new(None);
        let start = Instant::now();
        limiter.consume(u64::MAX / 2).await;
        assert_eq!(start.elapsed(), Duration::ZERO);

        limiter.set_limit(Some(1_000));
        limiter.consume(2_000).await;
        assert!(start.elapsed() >= Duration::from_millis(1_900));
        assert_eq!(limiter.limit(), Some(1_000));
    }
}
