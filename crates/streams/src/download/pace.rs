//! How fast one host is asked for segments, across every stream download of the app. A host that refuses a segment after serving others (`403`, `410`, `429`) is limiting how much one address asks of it: every download from it then waits, asks for fewer segments at once, and tries the refused one again.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use std::pin::Pin;

use tokio::sync::Notify;
use tokio::sync::futures::OwnedNotified;
use tokio::time::Instant;

/// How many times one segment is asked for again after a refusal.
pub(super) const REFUSED_TRIES: u32 = 4;
/// The wait after a host's first refusal, doubled at each refusal after it, up to [`LONGEST_WAIT`].
const FIRST_WAIT: Duration = Duration::from_secs(2);
const LONGEST_WAIT: Duration = Duration::from_secs(30);
/// Segments served in a row that let a limited host take one more request at once.
const SERVED_TO_GROW: u32 = 8;

/// Every stream download's pace, by host and port.
pub(super) static HOSTS: LazyLock<Pace> = LazyLock::new(Pace::default);

#[derive(Default)]
pub(super) struct Pace {
    hosts: Mutex<HashMap<String, Host>>,
}

#[derive(Default)]
struct Host {
    asking: usize,
    /// How many requests it takes at once since it refused one; `None` before any refusal.
    limit: Option<usize>,
    resume_at: Option<Instant>,
    /// Refusals since it last served a segment.
    refusals: u32,
    served: u32,
    changed: Arc<Notify>,
}

/// One request to a host, counted until dropped.
pub(super) struct Turn<'a> {
    pace: &'a Pace,
    host: String,
}

impl Pace {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Host>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until `host` may be asked again: past its wait, and under its limit.
    pub(super) async fn turn(&self, host: &str) -> Turn<'_> {
        loop {
            let (resume_at, mut changed) = match self.try_turn(host) {
                Ok(turn) => return turn,
                Err(wait) => wait,
            };
            match resume_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => changed.as_mut().await,
            }
        }
    }

    /// A turn at `host` now, or when to try again: at the end of its wait, or once its count changes, listened for before the lock goes so no change is missed.
    fn try_turn(&self, host: &str) -> Result<Turn<'_>, (Option<Instant>, Pin<Box<OwnedNotified>>)> {
        let mut hosts = self.lock();
        let entry = hosts.entry(host.to_owned()).or_default();
        let now = Instant::now();
        let resume_at = entry.resume_at.filter(|at| *at > now);
        let full = entry.limit.is_some_and(|limit| entry.asking >= limit);
        if resume_at.is_none() && !full {
            entry.asking += 1;
            return Ok(Turn {
                pace: self,
                host: host.to_owned(),
            });
        }
        let mut changed = Box::pin(entry.changed.clone().notified_owned());
        changed.as_mut().enable();
        Err((resume_at, changed))
    }

    /// `host` refused a segment: it takes half as many requests at once as it was asked, and none until its wait is over.
    pub(super) fn refused(&self, host: &str) {
        let mut hosts = self.lock();
        let entry = hosts.entry(host.to_owned()).or_default();
        entry.limit = Some((entry.asking / 2).max(1));
        entry.refusals += 1;
        entry.served = 0;
        let doubling = 2_u32.saturating_pow(entry.refusals - 1);
        entry.resume_at =
            Some(Instant::now() + FIRST_WAIT.saturating_mul(doubling).min(LONGEST_WAIT));
    }

    /// `host` served a segment: after enough in a row, a limited host takes one more request at once.
    pub(super) fn served(&self, host: &str) {
        let mut hosts = self.lock();
        let Some(entry) = hosts.get_mut(host) else {
            return;
        };
        entry.refusals = 0;
        entry.served += 1;
        if entry.served >= SERVED_TO_GROW {
            entry.served = 0;
            if let Some(limit) = &mut entry.limit {
                *limit += 1;
                entry.changed.notify_waiters();
            }
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut hosts = self.pace.lock();
        let Some(entry) = hosts.get_mut(&self.host) else {
            return;
        };
        entry.asking -= 1;
        entry.changed.notify_waiters();
        // A host that was never limited keeps nothing once no one asks it.
        if entry.asking == 0 && entry.limit.is_none() {
            hosts.remove(&self.host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal halves what the host takes at once and rests it; it is asked again once the wait is over, and served segments let it take more.
    #[tokio::test(start_paused = true)]
    async fn a_refusing_host_rests_then_takes_fewer_then_more() {
        let pace = Pace::default();
        let turns: Vec<Turn<'_>> = turns(&pace, 4).await;
        pace.refused("cdn:443");
        drop(turns);
        let started = Instant::now();
        let first = pace.turn("cdn:443").await;
        assert_eq!(Instant::now() - started, FIRST_WAIT);
        let second = pace.turn("cdn:443").await;
        let third = pace.turn("cdn:443");
        tokio::pin!(third);
        assert!(
            !ready(third.as_mut()),
            "two at once after four were refused"
        );
        for _ in 0..SERVED_TO_GROW {
            pace.served("cdn:443");
        }
        assert!(ready(third.as_mut()));
        drop((first, second));
    }

    /// Refusals in a row wait twice as long each time, up to the longest wait; another host is not held.
    #[tokio::test(start_paused = true)]
    async fn waits_double_and_other_hosts_go_on() {
        let pace = Pace::default();
        let mut waited = Vec::new();
        for _ in 0..6 {
            pace.refused("cdn:443");
            let started = Instant::now();
            drop(pace.turn("cdn:443").await);
            waited.push(Instant::now() - started);
        }
        let seconds = |s| Duration::from_secs(s);
        assert_eq!(waited, [2, 4, 8, 16, 30, 30].map(seconds));
        let started = Instant::now();
        drop(pace.turn("other:443").await);
        assert_eq!(Instant::now(), started);
    }

    fn ready<F: Future>(future: std::pin::Pin<&mut F>) -> bool {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        future.poll(&mut context).is_ready()
    }

    async fn turns(pace: &Pace, count: usize) -> Vec<Turn<'_>> {
        let mut turns = Vec::new();
        for _ in 0..count {
            turns.push(pace.turn("cdn:443").await);
        }
        turns
    }
}
