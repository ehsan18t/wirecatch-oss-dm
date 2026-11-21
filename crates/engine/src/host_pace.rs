//! Admission and recovery for native HTTP requests to one server.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::Retry;

use tokio::sync::Notify;
use tokio::time::Instant;
use url::Url;

const INITIAL_REST: Duration = Duration::from_secs(2);
const MAX_AUTO_REST: Duration = Duration::from_secs(30);
const SUCCESSES_TO_GROW: u32 = 8;

/// Shares server limits across native HTTP downloads. Create one for the transfers that share a network connection.
#[derive(Debug, Default)]
pub struct HostPace {
    hosts: Mutex<HashMap<Host, Load>>,
}

/// The native HTTP activity of one download. Give the same progress to each of its media tracks.
#[derive(Debug, Default)]
pub struct HostProgress {
    report_failures: bool,
    used: AtomicBool,
    active: AtomicUsize,
    delivering: AtomicUsize,
    waits: Mutex<Waits>,
}

#[derive(Debug, Default)]
struct Waits {
    next: u64,
    pending: HashMap<u64, (Option<Instant>, Option<Retry>)>,
}

/// A download waiting for admission, or retrying a request after a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostWait {
    /// Time left until the next attempt. Zero means waiting for capacity or beginning a retry.
    pub remaining: Duration,
    /// Whose own request is being tried again, with its retry budget.
    pub retry: Option<Retry>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Refusal {
    pub until: Instant,
    pub ambiguous: bool,
}

impl HostProgress {
    /// Tracks native HTTP waits and attaches request observations to terminal errors.
    #[must_use]
    pub fn with_failure_details() -> Self {
        Self {
            report_failures: true,
            ..Self::default()
        }
    }

    pub(crate) fn reports_failures(&self) -> bool {
        self.report_failures
    }

    /// Number of requests in flight, when this download uses native HTTP pacing.
    #[must_use]
    pub fn connections(&self) -> Option<usize> {
        self.used
            .load(Ordering::Relaxed)
            .then(|| self.active.load(Ordering::Relaxed))
    }

    /// The wait to display while there is no response body being delivered.
    #[must_use]
    pub fn waiting(&self) -> Option<HostWait> {
        if self.delivering.load(Ordering::Relaxed) != 0 {
            return None;
        }
        let waits = lock(&self.waits);
        let now = Instant::now();
        let (until, retry) = waits
            .pending
            .values()
            .copied()
            .max_by_key(|(until, retry)| {
                let counting_down = until.is_some_and(|at| at > now);
                (counting_down && retry.is_some(), retry.is_none(), *until)
            })?;
        Some(HostWait {
            remaining: until.map_or(Duration::ZERO, |at| at.saturating_duration_since(now)),
            retry,
        })
    }

    pub(crate) fn record(
        self: &Arc<Self>,
        until: Option<Instant>,
        retry: Option<Retry>,
    ) -> WaitGuard {
        let mut waits = lock(&self.waits);
        let id = waits.next;
        waits.next += 1;
        waits.pending.insert(id, (until, retry));
        WaitGuard {
            progress: self.clone(),
            id,
        }
    }
}

#[derive(Debug)]
pub(crate) struct WaitGuard {
    progress: Arc<HostProgress>,
    id: u64,
}

impl WaitGuard {
    fn until(&self, at: Option<Instant>) {
        if let Some((until, _)) = lock(&self.progress.waits).pending.get_mut(&self.id) {
            *until = at;
        }
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        lock(&self.progress.waits).pending.remove(&self.id);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Host {
    name: String,
    port: u16,
}

impl Host {
    fn of(url: &Url) -> Option<Self> {
        Some(Self {
            name: url.host_str()?.to_owned(),
            port: url.port_or_known_default()?,
        })
    }
}

#[derive(Debug, Default)]
struct Load {
    active: usize,
    limit: Option<usize>,
    until: Option<Instant>,
    refusals: u32,
    successes: u32,
    changed: Arc<Notify>,
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One download's observations, independent of the other transfers sharing its pace.
#[derive(Debug)]
pub(crate) struct Scope {
    pace: Arc<HostPace>,
    received: Mutex<HashSet<Host>>,
    progress: Arc<HostProgress>,
}

impl Scope {
    pub fn new(pace: Arc<HostPace>, progress: Option<Arc<HostProgress>>) -> Arc<Self> {
        let progress = progress.unwrap_or_default();
        progress.used.store(true, Ordering::Relaxed);
        Arc::new(Self {
            pace,
            received: Mutex::default(),
            progress,
        })
    }

    pub async fn turn(self: &Arc<Self>, url: &Url) -> Option<Turn> {
        let host = Host::of(url)?;
        let mut wait = None::<WaitGuard>;
        loop {
            let (until, changed) = {
                let mut hosts = lock(&self.pace.hosts);
                let load = hosts.entry(host.clone()).or_default();
                let until = load.until.filter(|at| *at > Instant::now());
                if until.is_none() && load.limit.is_none_or(|limit| load.active < limit) {
                    load.active += 1;
                    self.progress.active.fetch_add(1, Ordering::Relaxed);
                    return Some(Turn {
                        host,
                        scope: self.clone(),
                        received: false,
                    });
                }
                let mut changed = Box::pin(load.changed.clone().notified_owned());
                changed.as_mut().enable();
                (until, changed)
            };
            if let Some(wait) = &wait {
                wait.until(until);
            } else {
                wait = Some(self.progress.record(until, None));
            }
            if let Some(until) = until {
                tokio::select! {
                    () = changed => {}
                    () = tokio::time::sleep_until(until) => {}
                }
            } else {
                changed.await;
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct Turn {
    host: Host,
    scope: Arc<Scope>,
    received: bool,
}

impl Turn {
    pub fn refused(
        &self,
        status: u16,
        retry_after: Option<Duration>,
        ambiguous: bool,
    ) -> (bool, Option<Refusal>) {
        let busy = matches!(status, 429 | 503);
        let had_data = lock(&self.scope.received).contains(&self.host);
        let later_refusal = ambiguous && matches!(status, 403 | 410) && had_data;
        if !busy && !later_refusal {
            return (had_data, None);
        }
        let mut hosts = lock(&self.scope.pace.hosts);
        let Some(load) = hosts.get_mut(&self.host) else {
            return (had_data, None);
        };
        let factor = 2_u32.saturating_pow(load.refusals);
        load.refusals = load.refusals.saturating_add(1);
        load.successes = 0;
        let limit = (load.active / 2).max(1);
        load.limit = Some(load.limit.map_or(limit, |old| old.min(limit)));
        let rest = INITIAL_REST
            .saturating_mul(factor)
            .min(MAX_AUTO_REST)
            .max(retry_after.unwrap_or_default());
        let now = Instant::now();
        let until = load.until.unwrap_or(now).max(now + rest);
        load.until = Some(until);
        load.changed.notify_waiters();
        tracing::info!(
            host = self.host.name, port = self.host.port, status,
            rest = ?until.duration_since(now), limit = load.limit,
            "waiting before sending more requests to the server"
        );
        (
            had_data,
            Some(Refusal {
                until,
                ambiguous: later_refusal,
            }),
        )
    }

    pub fn data(&mut self, media: bool) {
        if !self.received {
            self.received = true;
            self.scope
                .progress
                .delivering
                .fetch_add(1, Ordering::Relaxed);
            if media {
                lock(&self.scope.received).insert(self.host.clone());
            }
        }
    }

    pub fn served(&self) {
        let mut hosts = lock(&self.scope.pace.hosts);
        if let Some(load) = hosts.get_mut(&self.host) {
            load.refusals = 0;
            if let Some(limit) = load.limit.as_mut() {
                load.successes += 1;
                if load.successes >= SUCCESSES_TO_GROW {
                    load.successes = 0;
                    *limit += 1;
                    load.changed.notify_waiters();
                }
            }
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.scope.progress.active.fetch_sub(1, Ordering::Relaxed);
        if self.received {
            self.scope
                .progress
                .delivering
                .fetch_sub(1, Ordering::Relaxed);
        }
        let mut hosts = lock(&self.scope.pace.hosts);
        if let Some(load) = hosts.get_mut(&self.host) {
            load.active -= 1;
            load.changed.notify_waiters();
            if load.active == 0 && load.limit.is_none() {
                hosts.remove(&self.host);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_peer_wait_does_not_turn_its_first_gone_response_into_throttling() {
        let pace = Arc::new(HostPace::default());
        let url = Url::parse("https://cdn.example/video").unwrap();
        let first = Scope::new(pace.clone(), None);
        let peer = Scope::new(pace, None);
        let mut turn = first.turn(&url).await.unwrap();
        turn.data(true);
        assert!(
            turn.refused(410, Some(Duration::from_secs(12)), true)
                .1
                .is_some()
        );
        assert!(turn.refused(403, None, true).1.is_some());
        drop(turn);
        let before = Instant::now();
        let turn = peer.turn(&url).await.unwrap();
        assert_eq!(before.elapsed(), Duration::from_secs(12));
        let (had_data, rest) = turn.refused(410, None, true);
        assert!(!had_data);
        assert!(rest.is_none());
    }
    #[tokio::test(start_paused = true)]
    async fn the_countdown_and_retry_describe_the_same_request() {
        let progress = Arc::new(HostProgress::default());
        let retry = Retry { attempt: 2, of: 6 };
        let now = Instant::now();
        let own = progress.record(Some(now + INITIAL_REST), Some(retry));
        let peer = progress.record(Some(now + MAX_AUTO_REST), None);
        assert_eq!(
            progress.waiting(),
            Some(HostWait {
                remaining: INITIAL_REST,
                retry: Some(retry),
            })
        );
        tokio::time::advance(INITIAL_REST).await;
        peer.until(None);
        assert_eq!(
            progress.waiting(),
            Some(HostWait {
                remaining: Duration::ZERO,
                retry: None,
            })
        );
        drop(peer);
        assert_eq!(progress.waiting().unwrap().retry, Some(retry));
        drop(own);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_waiter_releases_its_progress_before_the_rest_ends() {
        use std::future::Future as _;
        use std::task::{Context, Waker};
        let shared = Arc::new(HostPace::default());
        let trigger = Scope::new(shared.clone(), None);
        let url = Url::parse("https://cdn.example/file").unwrap();
        let turn = trigger.turn(&url).await.unwrap();
        assert!(turn.refused(429, None, false).1.is_some());
        drop(turn);

        let progress = Arc::new(HostProgress::default());
        let peer = Scope::new(shared, Some(progress.clone()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let wait = cancel.run_until_cancelled(peer.turn(&url));
        tokio::pin!(wait);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(progress.connections(), Some(0));
        assert_eq!(
            progress.waiting(),
            Some(HostWait {
                remaining: INITIAL_REST,
                retry: None
            })
        );
        cancel.cancel();
        assert!(wait.await.is_none());
        assert!(progress.waiting().is_none());
    }
    #[tokio::test(start_paused = true)]
    async fn in_flight_requests_drain_before_the_host_admits_more() {
        use std::future::Future as _;
        use std::task::{Context, Waker};
        let url = Url::parse("https://cdn.example/range").unwrap();
        let owner = Scope::new(Arc::default(), None);
        let mut first = owner.turn(&url).await.unwrap();
        first.data(true);
        let second = owner.turn(&url).await.unwrap();
        let third = owner.turn(&url).await.unwrap();
        assert!(third.refused(403, None, true).1.is_some());
        drop(third);
        let next = owner.turn(&url);
        tokio::pin!(next);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        tokio::time::advance(INITIAL_REST).await;
        assert!(next.as_mut().poll(&mut cx).is_pending());
        drop(second);
        assert!(next.as_mut().poll(&mut cx).is_pending());
        drop(first);
        let next = next.await.unwrap();
        assert_eq!(owner.progress.connections(), Some(1));
        drop(next);
    }
}
