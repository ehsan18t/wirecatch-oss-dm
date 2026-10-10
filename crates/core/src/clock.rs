//! Wall-clock time, injected so schedules can be tested without waiting.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{Local, NaiveDateTime, TimeDelta, Utc};
use tokio::sync::watch;

/// A future returned by [`Clock::sleep_until`].
pub type Sleep<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// The longest the system clock sleeps in one go. The wall clock can jump (the
/// machine sleeps, the user changes the time, daylight saving starts), so waits are
/// cut into pieces short enough to notice.
const MAX_SYSTEM_SLEEP: Duration = Duration::from_secs(60);

/// Where the manager reads the time of day from.
///
/// Schedules use local wall-clock time; timestamps shown to the user use Unix time.
/// Production code uses [`SystemClock`]; tests use [`ManualClock`] and move time by
/// hand, so a schedule spanning hours is tested in milliseconds.
pub trait Clock: Send + Sync + 'static {
    /// The current local date and time, as shown on the user's own clock.
    fn local_now(&self) -> NaiveDateTime;

    /// Milliseconds since the Unix epoch.
    fn unix_millis(&self) -> i64;

    /// Waits until this clock shows `deadline` or later. May return early, so
    /// callers read the time again after waking.
    fn sleep_until(&self, deadline: NaiveDateTime) -> Sleep<'_>;
}

/// The real clock of the machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn local_now(&self) -> NaiveDateTime {
        Local::now().naive_local()
    }

    fn unix_millis(&self) -> i64 {
        Utc::now().timestamp_millis()
    }

    fn sleep_until(&self, deadline: NaiveDateTime) -> Sleep<'_> {
        let wait = (deadline - self.local_now())
            .to_std()
            .unwrap_or(Duration::ZERO)
            .min(MAX_SYSTEM_SLEEP);
        Box::pin(tokio::time::sleep(wait))
    }
}

/// A clock that only moves when told to. Meant for tests and simulations.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<NaiveDateTime>,
    moved: watch::Sender<()>,
}

impl ManualClock {
    /// A clock showing `now` as local time.
    pub fn new(now: NaiveDateTime) -> Self {
        Self {
            now: Mutex::new(now),
            moved: watch::Sender::new(()),
        }
    }

    /// Sets the clock to `now` and wakes everything sleeping on it.
    pub fn set(&self, now: NaiveDateTime) {
        *self.now.lock().unwrap_or_else(|e| e.into_inner()) = now;
        self.moved.send_replace(());
    }

    /// Moves the clock forward by `by` and wakes everything sleeping on it.
    pub fn advance(&self, by: TimeDelta) {
        self.set(self.local_now() + by);
    }
}

impl Clock for ManualClock {
    fn local_now(&self) -> NaiveDateTime {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn unix_millis(&self) -> i64 {
        self.local_now().and_utc().timestamp_millis()
    }

    fn sleep_until(&self, deadline: NaiveDateTime) -> Sleep<'_> {
        // Subscribing before the first check means a move between the two is not missed.
        let mut moved = self.moved.subscribe();
        Box::pin(async move {
            while self.local_now() < deadline {
                if moved.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 12)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .unwrap()
    }

    #[tokio::test]
    async fn manual_clock_sleeps_until_moved_past_the_deadline() {
        let clock = ManualClock::new(at(1, 0));
        let mut sleep = clock.sleep_until(at(1, 30));
        let short = Duration::from_millis(20);
        assert!(tokio::time::timeout(short, &mut sleep).await.is_err());

        clock.advance(TimeDelta::minutes(10));
        assert!(tokio::time::timeout(short, &mut sleep).await.is_err());

        clock.advance(TimeDelta::minutes(20));
        tokio::time::timeout(Duration::from_secs(5), sleep)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn manual_clock_past_deadline_returns_at_once() {
        let clock = ManualClock::new(at(2, 0));
        tokio::time::timeout(Duration::from_secs(5), clock.sleep_until(at(1, 0)))
            .await
            .unwrap();
    }
}
