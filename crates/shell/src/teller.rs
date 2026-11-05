//! When and how the person is told about downloads that started or ended (D52, D63, D65): each
//! queue's choice, downloads that end close together sharing one notification, and the settings.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AppSettings, DownloadSummary, NotifyOnFinish, NotifyOnStart, Queue, QueueNotify, QueueReport,
    UiEvent,
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

/// Downloads waiting to be told together, as they are now: one that stops being as it ended, or
/// leaves the list, leaves the batch, and a batch with nothing left is not due.
#[derive(Debug, Default)]
struct Batch {
    rows: Vec<DownloadSummary>,
    due: Option<Instant>,
}

impl Batch {
    fn add(&mut self, rows: Vec<DownloadSummary>, now: Instant) {
        if !rows.is_empty() {
            self.rows.extend(rows);
            self.due.get_or_insert(now + NOTIFY_BATCH);
        }
    }

    fn follow(&mut self, event: &UiEvent) {
        match event {
            UiEvent::DownloadsChanged(rows) => {
                self.rows
                    .retain_mut(|told| match rows.iter().find(|r| r.id == told.id) {
                        Some(row) => still_as_told(told, row),
                        None => true,
                    });
            }
            UiEvent::DownloadsRemoved(ids) => self.rows.retain(|told| !ids.contains(&told.id)),
            _ => return,
        }
        self.drop_due_if_empty();
    }

    fn reload(&mut self, list: &[DownloadSummary]) {
        self.rows
            .retain_mut(|told| match list.iter().find(|r| r.id == told.id) {
                Some(row) => still_as_told(told, row),
                None => false,
            });
        self.drop_due_if_empty();
    }

    fn drop_due_if_empty(&mut self) {
        if self.rows.is_empty() {
            self.due = None;
        }
    }

    fn take_due(&mut self, now: Instant) -> Option<Vec<DownloadSummary>> {
        if self.due.is_none_or(|due| due > now) {
            return None;
        }
        self.due = None;
        Some(std::mem::take(&mut self.rows))
    }
}

/// Whether `told` still ended the way it did, going by its row now, which it takes on.
fn still_as_told(told: &mut DownloadSummary, row: &DownloadSummary) -> bool {
    let same = told.status == row.status;
    if same {
        told.clone_from(row);
    }
    same
}

/// Decides when to tell what: each queue's choice, and downloads that end close together
/// sharing one notification. Holds no clock of its own, so tests give it the time.
#[derive(Debug, Default)]
pub struct Teller {
    modes: HashMap<QueueId, QueueNotify>,
    finished: Batch,
    failed: Batch,
}

impl Teller {
    /// Follows the queues' choices.
    pub fn queues(&mut self, queues: &[Queue]) {
        self.modes = queues.iter().map(|q| (q.id, q.settings.notify)).collect();
    }

    fn tells_each(&self, queue: QueueId) -> bool {
        self.modes.get(&queue).copied().unwrap_or_default() == QueueNotify::EachDownload
    }

    /// Takes in downloads that ended `now`. An event goes to [`Self::follow`] first, so a download
    /// that ends again is told once.
    pub fn ended(&mut self, ended: Ended, now: Instant) {
        let Ended { completed, failed } = ended;
        let told = completed
            .into_iter()
            .filter(|r| self.tells_each(r.queue))
            .collect();
        self.finished.add(told, now);
        self.failed.add(failed, now);
    }

    /// Corrects what waits to be told by what `event` did to those downloads.
    pub fn follow(&mut self, event: &UiEvent) {
        self.finished.follow(event);
        self.failed.follow(event);
    }

    /// Corrects what waits to be told by the whole list, after missing events.
    pub fn reload(&mut self, list: &[DownloadSummary]) {
        self.finished.reload(list);
        self.failed.reload(list);
    }

    /// A queue is done: told now when it tells once it is done.
    #[must_use]
    pub fn queue_finished(&self, report: &QueueReport) -> Option<Tell> {
        (!self.tells_each(report.queue)).then(|| Tell::Queue(report.clone()))
    }

    /// When the next notification is due, if any waits.
    #[must_use]
    pub fn due(&self) -> Option<Instant> {
        match (self.finished.due, self.failed.due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The notifications due by `now`.
    pub fn take_due(&mut self, now: Instant) -> Vec<Tell> {
        let finished = self.finished.take_due(now).map(Tell::Finished);
        let failed = self.failed.take_due(now).map(Tell::Failed);
        finished.into_iter().chain(failed).collect()
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

#[cfg(test)]
mod tests {
    use wirecatch_protocol::DownloadId;
    use wirecatch_protocol::app::DownloadStatus::{self, Completed, Failed, Queued};
    use wirecatch_protocol::app::{FinishNoticeTime, MediaSummary, QueueSettings, QueueStatus};

    use super::*;
    use crate::activity::Activity;
    use crate::notifications::tests::row;

    const WHEN_DONE: QueueId = QueueId(7);

    fn changed(id: i64, status: DownloadStatus) -> UiEvent {
        UiEvent::DownloadsChanged(vec![row(id, "downloads", status)])
    }

    /// A teller and the activity feeding it, each event going the way `watch` sends it.
    #[derive(Default)]
    struct Watch {
        activity: Activity,
        teller: Teller,
    }

    impl Watch {
        fn apply(&mut self, event: &UiEvent, now: Instant) {
            self.teller.follow(event);
            self.teller.ended(self.activity.apply(event), now);
        }

        fn take_when_due(&mut self) -> Vec<Tell> {
            let at = self.teller.due().expect("something is due");
            self.teller.take_due(at)
        }
    }

    fn told_finished(tells: &[Tell]) -> Vec<i64> {
        let [Tell::Finished(rows)] = tells else {
            panic!("expected one tell of what finished: {tells:?}");
        };
        rows.iter().map(|r| r.id.0).collect()
    }

    fn told_failed(tells: &[Tell]) -> Vec<i64> {
        let [Tell::Failed(rows)] = tells else {
            panic!("expected one tell of what failed: {tells:?}");
        };
        rows.iter().map(|r| r.id.0).collect()
    }

    #[test]
    fn downloads_ending_within_the_batch_share_one_tell_when_the_first_is_due() {
        let mut watch = Watch::default();
        let first = Instant::now();
        watch.apply(&changed(1, Completed), first);
        watch.apply(&changed(2, Completed), first + NOTIFY_BATCH / 2);
        let due = first + NOTIFY_BATCH;
        assert_eq!(watch.teller.due(), Some(due));
        assert_eq!(watch.teller.take_due(first), [], "not due yet");
        assert_eq!(told_finished(&watch.teller.take_due(due)), [1, 2]);
        assert_eq!(watch.teller.due(), None);
    }

    #[test]
    fn finished_and_failed_downloads_are_told_apart() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Completed), now);
        watch.apply(&changed(2, Failed), now);
        let tells = watch.take_when_due();
        assert_eq!(told_finished(&tells[..1]), [1]);
        assert_eq!(told_failed(&tells[1..]), [2]);
    }

    #[test]
    fn a_failure_tried_again_within_the_batch_is_not_told() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Failed), now);
        watch.apply(&changed(1, Queued), now);
        assert_eq!(watch.teller.due(), None);
    }

    #[test]
    fn a_failure_tried_again_that_fails_again_is_told_once() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Failed), now);
        watch.apply(&changed(1, Queued), now);
        watch.apply(&changed(1, Failed), now);
        assert_eq!(told_failed(&watch.take_when_due()), [1]);
    }

    #[test]
    fn a_failure_tried_again_that_finishes_is_told_as_finished() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Failed), now);
        watch.apply(&changed(1, Completed), now);
        assert_eq!(told_finished(&watch.take_when_due()), [1]);
    }

    #[test]
    fn a_download_removed_within_the_batch_is_not_told() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Failed), now);
        watch.apply(&changed(2, Completed), now);
        watch.apply(&changed(3, Completed), now);
        let removed = UiEvent::DownloadsRemoved(vec![DownloadId(1), DownloadId(2)]);
        watch.apply(&removed, now);
        assert_eq!(told_finished(&watch.take_when_due()), [3]);
    }

    #[test]
    fn what_waits_is_told_as_it_is_now() {
        let mut watch = Watch::default();
        let now = Instant::now();
        watch.apply(&changed(1, Completed), now);
        let mut renamed = row(1, "downloads", Completed);
        renamed.file_name = "renamed.zip".to_owned();
        watch.apply(&UiEvent::DownloadsChanged(vec![renamed.clone()]), now);
        assert_eq!(watch.take_when_due(), [Tell::Finished(vec![renamed])]);
    }

    #[test]
    fn the_whole_list_corrects_what_waits_after_missed_events() {
        let mut watch = Watch::default();
        let now = Instant::now();
        for (id, status) in [(1, Completed), (2, Failed), (3, Failed)] {
            watch.apply(&changed(id, status), now);
        }
        watch
            .teller
            .reload(&[row(1, "downloads", Completed), row(2, "downloads", Queued)]);
        assert_eq!(told_finished(&watch.take_when_due()), [1]);
        assert_eq!(watch.teller.due(), None);
    }

    fn teller_with_a_queue_told_when_done() -> Teller {
        let mut settings = QueueSettings::new("Night");
        settings.notify = QueueNotify::WhenDone;
        let mut teller = Teller::default();
        teller.queues(&[Queue {
            id: WHEN_DONE,
            is_default: false,
            settings,
            status: QueueStatus::default(),
        }]);
        teller
    }

    #[test]
    fn a_queue_told_when_done_tells_no_finished_download_but_every_failure() {
        let mut teller = teller_with_a_queue_told_when_done();
        let in_queue = |status| DownloadSummary {
            queue: WHEN_DONE,
            ..row(1, "downloads", status)
        };
        let completed = Ended {
            completed: vec![in_queue(Completed)],
            failed: Vec::new(),
        };
        teller.ended(completed, Instant::now());
        assert_eq!(teller.due(), None);
        let failed = Ended {
            completed: Vec::new(),
            failed: vec![in_queue(Failed)],
        };
        teller.ended(failed, Instant::now());
        assert!(teller.due().is_some());
    }

    #[test]
    fn a_queue_report_is_told_only_by_a_queue_told_when_done() {
        let teller = teller_with_a_queue_told_when_done();
        let report = |queue| QueueReport {
            queue,
            name: "Night".to_owned(),
            completed: vec![row(1, "downloads", Completed)],
            failed: Vec::new(),
        };
        assert!(teller.queue_finished(&report(WHEN_DONE)).is_some());
        assert!(teller.queue_finished(&report(QueueId(8))).is_none());
    }

    fn told_under(notify: NotifyOnFinish, tell: &Tell) -> Telling {
        let settings = AppSettings {
            notify_on_finish: notify,
            finish_notice_time: FinishNoticeTime::EightSeconds,
            ..wirecatch_settings::default_settings()
        };
        tell.telling(&settings)
    }

    #[test]
    fn each_setting_tells_in_its_own_way() {
        let tell = Tell::Finished(vec![row(1, "downloads", Completed)]);
        assert_eq!(told_under(NotifyOnFinish::Nothing, &tell), Telling::Nothing);
        assert_eq!(told_under(NotifyOnFinish::Window, &tell), Telling::Window);
        assert!(matches!(
            told_under(NotifyOnFinish::Notification, &tell),
            Telling::System(_)
        ));
        assert!(matches!(
            told_under(NotifyOnFinish::Browser, &tell),
            Telling::Browser {
                kind: NoticeKind::Finished,
                stays: Some(stays),
                ..
            } if stays == Duration::from_secs(8)
        ));
    }

    #[test]
    fn a_failure_notice_in_the_browser_stays_until_it_is_closed() {
        let failed = row(1, "downloads", Failed);
        let queue_with_failure = Tell::Queue(QueueReport {
            queue: WHEN_DONE,
            name: "Night".to_owned(),
            completed: Vec::new(),
            failed: vec![failed.clone()],
        });
        for tell in [Tell::Failed(vec![failed]), queue_with_failure] {
            assert!(matches!(
                told_under(NotifyOnFinish::Browser, &tell),
                Telling::Browser {
                    kind: NoticeKind::Failed,
                    stays: None,
                    ..
                }
            ));
        }
    }

    #[test]
    fn a_start_is_told_only_while_the_settings_ask_for_it() {
        let made = |_| crate::notifications::plain("Started".into(), String::new());
        let mut settings = wirecatch_settings::default_settings();
        settings.notify_on_start = NotifyOnStart::Nothing;
        assert!(start_notice(&settings, made).is_none());
        settings.notify_on_start = NotifyOnStart::Browser;
        let (_, stays) = start_notice(&settings, made).unwrap();
        assert_eq!(
            stays,
            Duration::from_millis(settings.start_notice_time.millis())
        );
    }

    #[test]
    fn a_video_read_for_its_qualities_is_not_told_as_started_again() {
        let with_source = |source| DownloadSummary {
            media: Some(MediaSummary {
                quality: "1080p".to_owned(),
                source,
                video_key: None,
                instead_of: None,
            }),
            ..row(1, "downloads", Queued)
        };
        assert!(told_as_started(&row(1, "downloads", Queued)));
        assert!(told_as_started(&with_source(MediaKind::Direct)));
        assert!(!told_as_started(&with_source(MediaKind::Page)));
    }
}
