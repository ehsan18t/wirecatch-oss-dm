//! Forgetting the sign-in (cookies, credentials and tokens) of downloads that
//! have not finished within a week, so a browser session does not sit in the
//! database for good (D17). A download that then needs the sign-in fails with a
//! plain note asking for a fresh link.

use std::sync::{Arc, Weak};

use chrono::TimeDelta;
use tracing::info;
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, FixAction, UserError};

use super::state::{Entry, State};
use super::{Inner, Manager};
use crate::clock::Clock;
use crate::headers;
use crate::store::Origin;

/// How long an unfinished download keeps the browser's sign-in.
pub(super) const FORGET_AFTER_MILLIS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How often downloads are looked at.
const LOOK_EVERY: TimeDelta = TimeDelta::hours(1);

/// What a download whose sign-in was forgotten says when it needs a fresh link.
pub(super) const FORGOT_SIGN_IN: &str =
    "Wirecatch forgot the sign-in for this download after 7 days; get a fresh link.";

impl Manager {
    /// Deletes the cookies and credentials of every download that is waiting,
    /// paused, failed or cancelled and was added at least a week ago.
    pub(super) fn forget_old_sign_ins(&self, state: &mut State) {
        let now = self.inner.clock.unix_millis();
        let forgot: Vec<DownloadId> = state
            .downloads
            .iter_mut()
            .filter(|(_, entry)| !entry.is_running())
            .filter_map(|(id, entry)| forget_if_old(entry, now).then_some(*id))
            .collect();
        if !forgot.is_empty() {
            info!(
                downloads = forgot.len(),
                "forgot the sign-in of downloads older than a week"
            );
        }
        for id in forgot {
            self.save(state, id);
        }
    }
}

/// Deletes the cookies and credentials of `entry` if it has not finished and
/// was added at least a week before `now`; returns whether anything was
/// deleted. Also used right before a download starts, so a stale sign-in is
/// never sent.
pub(super) fn forget_if_old(entry: &mut Entry, now: i64) -> bool {
    let unfinished = matches!(
        entry.row.status,
        DownloadStatus::Queued
            | DownloadStatus::Paused
            | DownloadStatus::Failed
            | DownloadStatus::Cancelled
    );
    if !unfinished || now.saturating_sub(entry.row.added_at) < FORGET_AFTER_MILLIS {
        return false;
    }
    let Some(kept) = headers::without_sign_in(&entry.origin.headers) else {
        return false;
    };
    entry.origin.headers = kept;
    entry.origin.forgot_sign_in = true;
    // A failure that already asks for a fresh link now says why.
    if let Some(error) = entry.row.error.take() {
        entry.row.error = Some(explain(&entry.origin, error));
    }
    true
}

/// `error` for a download with `origin`: when its sign-in was forgotten and the
/// failure asks for a fresh link, it says why.
pub(super) fn explain(origin: &Origin, error: UserError) -> UserError {
    if origin.forgot_sign_in && error.fix == Some(FixAction::RefreshLink) {
        UserError {
            message: FORGOT_SIGN_IN.into(),
            fix: Some(FixAction::RefreshLink),
        }
    } else {
        error
    }
}

/// Forgets old sign-ins at startup and then every hour while the app runs.
pub(super) async fn run_sign_in_expiry(
    weak: Weak<Inner>,
    clock: Arc<dyn Clock>,
    stopped: CancellationToken,
) {
    loop {
        let Some(manager) = Manager::from_weak(&weak) else {
            return;
        };
        {
            let mut state = manager.lock();
            if state.closed {
                return;
            }
            manager.forget_old_sign_ins(&mut state);
        }
        drop(manager);
        let next = clock.local_now() + LOOK_EVERY;
        tokio::select! {
            () = clock.sleep_until(next) => {}
            () = stopped.cancelled() => return,
        }
    }
}
