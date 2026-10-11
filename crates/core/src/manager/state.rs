//! The manager's in-memory state. Everything here is guarded by one mutex that is
//! never held across an `.await`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::NaiveDateTime;
use tokio::task::AbortHandle;
use wirecatch_engine::{CancellationToken, Progress, RateLimiter};
use wirecatch_protocol::app::{
    AppSettings, DownloadStatus, DownloadSummary, PendingCapture, Queue,
};
use wirecatch_protocol::extension::NewDownload;
use wirecatch_protocol::wire::QueueSummary;
use wirecatch_protocol::{DownloadId, QueueId};

use super::addons::AddonSlot;
use super::handovers::Handovers;
use super::media::MediaPending;
use crate::error::{CoreError, Result};
use crate::media::{MediaJob, MediaProgress, Reading, StoredMedia};
use crate::naming::Claimed;
use crate::schedule::Gate;
use crate::speed::SpeedMeter;
use crate::store::{LastIds, NameChoice, Origin, StoredDownload};

pub(super) struct State {
    pub settings: AppSettings,
    /// In sidebar order.
    pub queues: Vec<QueueSlot>,
    pub downloads: BTreeMap<DownloadId, Entry>,
    /// Final paths of unfinished downloads, so a new one never picks the same.
    /// Kept up to date by [`State::insert_entry`], [`State::remove_entry`] and by
    /// completion, which hands the name to the finished file.
    pub claimed: Claimed,
    /// Downloads with a run, so the ticker visits only those.
    pub running: BTreeSet<DownloadId>,
    pub pending: BTreeMap<u64, Pending>,
    /// Media captures being looked at, waiting for the media add-on, or waiting
    /// for the user's choice of quality. Their ids come from the same counter as
    /// `pending`, so the two never share one.
    pub media: BTreeMap<u64, MediaPending>,
    /// The media add-on.
    pub addon: AddonSlot,
    pub handovers: Handovers,
    /// Rows changed since the last `DownloadsChanged` event.
    pub dirty: BTreeSet<DownloadId>,
    /// When schedules were last evaluated, so a start time crossed since then is seen.
    pub last_checked: NaiveDateTime,
    pub ids: Counters,
    /// Set by shutdown: nothing new starts.
    pub closed: bool,
}

/// Sources of fresh ids and positions.
#[derive(Debug, Default)]
pub(super) struct Counters {
    pub download: i64,
    pub queue: i64,
    pub pending: u64,
    pub run: u64,
    pub position: i64,
}

impl Counters {
    pub fn download(&mut self) -> DownloadId {
        self.download += 1;
        DownloadId(self.download)
    }

    pub fn queue(&mut self) -> QueueId {
        self.queue += 1;
        QueueId(self.queue)
    }

    pub fn pending(&mut self) -> u64 {
        self.pending += 1;
        self.pending
    }

    pub fn run(&mut self) -> u64 {
        self.run += 1;
        self.run
    }

    pub fn position(&mut self) -> i64 {
        self.position += 1;
        self.position
    }

    /// The highest download and queue ids handed out so far.
    pub fn last(&self) -> LastIds {
        LastIds {
            download: self.download,
            queue: self.queue,
        }
    }
}

/// A queue with what it needs at run time.
pub(super) struct QueueSlot {
    pub queue: Queue,
    /// Shared by every download of the queue; a new limit applies to running downloads at once.
    pub limiter: Arc<RateLimiter>,
    pub gate: Gate,
}

impl QueueSlot {
    pub fn new(queue: Queue) -> Self {
        Self {
            limiter: Arc::new(RateLimiter::new(queue.speed_limit)),
            queue,
            gate: Gate::default(),
        }
    }
}

/// One download.
pub(super) struct Entry {
    pub row: DownloadSummary,
    /// Order within its queue; lower runs first.
    pub position: i64,
    pub origin: Origin,
    /// Whether its path is final, and the user's or browser's choices for it.
    pub naming: NameChoice,
    /// What the browser saw of a captured file, checked when the server answers.
    pub expect: Option<Expectation>,
    /// Present while the engine works on it.
    pub run: Option<Run>,
    /// Was running when the app last stopped, so it gets the first free slot.
    pub resume_first: bool,
    /// Set while its partial files are being deleted; a new run waits for it.
    pub cleanup: Option<CancellationToken>,
    /// Being removed: hidden from the list and from every operation, and never started.
    pub removing: bool,
    /// Set while its files are moved to another folder (cancelled when done); it is
    /// not started meanwhile.
    pub moving: Option<CancellationToken>,
    /// For a stream or a page read by yt-dlp: what to download. `None` for every
    /// other download, including direct media files.
    pub media_job: Option<MediaJob>,
}

/// What the browser saw of a captured download, so the app can tell when its own
/// request got something else (another size, or a web page instead of the file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Expectation {
    /// The size the browser reported, if any.
    pub size: Option<u64>,
    /// The browser's name for the file, which says whether a web page is expected.
    pub name: Option<String>,
}

/// An engine run in progress.
pub(super) struct Run {
    pub id: u64,
    /// Cancelled once the server has answered with the file and its final path is
    /// chosen; only on a download's first run, whose path is not settled yet.
    pub answered: CancellationToken,
    pub cancel: CancellationToken,
    /// Cancelled by the run's task once the engine has returned and closed its files.
    pub exited: CancellationToken,
    pub gauge: Gauge,
    /// Whether the engine's part file got its Mark of the Web, so the file is
    /// marked before it is renamed into place.
    pub marked: bool,
    /// Why the run is being stopped, once it is.
    pub stop: Option<Stop>,
    pub meter: SpeedMeter,
    /// The run's task, so a run that ignores its stop request can be abandoned.
    pub task: AbortHandle,
}

/// Where a run's progress is read from.
pub(super) enum Gauge {
    Engine(Arc<Progress>),
    Media(Arc<MediaProgress>),
}

impl Gauge {
    pub fn read(&self) -> Reading {
        match self {
            Gauge::Engine(p) => Reading {
                answered: p.file_name().is_some(),
                done: p.done(),
                total: p.total(),
                connections: p.connections(),
                processing: false,
                waiting: false,
            },
            Gauge::Media(m) => m.read(),
        }
    }
}

/// Why a run was stopped, which decides the status it ends in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// The user paused it.
    Pause,
    /// A limit or schedule took its slot away; it waits for the next one.
    Requeue,
    /// The user cancelled it; the partial file is deleted.
    Cancel,
    /// The user cancelled it and then pressed Retry before it had stopped: the
    /// partial file is deleted and it goes back in line to start over.
    Restart,
    /// It is being removed from the list.
    Remove,
    /// The app is closing; it continues on the next start.
    Shutdown,
}

impl Stop {
    fn is_users_choice(self) -> bool {
        matches!(
            self,
            Stop::Pause | Stop::Cancel | Stop::Restart | Stop::Remove
        )
    }
}

/// A browser capture waiting for the user's confirmation.
pub(super) struct Pending {
    pub capture: PendingCapture,
    /// The extension's handover ids: one per capture of this link (several tabs
    /// can capture the same link while the question is showing). The question
    /// goes away only when every one of them is withdrawn.
    pub handovers: Vec<String>,
    pub request: NewDownload,
}

impl State {
    /// A download that exists and is not being removed.
    pub fn entry(&self, id: DownloadId) -> Result<&Entry> {
        self.downloads
            .get(&id)
            .filter(|e| !e.removing)
            .ok_or(CoreError::DownloadNotFound(id))
    }

    /// A download that exists and is not being removed.
    pub fn entry_mut(&mut self, id: DownloadId) -> Result<&mut Entry> {
        self.downloads
            .get_mut(&id)
            .filter(|e| !e.removing)
            .ok_or(CoreError::DownloadNotFound(id))
    }

    /// Every download that is not being removed.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.downloads.values().filter(|e| !e.removing)
    }

    pub fn slot(&self, id: QueueId) -> Result<&QueueSlot> {
        self.queues
            .iter()
            .find(|s| s.queue.id == id)
            .ok_or(CoreError::QueueNotFound(id))
    }

    pub fn slot_mut(&mut self, id: QueueId) -> Result<&mut QueueSlot> {
        self.queues
            .iter_mut()
            .find(|s| s.queue.id == id)
            .ok_or(CoreError::QueueNotFound(id))
    }

    /// The queue flagged as default; there always is one.
    pub fn default_queue(&self) -> QueueId {
        self.queues
            .iter()
            .find(|s| s.queue.is_default)
            .map_or(crate::settings::DEFAULT_QUEUE_ID, |s| s.queue.id)
    }

    pub fn queue_list(&self) -> Vec<Queue> {
        self.queues.iter().map(|s| s.queue.clone()).collect()
    }

    pub fn queue_summaries(&self) -> Vec<QueueSummary> {
        self.queues
            .iter()
            .map(|s| QueueSummary {
                id: s.queue.id,
                name: s.queue.name.clone(),
                is_default: s.queue.is_default,
            })
            .collect()
    }

    /// Adds a download, claiming its path unless it has already finished.
    pub fn insert_entry(&mut self, entry: Entry) {
        if entry.row.status != DownloadStatus::Completed {
            self.claimed.insert(entry.path());
        }
        self.downloads.insert(entry.row.id, entry);
    }

    /// Takes a download out, giving up its path.
    pub fn remove_entry(&mut self, id: DownloadId) -> Option<Entry> {
        let entry = self.downloads.remove(&id)?;
        self.claimed.remove(entry.path());
        self.running.remove(&id);
        self.dirty.remove(&id);
        Some(entry)
    }

    /// Checks that the maintained sets agree with the downloads. Debug builds only;
    /// every test that runs the ticker exercises it.
    #[cfg(debug_assertions)]
    pub fn assert_consistent(&self) {
        let running = self.downloads.values().filter(|e| e.is_running()).count();
        assert_eq!(running, self.running.len(), "running set out of step");
        assert!(
            self.running
                .iter()
                .all(|id| self.downloads.get(id).is_some_and(Entry::is_running)),
            "running set names a download that is not running"
        );
        let unfinished = self
            .downloads
            .values()
            .filter(|e| e.row.status != DownloadStatus::Completed);
        // Reservations made for a moment (see `Claimed`) are not counted.
        assert_eq!(
            unfinished.clone().count(),
            self.claimed.len(),
            "claimed paths out of step"
        );
        assert!(
            unfinished
                .into_iter()
                .all(|e| self.claimed.contains(e.path())),
            "an unfinished download's path is not claimed"
        );
    }

    /// Whether a queue other than `except` already uses `name`.
    pub fn queue_name_taken(&self, name: &str, except: Option<QueueId>) -> bool {
        self.queues
            .iter()
            .any(|s| Some(s.queue.id) != except && s.queue.name.eq_ignore_ascii_case(name))
    }

    /// Records that `id` changed, for the next batched event.
    pub fn touch(&mut self, id: DownloadId) {
        self.dirty.insert(id);
    }
}

impl Entry {
    /// The row as it leaves the manager (for the UI, events and callers):
    /// a password in its link is hidden. `row` keeps the real link.
    pub fn summary(&self) -> DownloadSummary {
        let mut row = self.row.clone();
        row.url = crate::redact::url_for_display(&row.url);
        // Some failures name the link they were about.
        if let Some(error) = &mut row.error {
            error.message = crate::redact::text_for_display(&error.message);
        }
        row
    }

    pub fn path(&self) -> &std::path::Path {
        std::path::Path::new(&self.row.path)
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Whether a server has answered the app with this file at some point, so the
    /// browser's copy is no longer needed.
    pub fn server_answered(&self) -> bool {
        self.naming.settled || self.row.status == DownloadStatus::Completed
    }

    pub fn stored(&self) -> StoredDownload {
        StoredDownload {
            summary: self.row.clone(),
            position: self.position,
            origin: self.origin.clone(),
            naming: self.naming.clone(),
            media: self.row.media.clone().map(|summary| StoredMedia {
                summary,
                job: self.media_job.clone(),
            }),
        }
    }

    /// The files and folders an unfinished download keeps next to its final path.
    pub fn partial_paths(&self) -> Vec<std::path::PathBuf> {
        let mut paths = crate::naming::partial_files(self.path()).to_vec();
        if self.media_job.is_some() {
            paths.push(crate::media::work_dir(self.path()));
            paths.push(crate::media::stream_dir(self.path()));
        }
        paths
    }

    /// Asks a running download to stop; the run's end decides its new status.
    /// Returns `false` if it was not running.
    pub fn stop(&mut self, why: Stop) -> bool {
        match &mut self.run {
            Some(run) => {
                // The user's latest choice wins, and the app's own reasons (a limit,
                // a schedule, shutting down) never override a choice the user made.
                if run
                    .stop
                    .is_none_or(|current| why.is_users_choice() || !current.is_users_choice())
                {
                    run.stop = Some(why);
                }
                run.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Resume or Retry pressed while the download is still stopping: once it has
    /// stopped it goes back in line instead of staying paused (or, with
    /// `restart_cancelled`, cancelled).
    pub fn requeue_when_stopped(&mut self, restart_cancelled: bool) {
        if let Some(run) = &mut self.run {
            match run.stop {
                Some(Stop::Pause) => run.stop = Some(Stop::Requeue),
                Some(Stop::Cancel) if restart_cancelled => run.stop = Some(Stop::Restart),
                _ => {}
            }
        }
    }

    /// Clears what only makes sense while running.
    pub fn clear_live_fields(&mut self) {
        self.row.speed = 0;
        self.row.connections = 0;
        self.row.eta_secs = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::Category;

    fn running() -> Entry {
        Entry {
            row: DownloadSummary {
                id: DownloadId(1),
                url: "https://example.com/f".into(),
                file_name: "f".into(),
                path: "f".into(),
                queue: QueueId(1),
                category: Category::Other,
                status: DownloadStatus::Downloading,
                total: None,
                done: 0,
                speed: 0,
                connections: 0,
                eta_secs: None,
                error: None,
                added_at: 0,
                completed_at: None,
                waiting: None,
                media: None,
            },
            position: 1,
            origin: Origin::default(),
            naming: NameChoice::settled(),
            expect: None,
            run: Some(Run {
                id: 1,
                answered: CancellationToken::new(),
                cancel: CancellationToken::new(),
                exited: CancellationToken::new(),
                gauge: Gauge::Engine(Arc::new(Progress::default())),
                marked: false,
                stop: None,
                meter: SpeedMeter::default(),
                task: idle_task(),
            }),
            resume_first: false,
            cleanup: None,
            removing: false,
            moving: None,
            media_job: None,
        }
    }

    fn idle_task() -> AbortHandle {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.spawn(async {}).abort_handle()
    }

    fn reason_after(steps: &[Stop]) -> Option<Stop> {
        let mut entry = running();
        for step in steps {
            assert!(entry.stop(*step));
        }
        let run = entry.run.unwrap();
        assert!(run.cancel.is_cancelled());
        run.stop
    }

    #[test]
    fn the_row_that_leaves_the_manager_hides_link_passwords() {
        let mut entry = running();
        entry.row.url = "https://ada:s3cret@example.com/a.zip".into();
        entry.row.error = Some(wirecatch_protocol::app::UserError {
            message: "The key at https://ada:s3cret@example.com/key is not usable.".into(),
            fix: None,
        });
        let shown = entry.summary();
        assert_eq!(shown.url, "https://ada:***@example.com/a.zip");
        assert_eq!(
            shown.error.unwrap().message,
            "The key at https://ada:***@example.com/key is not usable."
        );
        // The manager keeps the real link.
        assert_eq!(entry.row.url, "https://ada:s3cret@example.com/a.zip");
    }

    #[test]
    fn the_users_choice_is_never_overridden_by_the_app() {
        assert_eq!(
            reason_after(&[Stop::Pause, Stop::Shutdown]),
            Some(Stop::Pause)
        );
        assert_eq!(
            reason_after(&[Stop::Cancel, Stop::Requeue]),
            Some(Stop::Cancel)
        );
        assert_eq!(
            reason_after(&[Stop::Requeue, Stop::Pause]),
            Some(Stop::Pause)
        );
        assert_eq!(
            reason_after(&[Stop::Requeue, Stop::Shutdown]),
            Some(Stop::Shutdown)
        );
        assert_eq!(
            reason_after(&[Stop::Pause, Stop::Remove]),
            Some(Stop::Remove)
        );
    }

    #[test]
    fn stopping_an_idle_download_does_nothing() {
        let mut entry = running();
        entry.run = None;
        assert!(!entry.stop(Stop::Pause));
    }
}
