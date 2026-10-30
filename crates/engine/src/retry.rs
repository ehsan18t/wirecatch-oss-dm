//! The one retry policy: what follows a failed attempt, and how long to wait.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use reqwest::StatusCode;

use crate::error::Error;
use crate::ftp::reply;
use crate::http;
use crate::numeric::saturating_cast;

/// Consecutive refused attempts, while other connections of the same download
/// are receiving data, after which a connection gives up its part to them.
/// One refusal can be a fluke; two in a row mean the server allows no more
/// connections than are already open. A refusal that asks for a long wait
/// (`Retry-After`, or a delay beyond the normal backoff) steps back at once,
/// so the segment is not held hostage while the others could finish it.
const REFUSALS_BEFORE_STEPPING_BACK: u32 = 2;

/// The most random delay added to a pause, so connections that failed
/// together spread their next attempts out.
const MAX_JITTER: Duration = Duration::from_millis(250);

/// The pauses between failed attempts
/// ([`DownloadOptions::retry_backoff`](crate::DownloadOptions::retry_backoff)):
/// [`first`](Self::first) after the first failure, doubling with each failure
/// in a row up to [`max`](Self::max), plus a random fraction of a second so
/// connections that failed together do not come back together. A server's
/// `Retry-After` replaces the pause, whatever these are.
///
/// ```
/// # use std::time::Duration;
/// # use wirecatch_engine::{Backoff, DownloadOptions};
/// let mut options = DownloadOptions::default();
/// options.retry_backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Backoff {
    /// The pause after the first failure (default 500 ms). Must not be zero.
    pub first: Duration,
    /// The longest pause (default 10 s). Must not be shorter than
    /// [`first`](Self::first).
    pub max: Duration,
}

impl Backoff {
    /// Pauses from `first`, doubling up to `max`.
    #[must_use]
    pub const fn new(first: Duration, max: Duration) -> Self {
        Self { first, max }
    }

    /// Whether these pauses can work: [`first`](Self::first) is not zero and
    /// [`max`](Self::max) is not shorter.
    pub(crate) fn is_valid(self) -> bool {
        !self.first.is_zero() && self.first <= self.max
    }

    /// How long to wait before the next attempt: what the server asked for in
    /// `Retry-After` if it said, otherwise [`pause`](Self::pause). Jitter is
    /// only ever added, so a server's delay is never cut short.
    pub(crate) fn delay(self, failures: u32, retry_after: Option<Duration>) -> Duration {
        match retry_after {
            Some(delay) => delay + self.jitter(),
            None => self.pause(failures),
        }
    }

    /// [`first`](Self::first), doubled for each failure after the first,
    /// capped at [`max`](Self::max), plus [`jitter`](Self::jitter).
    pub(crate) fn pause(self, failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1);
        self.first
            .saturating_mul(2u32.saturating_pow(doublings))
            .min(self.max)
            + self.jitter()
    }

    /// A random delay below [`MAX_JITTER`] and half of [`first`](Self::first),
    /// different on every call: connections that fail at the same moment must
    /// not come back at the same moment.
    fn jitter(self) -> Duration {
        // Every `RandomState` is keyed apart from the last one, so a hash made with a
        // new one is a fresh random number, without a dependency just for this.
        let random = RandomState::new().build_hasher().finish();
        let span: u64 = saturating_cast(MAX_JITTER.min(self.first / 2).as_nanos());
        Duration::from_nanos(random.checked_rem(span).unwrap_or(0))
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(500), Duration::from_secs(10))
    }
}

/// What a connection does after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Try again after this long.
    Retry(Duration),
    /// The server keeps refusing this connection: hand the segment to the
    /// connections that are working if there are any, else retry after this
    /// long.
    StepBackOrRetry(Duration),
    /// Stop and report the error.
    GiveUp,
}

/// How a refusal counts: an attempt the server turned away (see
/// [`is_refusal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusals {
    /// Like any other failure. For requests that are the only ones of their
    /// kind, which have no other connection to hand their work to.
    AsFailures,
    /// As a sign the server allows no more connections: retried even when the
    /// error alone would not be, and after a few in a row (or one asking for a
    /// long wait) the connection may step back for the others.
    StepBack,
}

/// The one retry policy: a connection's or a request's failed attempts in a
/// row, and what follows the next one.
#[derive(Debug)]
pub(crate) struct Attempts {
    refusal_policy: Refusals,
    /// The most attempts in a row that receive nothing
    /// ([`DownloadOptions::max_retries`](crate::DownloadOptions::max_retries)).
    max_retries: u32,
    backoff: Backoff,
    /// Attempts in a row that received nothing.
    failures: u32,
    /// Of those, attempts the server turned away.
    refusals: u32,
}

impl Attempts {
    /// No failed attempt yet; at most `max_retries` in a row that receive
    /// nothing, with `backoff`'s pauses between them.
    pub fn new(refusal_policy: Refusals, max_retries: u32, backoff: Backoff) -> Self {
        Self {
            refusal_policy,
            max_retries,
            backoff,
            failures: 0,
            refusals: 0,
        }
    }

    /// Decides what follows a failed attempt. `progressed` says whether it
    /// received anything, which resets the count.
    ///
    /// With [`Refusals::StepBack`], a refusal is retried even when the error
    /// alone would not be: an FTP server turning away one more login (530)
    /// already accepted the same login on another connection.
    pub fn after_failure(
        &mut self,
        error: &Error,
        retry_after: Option<Duration>,
        progressed: bool,
    ) -> Next {
        let refused = self.refusal_policy == Refusals::StepBack && is_refusal(error);
        if !(error.is_retryable() || refused) {
            return Next::GiveUp;
        }
        // Only attempts that received nothing count against the budget
        // (`DownloadOptions::max_retries`).
        if progressed {
            self.failures = 0;
        } else if self.failures >= self.max_retries {
            return Next::GiveUp;
        } else {
            self.failures += 1;
        }
        self.refusals = if !progressed && refused {
            self.refusals + 1
        } else {
            0
        };
        let delay = self.backoff.delay(self.failures, retry_after);
        let long_wait = retry_after.is_some() || delay > self.backoff.max;
        if self.refusals > 0 && (self.refusals >= REFUSALS_BEFORE_STEPPING_BACK || long_wait) {
            Next::StepBackOrRetry(delay)
        } else {
            Next::Retry(delay)
        }
    }
}

/// Whether a failed attempt that received nothing looks like the server turning
/// this connection away: busy statuses, or a connection that was accepted and
/// then closed or reset. Timeouts are not refusals; a slow network is not a limit.
pub(crate) fn is_refusal(error: &Error) -> bool {
    match error {
        Error::HttpStatus { status, .. } => StatusCode::from_u16(*status).is_ok_and(http::is_busy),
        // No answer in time (`Stalled`) is not a refusal either.
        Error::Network(e) => !e.is_timeout(),
        Error::PrematureEof { .. } => true,
        // 421: too many connections. 530 on a later connection: this login
        // worked on the first one, so the server limits sessions. 425: the
        // server would not open another data connection.
        Error::FtpReply { code, .. } => matches!(
            *code,
            reply::SERVICE_UNAVAILABLE | reply::NO_DATA_CONNECTION | reply::NOT_LOGGED_IN
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let backoff = Backoff::default();
        assert!(backoff.pause(1) < backoff.first + MAX_JITTER);
        assert!(backoff.pause(3) >= Duration::from_secs(2));
        assert!(backoff.pause(30) < backoff.max + MAX_JITTER);
        assert!(backoff.pause(u32::MAX) < backoff.max + MAX_JITTER);
    }

    #[test]
    fn short_pauses_get_short_jitter() {
        let backoff = Backoff::new(Duration::from_millis(10), Duration::from_millis(40));
        assert!(backoff.pause(1) < Duration::from_millis(15));
        assert!(backoff.pause(10) < Duration::from_millis(45));
        let tiny = Backoff::new(Duration::from_nanos(1), Duration::from_nanos(1));
        assert_eq!(tiny.pause(1), Duration::from_nanos(1));
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
        let mut attempts = Attempts::new(Refusals::StepBack, 6, Backoff::default());
        let first = attempts.after_failure(error, None, false);
        let second = attempts.after_failure(error, None, false);
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
        let mut attempts = Attempts::new(Refusals::StepBack, 6, Backoff::default());
        assert_eq!(attempts.after_failure(&ftp(550), None, false), Next::GiveUp);
    }

    #[test]
    fn the_retry_budget_and_progress() {
        let mut attempts = Attempts::new(Refusals::StepBack, 0, Backoff::default());
        assert_eq!(attempts.after_failure(&ftp(421), None, false), Next::GiveUp);
        let mut attempts = Attempts::new(Refusals::StepBack, 1, Backoff::default());
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, false),
            Next::Retry(_)
        ));
        assert_eq!(attempts.after_failure(&ftp(450), None, false), Next::GiveUp);
        // An attempt that received something does not count, and starts the
        // count again.
        let mut attempts = Attempts::new(Refusals::StepBack, 1, Backoff::default());
        attempts.after_failure(&ftp(450), None, false);
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, true),
            Next::Retry(_)
        ));
        assert!(matches!(
            attempts.after_failure(&ftp(450), None, false),
            Next::Retry(_)
        ));
        // A refusal that asks for a long wait steps back at once.
        let mut attempts = Attempts::new(Refusals::StepBack, 6, Backoff::default());
        let busy = Error::HttpStatus {
            status: 503,
            url: "https://h/f".into(),
        };
        assert!(matches!(
            attempts.after_failure(&busy, Some(Duration::from_secs(5)), false),
            Next::StepBackOrRetry(_)
        ));
    }

    /// Connections that fail together ask for their delay within the same
    /// millisecond; they must still not all retry at the same moment.
    #[test]
    fn connections_failing_together_get_different_delays() {
        let backoff = Backoff::default();
        let delays: std::collections::HashSet<Duration> =
            (0..16).map(|_| backoff.pause(1)).collect();
        assert!(
            delays.len() >= 8,
            "16 delays asked for at once, only {} different",
            delays.len()
        );
        assert!(
            delays
                .iter()
                .all(|d| *d >= backoff.first && *d < backoff.first + MAX_JITTER)
        );
    }

    #[test]
    fn a_lone_request_treats_refusals_as_failures() {
        let mut attempts = Attempts::new(Refusals::AsFailures, 6, Backoff::default());
        assert_eq!(attempts.after_failure(&ftp(530), None, false), Next::GiveUp);
        let busy = Error::HttpStatus {
            status: 503,
            url: "https://h/f".into(),
        };
        for _ in 0..3 {
            assert!(matches!(
                attempts.after_failure(&busy, Some(Duration::from_secs(5)), false),
                Next::Retry(_)
            ));
        }
    }

    #[test]
    fn retry_after_is_never_shortened() {
        let asked = Duration::from_secs(3);
        let backoff = Backoff::default();
        let delay = backoff.delay(1, Some(asked));
        assert!(delay >= asked && delay < asked + MAX_JITTER);
        assert!(backoff.delay(1, None) < Duration::from_millis(800));
    }
}
