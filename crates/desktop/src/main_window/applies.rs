//! Which actions apply to a download, decided once: the list's buttons, the selection bar and the
//! context menu all ask here, so no place offers what another refuses.

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction, WaitReason};

/// Running, or waiting in its queue to run.
pub fn can_pause(row: &DownloadSummary) -> bool {
    matches!(
        row.status,
        DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing
    )
}

pub fn can_resume(row: &DownloadSummary) -> bool {
    row.status == DownloadStatus::Paused
}

/// A failed download tries again from where it stopped; a cancelled one starts over.
pub fn can_retry(row: &DownloadSummary) -> bool {
    matches!(
        row.status,
        DownloadStatus::Failed | DownloadStatus::Cancelled
    )
}

/// Cancelling discards the partial file, so it is for a download that still has one.
pub fn can_cancel(row: &DownloadSummary) -> bool {
    !matches!(
        row.status,
        DownloadStatus::Completed | DownloadStatus::Cancelled
    )
}

/// A fresh link stands in for an expired one while bytes are still to come from it.
pub fn can_refresh_link(row: &DownloadSummary) -> bool {
    can_cancel(row)
        && !matches!(
            row.status,
            DownloadStatus::Failed | DownloadStatus::Processing
        )
}

/// A new link is offered for what still downloads from its link, and for a failure whose fix is one.
pub fn offers_new_link(row: &DownloadSummary) -> bool {
    can_refresh_link(row)
        || row
            .error
            .as_ref()
            .is_some_and(|error| error.fix == Some(FixAction::RefreshLink))
}

/// The link may be why a download is stuck: it is paused, as a link often expires while it waits,
/// or the server turned it away.
pub fn link_may_have_expired(row: &DownloadSummary) -> bool {
    can_refresh_link(row)
        && (row.status == DownloadStatus::Paused
            || matches!(row.waiting, Some(WaitReason::ServerRefused { .. })))
}

/// Moving to `queue` is offered while some of the downloads are in another queue.
pub fn can_move_to(rows: &[&DownloadSummary], queue: QueueId) -> bool {
    rows.iter().any(|row| row.queue != queue)
}

/// What a selection offers: an action applies when at least one of its downloads takes it.
pub struct Applicable {
    pub pause: bool,
    pub resume: bool,
    pub retry: bool,
}

impl Applicable {
    pub fn to(rows: &[&DownloadSummary]) -> Self {
        Self {
            pause: rows.iter().any(|r| can_pause(r)),
            resume: rows.iter().any(|r| can_resume(r)),
            retry: rows.iter().any(|r| can_retry(r)),
        }
    }
}
