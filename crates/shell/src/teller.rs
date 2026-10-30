//! When and how the person is told about downloads that started or ended (D52, D63, D65): each
//! queue's choice, downloads that end close together sharing one notification, and the settings.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AppSettings, DownloadSummary, NotifyOnFinish, NotifyOnStart, Queue, QueueNotify, QueueReport,
};
use wirecatch_protocol::extension::{MediaKind, NoticeKind};

use crate::activity::Ended;
use crate::notifications::{self, Notification};

/// Downloads finishing (or failing) within this long of the first one share one notification.
pub const NOTIFY_BATCH: Duration = Duration::from_secs(3);

/// What to tell the person about downloads that ended (D52).
#[derive(Debug, Clone, PartialEq)]
pub enum Tell {
    /// Downloads that finished close together, in queues that tell each download.
    Finished(Vec<DownloadSummary>),
    /// Downloads that failed close together, whatever their queue: a failure is told at once.
    Failed(Vec<DownloadSummary>),
    /// A queue that tells once it is done is done.
    Queue(QueueReport),
}

/// Decides when to tell what: each queue's choice, and downloads that end close together
/// sharing one notification. Holds no clock of its own, so tests give it the time.
#[derive(Debug, Default)]
pub struct Teller {
    modes: HashMap<QueueId, QueueNotify>,
    finished: Vec<DownloadSummary>,
    finished_due: Option<Instant>,
    failed: Vec<DownloadSummary>,
    failed_due: Option<Instant>,
}

impl Teller {
    /// Follows the queues' choices.
    pub fn queues(&mut self, queues: &[Queue]) {
        self.modes = queues.iter().map(|q| (q.id, q.settings.notify)).collect();
    }

    fn tells_each(&self, queue: QueueId) -> bool {
        self.modes.get(&queue).copied().unwrap_or_default() == QueueNotify::EachDownload
    }

    /// Takes in downloads that ended `now`.
    pub fn ended(&mut self, ended: Ended, now: Instant) {
        let Ended { completed, failed } = ended;
        let told: Vec<DownloadSummary> = completed
            .into_iter()
            .filter(|r| self.tells_each(r.queue))
            .collect();
        if !told.is_empty() {
            self.finished.extend(told);
            self.finished_due.get_or_insert(now + NOTIFY_BATCH);
        }
        if !failed.is_empty() {
            self.failed.extend(failed);
            self.failed_due.get_or_insert(now + NOTIFY_BATCH);
        }
    }

    /// A queue is done: told now when it tells once it is done.
    #[must_use]
    pub fn queue_finished(&self, report: &QueueReport) -> Option<Tell> {
        (!self.tells_each(report.queue)).then(|| Tell::Queue(report.clone()))
    }

    /// When the next notification is due, if any waits.
    #[must_use]
    pub fn due(&self) -> Option<Instant> {
        match (self.finished_due, self.failed_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The notifications due by `now`.
    pub fn take_due(&mut self, now: Instant) -> Vec<Tell> {
        let mut tells = Vec::new();
        if self.finished_due.is_some_and(|due| due <= now) {
            self.finished_due = None;
            tells.push(Tell::Finished(std::mem::take(&mut self.finished)));
        }
        if self.failed_due.is_some_and(|due| due <= now) {
            self.failed_due = None;
            tells.push(Tell::Failed(std::mem::take(&mut self.failed)));
        }
        tells
    }
}

/// How something that ended reaches the person, the way the settings say (D52, D65).
#[derive(Debug, Clone, PartialEq)]
pub enum Telling {
    Nothing,
    System(Notification),
    /// A notice in the browser in front, of `kind`, which stays `stays` (`None` until it is closed).
    Browser {
        notification: Notification,
        kind: NoticeKind,
        stays: Option<Duration>,
    },
    /// The download complete window lists it.
    Window,
}

impl Tell {
    /// The notification that tells it; `None` when nothing in it is told.
    #[must_use]
    pub fn notification(&self) -> Option<Notification> {
        match self {
            Self::Finished(rows) => notifications::finished(&rows.iter().collect::<Vec<_>>()),
            Self::Failed(rows) => notifications::failed(&rows.iter().collect::<Vec<_>>()),
            Self::Queue(report) => notifications::queue_done(report),
        }
    }

    /// How it is told under `settings`.
    #[must_use]
    pub fn telling(&self, settings: &AppSettings) -> Telling {
        let told =
            |made: fn(Notification) -> Telling| self.notification().map_or(Telling::Nothing, made);
        match settings.notify_on_finish {
            NotifyOnFinish::Nothing => Telling::Nothing,
            NotifyOnFinish::Notification => told(Telling::System),
            NotifyOnFinish::Window => Telling::Window,
            NotifyOnFinish::Browser => {
                let failed = match self {
                    Self::Finished(_) => false,
                    Self::Failed(_) => true,
                    Self::Queue(report) => !report.failed.is_empty(),
                };
                // A failure needs the person: its notice stays until it is closed.
                let (kind, stays) = if failed {
                    (NoticeKind::Failed, None)
                } else {
                    let stays = settings
                        .finish_notice_time
                        .millis()
                        .map(Duration::from_millis);
                    (NoticeKind::Finished, stays)
                };
                self.notification()
                    .map_or(Telling::Nothing, |notification| Telling::Browser {
                        notification,
                        kind,
                        stays,
                    })
            }
        }
    }
}

/// The start notice `made` gives, with how long it stays, while `settings` tell what started
/// (D63, D65).
pub fn start_notice(
    settings: &AppSettings,
    made: impl FnOnce(Duration) -> Notification,
) -> Option<(Notification, Duration)> {
    if settings.notify_on_start == NotifyOnStart::Nothing {
        return None;
    }
    let stays = Duration::from_millis(settings.start_notice_time.millis());
    Some((made(stays), stays))
}

/// Whether a browser download that started without a window is told as started: a video read for
/// its qualities was told when it was handed over (D67), and one added in the app is not the
/// browser's.
#[must_use]
pub fn told_as_started(row: &DownloadSummary) -> bool {
    !row.media
        .as_ref()
        .is_some_and(|m| m.source != MediaKind::Direct)
}
