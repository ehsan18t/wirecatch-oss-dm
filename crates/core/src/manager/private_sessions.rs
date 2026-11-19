//! The browsers' private sessions (D279): which are open, and forgetting a session's downloads
//! when it ends, whichever way it ends.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::time::Duration;

use tracing::Instrument as _;

use wirecatch_protocol::extension::NewDownload;
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use crate::error::CoreError;

/// The default [`ManagerConfig::private_session_grace`](crate::ManagerConfig::private_session_grace):
/// long enough for a background the browser restarted, or an extension that was updated, to
/// connect again and name the session.
pub const PRIVATE_SESSION_GRACE: Duration = Duration::from_secs(60);

/// Ended sessions remembered, so a capture arriving late for one is refused.
const ENDED_KEPT: usize = 64;

/// `work`, run inside the span whose lines the log leaves out when `private` (D279): a private
/// download's addresses and file names must not outlive its session.
pub(super) fn private_scope<F: Future>(
    private: bool,
    work: F,
) -> tracing::instrument::Instrumented<F> {
    // An error span: enabled whatever the filter, so its lines are always recognised.
    let span = if private {
        tracing::error_span!("private_download")
    } else {
        tracing::Span::none()
    };
    work.instrument(span)
}

/// The private session a download with `origin` comes from, `None` for a normal window.
pub(super) fn session_of(origin: &crate::headers::Origin) -> Option<String> {
    origin
        .browser_source
        .as_ref()
        .and_then(|source| source.private_session.clone())
}

/// Whether a download with `origin` comes from a private window.
pub(super) fn is_private(origin: &crate::headers::Origin) -> bool {
    origin
        .browser_source
        .as_ref()
        .is_some_and(|source| source.private_session.is_some())
}

/// One connection's claim on a private session, released when the connection closes.
#[derive(Debug)]
pub struct SessionClaim {
    id: Option<String>,
}

#[derive(Default)]
pub(super) struct PrivateSessions {
    /// How many connections name each open session.
    claims: HashMap<String, usize>,
    /// Sessions no connection names, each with the grace it waits out.
    unclaimed: HashMap<String, u64>,
    next_grace: u64,
    ended: VecDeque<String>,
    ended_set: HashSet<String>,
}

impl PrivateSessions {
    fn end(&mut self, id: &str) {
        self.claims.remove(id);
        self.unclaimed.remove(id);
        if self.ended_set.insert(id.to_owned()) {
            self.ended.push_back(id.to_owned());
            if self.ended.len() > ENDED_KEPT
                && let Some(oldest) = self.ended.pop_front()
            {
                self.ended_set.remove(&oldest);
            }
        }
    }
}

impl Manager {
    /// A connection names private session `id`, in its hello or on a capture (D279). An ended
    /// session stays ended.
    #[must_use]
    pub fn claim_private_session(&self, id: Option<&str>) -> SessionClaim {
        let Some(id) = id else {
            return SessionClaim { id: None };
        };
        let mut sessions = self
            .inner
            .private
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.ended_set.contains(id) {
            return SessionClaim { id: None };
        }
        sessions.unclaimed.remove(id);
        *sessions.claims.entry(id.to_owned()).or_insert(0) += 1;
        SessionClaim {
            id: Some(id.to_owned()),
        }
    }

    /// The connection that held `claim` closed. A session no connection names any more ends
    /// once [`private_session_grace`](crate::ManagerConfig::private_session_grace) passes
    /// without one naming it again.
    pub fn release_private_session(&self, claim: SessionClaim) {
        let Some(id) = claim.id else { return };
        let grace = {
            let mut sessions = self
                .inner
                .private
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(count) = sessions.claims.get_mut(&id) else {
                return;
            };
            *count -= 1;
            if *count > 0 {
                return;
            }
            sessions.claims.remove(&id);
            sessions.next_grace += 1;
            let grace = sessions.next_grace;
            sessions.unclaimed.insert(id.clone(), grace);
            grace
        };
        let weak = std::sync::Arc::downgrade(&self.inner);
        let wait = self.inner.private_session_grace;
        self.inner.runtime.spawn(async move {
            tokio::time::sleep(wait).await;
            let Some(inner) = weak.upgrade() else { return };
            let manager = Manager { inner };
            let still_waiting = {
                let sessions = manager
                    .inner
                    .private
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                sessions.unclaimed.get(&id) == Some(&grace)
            };
            if still_waiting {
                manager.end_private_session(&id);
            }
        });
    }

    /// The browser's last private window of session `id` closed: its downloads are forgotten.
    pub fn end_private_session(&self, id: &str) {
        self.inner
            .private
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .end(id);
        self.forget_private_downloads(|session| session == id);
    }

    /// Whether session `id` has ended, so a capture for it is refused.
    pub(super) fn private_session_ended(&self, id: &str) -> bool {
        self.inner
            .private
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ended_set
            .contains(id)
    }

    /// Stops and removes every private download, waiting capture and media question whose
    /// session `ended` says has ended, with their partial files; finished files stay where they
    /// were saved.
    fn forget_private_downloads(&self, ended: impl Fn(&str) -> bool) {
        let ended = |session: Option<&str>| session.is_some_and(&ended);
        let (downloads, captures, media) = {
            let state = self.lock();
            let downloads: Vec<DownloadId> = state
                .downloads
                .values()
                .filter(|entry| ended(entry.private_session.as_deref()))
                .map(|entry| entry.row.id)
                .collect();
            let captures: Vec<QuestionId> = state
                .pending
                .iter()
                .filter(|(_, pending)| {
                    let route = pending.request.browser_route.as_deref();
                    ended(route.and_then(|route| route.private_session.as_deref()))
                })
                .map(|(id, _)| *id)
                .collect();
            let media: Vec<QuestionId> = state
                .media
                .iter()
                .filter(|(_, pending)| ended(session_of(&pending.source.fetch.origin).as_deref()))
                .map(|(id, _)| *id)
                .collect();
            (downloads, captures, media)
        };
        // A question the person answered meanwhile is gone already, so its error is no failure.
        for id in captures {
            if let Err(e) = self.reject_capture(id) {
                tracing::debug!(error = %e, "a private capture was answered as its session ended");
            }
        }
        for id in media {
            if let Err(e) = self.dismiss_media_choice(id) {
                tracing::debug!(error = %e, "private media was answered as its session ended");
            }
        }
        if !downloads.is_empty() {
            self.remove_in_background(downloads);
        }
    }

    /// Why a capture from a private window is refused: its session ended, or it names none (D279).
    pub(super) fn refused_private_capture(&self, capture: &NewDownload) -> Option<CoreError> {
        let route = capture.browser_route.as_deref()?;
        let refused = match route.private_session.as_deref() {
            Some(session) => self.private_session_ended(session),
            None => route.private_window,
        };
        refused.then(|| CoreError::NotStarted(wirecatch_messages::private_session_ended()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_private_span_is_the_one_the_log_leaves_out() {
        let subscriber = tracing_subscriber::registry();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::error_span!("private_download");
            let name = span.metadata().map(tracing::Metadata::name);
            assert_eq!(name, Some(wirecatch_logging::PRIVATE_SPAN));
        });
    }
}
