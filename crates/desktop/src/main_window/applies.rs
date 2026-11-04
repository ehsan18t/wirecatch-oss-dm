//! Which actions apply to a download, decided once: the list's buttons, the selection bar and the
//! context menu all ask here, so no place offers what another refuses.

use gpui_kit::component::IconName;
use gpui_kit::{App, Window};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction, WaitReason};
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use crate::backend::act;
use crate::files;

const RETRY: &str = "Try again";
const START_OVER: &str = "Start over";

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

/// What a row's hover buttons do.
#[derive(Clone, Copy)]
pub enum RowAction {
    Pause,
    Resume,
    Retry,
    Open,
    ShowInFolder,
}

impl RowAction {
    /// The buttons a download in `status` offers on hover.
    pub fn offered(status: DownloadStatus) -> &'static [(Self, &'static str, IconName)] {
        match status {
            DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing => &[(Self::Pause, "Pause", IconName::Pause)],
            DownloadStatus::Paused => &[(Self::Resume, "Resume", IconName::Play)],
            DownloadStatus::Failed => &[(Self::Retry, RETRY, IconName::RotateCw)],
            DownloadStatus::Cancelled => &[(Self::Retry, START_OVER, IconName::RotateCw)],
            DownloadStatus::Completed => &[
                (Self::Open, "Open", IconName::ExternalLink),
                (Self::ShowInFolder, "Show in folder", IconName::FolderOpen),
            ],
        }
    }

    pub fn run(self, id: DownloadId, window: &mut Window, cx: &mut App) {
        match self {
            Self::Pause => act(window.window_handle(), cx, move |manager| {
                api::pause(manager, id)
            }),
            Self::Resume => act(window.window_handle(), cx, move |manager| {
                api::resume(manager, id)
            }),
            Self::Retry => act(window.window_handle(), cx, move |manager| {
                api::retry(manager, id)
            }),
            Self::Open => files::open(id, window, cx),
            Self::ShowInFolder => files::show_in_folder(id, window, cx),
        }
    }
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
    /// What retrying is called: starting over when all of it is cancelled.
    pub retry_word: &'static str,
}

impl Applicable {
    /// Retrying `named`, in the words of [`Self::retry_word`].
    pub fn retry_phrase(&self, named: &str) -> String {
        if self.retry_word == START_OVER {
            format!("Start {named} over")
        } else {
            format!("Try {named} again")
        }
    }

    pub fn to(rows: &[&DownloadSummary]) -> Self {
        Self {
            pause: rows.iter().any(|r| can_pause(r)),
            resume: rows.iter().any(|r| can_resume(r)),
            retry: rows.iter().any(|r| can_retry(r)),
            retry_word: if rows.iter().any(|r| r.status == DownloadStatus::Failed) {
                RETRY
            } else {
                START_OVER
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::downloads::download;

    #[test]
    fn a_row_offers_exactly_what_the_rules_allow() {
        for status in [
            DownloadStatus::Queued,
            DownloadStatus::Connecting,
            DownloadStatus::Downloading,
            DownloadStatus::Processing,
            DownloadStatus::Paused,
            DownloadStatus::Completed,
            DownloadStatus::Failed,
            DownloadStatus::Cancelled,
        ] {
            let row = DownloadSummary {
                status,
                ..download(1)
            };
            let offers = |wanted: fn(&RowAction) -> bool| {
                RowAction::offered(status)
                    .iter()
                    .any(|(action, ..)| wanted(action))
            };
            assert_eq!(
                offers(|a| matches!(a, RowAction::Pause)),
                can_pause(&row),
                "{status:?}"
            );
            assert_eq!(
                offers(|a| matches!(a, RowAction::Resume)),
                can_resume(&row),
                "{status:?}"
            );
            assert_eq!(
                offers(|a| matches!(a, RowAction::Retry)),
                can_retry(&row),
                "{status:?}"
            );
        }
    }

    #[test]
    fn retrying_a_named_download_uses_the_same_words() {
        let (failed, cancelled) = (
            DownloadSummary {
                status: DownloadStatus::Failed,
                ..download(1)
            },
            DownloadSummary {
                status: DownloadStatus::Cancelled,
                ..download(2)
            },
        );
        assert_eq!(
            Applicable::to(&[&failed]).retry_phrase("it"),
            "Try it again"
        );
        assert_eq!(
            Applicable::to(&[&cancelled]).retry_phrase("it"),
            "Start it over"
        );
    }

    #[test]
    fn every_place_calls_retrying_a_cancelled_download_the_same() {
        let row = |status| DownloadSummary {
            status,
            ..download(1)
        };
        for status in [DownloadStatus::Cancelled, DownloadStatus::Failed] {
            let (_, offered, _) = RowAction::offered(status)[0].clone();
            let only = row(status);
            assert_eq!(Applicable::to(&[&only]).retry_word, offered);
        }
        let (failed, cancelled) = (row(DownloadStatus::Failed), row(DownloadStatus::Cancelled));
        assert_eq!(Applicable::to(&[&failed, &cancelled]).retry_word, RETRY);
    }
}
