//! The manager's in-memory state. Everything here is guarded by one mutex that is
//! never held across an `.await`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::NaiveDateTime;
use tokio::task::AbortHandle;
use wirecatch_engine::{
    CancellationToken, ManualProxies, Progress, ProxyChoice, ProxyRoute, RateLimiter, Url,
};
use wirecatch_media::{MediaJob, MediaProgress, Reading, StoredMedia};
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{
    AppSettings, Category, DownloadStatus, DownloadSummary, FileExistsQuestion, PendingCapture,
    Queue, Setting, WhenFileExists,
};
use wirecatch_protocol::extension::NewDownload;
use wirecatch_protocol::proxy::{ProxyServer, ProxyUse};
use wirecatch_protocol::wire::QueueSummary;
use wirecatch_protocol::{DownloadId, ProxyServerId, QuestionId, QueueId};
use wirecatch_queues::schedule::Gate;
use wirecatch_settings::folders::KnownFolders;
use wirecatch_settings::proxy::{ComputerProxy, Logins, SavedProxyServer};
use wirecatch_sites::local_network::ChosenBy;
use wirecatch_sites::public_suffix::SuffixList;
use wirecatch_store::{LastIds, NameChoice, StoredDownload};

use super::addons::AddonSlot;
use super::handovers::Handovers;
use super::media::MediaPending;
use super::questions::{Questions, Waiting};
use super::suffix_list::SuffixCheck;
use crate::batches::Batches;
use crate::error::{CoreError, Result};
use crate::headers::Origin;
use crate::naming::Claimed;
use crate::speed::SpeedMeter;

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
    /// The system's proxy setting as last read, for the proxy a run is shown going through: read off
    /// the lock, as on Windows it is in the registry. The run itself reads it when it connects.
    pub system_proxy: Option<ManualProxies>,
    /// Where the computer's proxy is read from ([`ManagerConfig::computer_proxy`](crate::ManagerConfig::computer_proxy)).
    pub computer_proxy: ComputerProxy,
    /// Every unfinished download, and any finished since [`State::prune_finished`] last ran: the scheduler visits these, not the whole history (CORE-10). A finished download never becomes unfinished again, so only adding one puts it here.
    pub unfinished: BTreeSet<DownloadId>,
    pub pending: Questions<Pending>,
    /// Media captures being looked at, waiting for the media add-on, or waiting
    /// for the user's choice of quality. Their ids come from the same counter as
    /// `pending`, so the two never share one.
    pub media: Questions<MediaPending>,
    /// Downloads paused until the person says what to do with the file already in their
    /// folder (D88). Ids from the same counter as `pending`.
    pub file_exists: BTreeMap<QuestionId, Asked>,
    /// The media add-on.
    pub addon: AddonSlot,
    /// The Public Suffix List every decision about sites reads (D39, D44).
    pub suffixes: Arc<SuffixList>,
    /// The last time a newer list was looked for.
    pub suffix_check: SuffixCheck,
    pub handovers: Handovers,
    /// Rows changed since the last `DownloadsChanged` event.
    pub dirty: BTreeSet<DownloadId>,
    /// What each queue at work has finished or failed so far (D52).
    pub batches: Batches,
    /// The passwords of the proxy servers, which never leave the app (D71.2).
    pub proxy_logins: Logins,
    pub proxy_servers: Vec<SavedProxyServer>,
    /// When schedules were last evaluated, so a start time crossed since then is seen.
    pub last_checked: NaiveDateTime,
    pub ids: Counters,
    /// Set by shutdown: nothing new starts.
    pub closed: bool,
}

/// Sources of fresh ids and positions.
#[derive(Debug, Default, Clone)]
pub(super) struct Counters {
    pub download: i64,
    pub queue: i64,
    pub proxy_server: i64,
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

    pub fn proxy_server(&mut self) -> ProxyServerId {
        self.proxy_server += 1;
        ProxyServerId(self.proxy_server)
    }

    /// A fresh question id, shared by pending captures, media and files already in a folder.
    pub fn pending(&mut self) -> QuestionId {
        self.pending += 1;
        QuestionId(self.pending)
    }

    pub fn run(&mut self) -> u64 {
        self.run += 1;
        self.run
    }

    pub fn position(&mut self) -> i64 {
        self.position += 1;
        self.position
    }

    /// The highest download, queue and proxy server ids handed out so far.
    pub fn last(&self) -> LastIds {
        LastIds {
            download: self.download,
            queue: self.queue,
            proxy_server: self.proxy_server,
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
            limiter: Arc::new(RateLimiter::new(queue.settings.speed_limit.own())),
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
    /// Who chose its link, which decides whether it may reach the local network.
    pub chosen_by: ChosenBy,
    /// Whether its path is final, and the user's or browser's choices for it.
    pub naming: NameChoice,
    /// What the browser saw of a captured file, checked when the server answers.
    pub expect: Option<Expectation>,
    /// Present while the engine works on it.
    pub run: Option<Run>,
    /// Was running when the app last stopped, so it gets the first free slot.
    pub resume_first: bool,
    /// The proxy its run went through, in words, while it is running and after it failed:
    /// `None` when the run went directly.
    pub through_proxy: Option<String>,
    /// Set while its partial files are being deleted; a new run waits for it.
    pub cleanup: Option<CancellationToken>,
    /// A move or a removal under way, which keeps it from starting (CORE-12).
    pub hold: Hold,
    /// Where a move that failed halfway and could not be undone left some of its
    /// files: that path stays taken (reserved) until the download is tried again
    /// or removed, so no other download takes the name next to them, and the
    /// download is not moved or relinked meanwhile. Saved with the download, so
    /// this holds across restarts.
    pub stranded: Option<PathBuf>,
    /// For a stream or a page read by yt-dlp: what to download. `None` for every
    /// other download, including direct media files.
    pub media_job: Option<MediaJob>,
    /// What the person chose for the file already in its folder (D88), for its next run. Not
    /// saved: after a restart a duplicate is judged again.
    pub on_existing: Option<OnExisting>,
    /// Added in the app itself: if the server first answers with a web page while
    /// the media add-on is installed, that page is read for its video instead
    /// (D51). Not saved, so a download restored after a restart is not checked.
    pub video_check: bool,
}

/// A [`FileExistsQuestion`] and the file it is about, whose path its words may not hold exactly
/// (a name that is not valid Unicode).
pub(super) struct Asked {
    pub question: FileExistsQuestion,
    pub path: PathBuf,
}

/// What a download does about the file already in its folder, once decided (D88).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnExisting {
    /// Its run finishes over the file at its path.
    Replace,
    /// Its path is the next free name.
    KeepBoth,
}

/// A file found where a download would save (D88, D96), and what the download does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    pub path: PathBuf,
    pub size: u64,
    pub saved_at: Option<UnixMillis>,
    pub then: WhenFileExists,
    /// It looks like the file the download would save, not a different file of the same name.
    pub same: bool,
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
    /// The file already in the folder, with [`Stop::FileExists`].
    pub found: Option<Found>,
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
                processing: None,
                retry_at: None,
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
    /// The server answered with a web page, which is read for its video instead
    /// of downloaded (D51).
    ReadAsVideo,
    /// Its file is already in the folder (D88): what was found is [`Run::found`].
    FileExists,
}

/// What a download that is being cancelled does once it has stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterCancel {
    Stay,
    Restart,
}

impl Entry {
    /// Queued but not started while its files move to another name.
    pub fn held_for_another_name(&mut self) {
        self.hold.moving(CancellationToken::new());
        self.row.status = DownloadStatus::Queued;
    }
}

/// What keeps a download from starting, besides a run of its own. A removal that finds a move under way waits for it, so the right files are deleted.
#[derive(Debug, Default)]
pub(super) enum Hold {
    #[default]
    Free,
    /// Its files are moving to another folder or name; cancelled when done.
    Moving(CancellationToken),
    /// Being removed: hidden from the list and from every operation, and never started.
    Removing {
        after_move: Option<CancellationToken>,
    },
}

impl Hold {
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }

    pub fn is_removing(&self) -> bool {
        matches!(self, Self::Removing { .. })
    }

    /// Starts a move of a free download, ended by [`end_move`](Self::end_move).
    pub fn begin_move(&mut self) {
        debug_assert!(self.is_free(), "a move began while {self:?}");
        *self = Self::Moving(CancellationToken::new());
    }

    /// Marks a move under way with `done`, kept through a removal.
    fn moving(&mut self, done: CancellationToken) {
        match self {
            Self::Removing { after_move } => *after_move = Some(done),
            _ => *self = Self::Moving(done),
        }
    }

    /// Ends a move, waking a removal waiting for it.
    pub fn end_move(&mut self) {
        match std::mem::take(self) {
            Self::Moving(done) => done.cancel(),
            Self::Removing { after_move } => {
                if let Some(done) = after_move {
                    done.cancel();
                }
                *self = Self::Removing { after_move: None };
            }
            Self::Free => {}
        }
    }

    pub fn begin_remove(&mut self) {
        let after_move = match std::mem::take(self) {
            Self::Moving(done) => Some(done),
            Self::Removing { after_move } => after_move,
            Self::Free => None,
        };
        *self = Self::Removing { after_move };
    }

    /// The move under way, for a removal to wait for.
    pub fn move_under_way(&self) -> Option<CancellationToken> {
        match self {
            Self::Moving(done)
            | Self::Removing {
                after_move: Some(done),
            } => Some(done.clone()),
            _ => None,
        }
    }
}

impl Stop {
    fn is_users_choice(self) -> bool {
        matches!(
            self,
            Stop::Pause | Stop::Cancel | Stop::Restart | Stop::Remove
        )
    }
}

impl Waiting for Pending {
    fn handovers(&mut self) -> &mut Vec<String> {
        &mut self.handovers
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
    /// Who chose the link, decided when it was captured.
    pub chosen_by: ChosenBy,
}

impl State {
    /// The proxy of the app's own requests (add-on and suffix-list fetches, the updater): Main's
    /// (D73.2). A download's is in its [`RunPolicy`](super::policy::RunPolicy).
    pub fn app_proxy(&self) -> ProxyChoice {
        self.queue_proxy(self.slot(self.default_queue()).ok().map(|s| &s.queue))
    }

    /// The proxy `queue`'s downloads go through, its own or the app's; the app's with no queue.
    pub fn queue_proxy(&self, queue: Option<&Queue>) -> ProxyChoice {
        self.queue_proxy_with_use(queue).1
    }

    fn queue_proxy_with_use(&self, queue: Option<&Queue>) -> (ProxyUse, ProxyChoice) {
        let app = &self.settings.proxy;
        let (proxy, exceptions) = match queue {
            Some(queue) => (
                queue.settings.proxy.own().unwrap_or(app.proxy),
                wirecatch_settings::proxy::exceptions(app, &queue.settings.exceptions),
            ),
            None => (
                app.proxy,
                wirecatch_settings::proxy::exceptions(app, &Setting::Global),
            ),
        };
        let choice = wirecatch_settings::proxy::engine_choice(
            proxy,
            &exceptions,
            app.ftp_bypass_proxy,
            &self.proxy_servers,
            &self.proxy_logins,
            &self.computer_proxy,
        );
        (proxy, choice)
    }

    /// How a run in `queue` reaches `url`, in words, and the proxy it goes through when it goes
    /// through one (D73.3).
    pub fn proxy_route(&self, queue: QueueId, url: &str) -> (String, Option<String>) {
        let (proxy, choice) = self.queue_proxy_with_use(self.slot(queue).ok().map(|s| &s.queue));
        let choice = match choice {
            ProxyChoice::System => self
                .system_proxy
                .clone()
                .map_or(ProxyChoice::Direct, ProxyChoice::Manual),
            other => other,
        };
        let route = Url::parse(url).map_or(ProxyRoute::Direct, |url| choice.route(&url));
        match route {
            ProxyRoute::Through(address) => {
                let address = address
                    .split_once("://")
                    .map_or(address.as_str(), |(_, a)| a);
                let profile = match proxy {
                    ProxyUse::Server(id) => self.proxy_servers.iter().find(|p| p.id == id),
                    _ => None,
                };
                let label = match profile {
                    Some(p) if !p.settings.name.trim().is_empty() => {
                        format!("{} ({})", p.settings.name.trim(), p.settings.address())
                    }
                    Some(p) => p.settings.address(),
                    None => format!("System proxy ({address})"),
                };
                (label.clone(), Some(label))
            }
            ProxyRoute::Excepted => ("No proxy (an exception)".into(), None),
            _ => ("No proxy".into(), None),
        }
    }

    /// The proxy servers as the windows see them, without their passwords.
    pub fn proxy_server_list(&self) -> Vec<ProxyServer> {
        self.proxy_servers
            .iter()
            .map(|p| ProxyServer {
                id: p.id,
                settings: p.settings.clone(),
                has_password: self.proxy_logins.contains_key(&p.id),
                last_test: p.last_test.clone(),
            })
            .collect()
    }

    /// A download that exists and is not being removed.
    pub fn entry(&self, id: DownloadId) -> Result<&Entry> {
        self.downloads
            .get(&id)
            .filter(|e| !e.hold.is_removing())
            .ok_or(CoreError::DownloadNotFound(id))
    }

    /// A download that exists and is not being removed.
    pub fn entry_mut(&mut self, id: DownloadId) -> Result<&mut Entry> {
        self.downloads
            .get_mut(&id)
            .filter(|e| !e.hold.is_removing())
            .ok_or(CoreError::DownloadNotFound(id))
    }

    /// Every download that is not being removed.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.downloads.values().filter(|e| !e.hold.is_removing())
    }

    pub fn slot(&self, id: QueueId) -> Result<&QueueSlot> {
        self.queues
            .iter()
            .find(|s| s.queue.id == id)
            .ok_or(CoreError::QueueNotFound(id))
    }

    /// The folder new downloads of `category` go to in `queue` (D94.2).
    pub fn folder_for(
        &self,
        queue: QueueId,
        category: Category,
        folders: &KnownFolders,
    ) -> Result<PathBuf> {
        let queue = &self.slot(queue)?.queue.settings;
        Ok(wirecatch_settings::folder_for(
            &self.settings,
            queue,
            category,
            folders,
        )?)
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
            .map_or(wirecatch_settings::DEFAULT_QUEUE_ID, |s| s.queue.id)
    }

    pub fn queue_list(&self) -> Vec<Queue> {
        self.queues.iter().map(|s| s.queue.clone()).collect()
    }

    pub fn queue_summaries(&self) -> Vec<QueueSummary> {
        self.queues
            .iter()
            .map(|s| QueueSummary {
                id: s.queue.id,
                name: s.queue.settings.name.clone(),
                is_default: s.queue.is_default,
            })
            .collect()
    }

    /// Adds a download, claiming its path unless it has already finished.
    pub fn insert_entry(&mut self, entry: Entry) {
        if entry.row.status != DownloadStatus::Completed {
            self.claimed.insert(entry.path());
            self.unfinished.insert(entry.row.id);
        }
        if let Some(stranded) = &entry.stranded {
            self.claimed.reserve(stranded);
        }
        self.downloads.insert(entry.row.id, entry);
    }

    /// Gives up the path a failed move left files at, if any (see
    /// [`Entry::stranded`]).
    pub fn release_stranded(&mut self, id: DownloadId) {
        if let Some(stranded) = self
            .downloads
            .get_mut(&id)
            .and_then(|entry| entry.stranded.take())
        {
            self.claimed.release(&stranded);
        }
    }

    /// Takes a download out, giving up its path.
    pub fn remove_entry(&mut self, id: DownloadId) -> Option<Entry> {
        let entry = self.downloads.remove(&id)?;
        self.claimed.remove(entry.path());
        if let Some(stranded) = &entry.stranded {
            self.claimed.release(stranded);
        }
        self.running.remove(&id);
        self.unfinished.remove(&id);
        self.dirty.remove(&id);
        self.batches.forget(id);
        Some(entry)
    }

    /// Drops downloads that have finished from [`unfinished`](Self::unfinished).
    pub fn prune_finished(&mut self) {
        let downloads = &self.downloads;
        self.unfinished.retain(|id| {
            downloads
                .get(id)
                .is_some_and(|e| e.row.status != DownloadStatus::Completed)
        });
    }

    /// Every unfinished download.
    pub fn unfinished(&self) -> impl Iterator<Item = &Entry> {
        self.unfinished
            .iter()
            .filter_map(|id| self.downloads.get(id))
            .filter(|e| e.row.status != DownloadStatus::Completed)
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
        assert!(
            self.downloads
                .values()
                .filter(|e| e.row.status != DownloadStatus::Completed)
                .all(|e| self.unfinished.contains(&e.row.id)),
            "an unfinished download is missing from the unfinished set"
        );
    }

    /// Whether a queue other than `except` already uses `name`.
    pub fn queue_name_taken(&self, name: &str, except: Option<QueueId>) -> bool {
        self.queues
            .iter()
            .any(|s| Some(s.queue.id) != except && s.queue.settings.name.eq_ignore_ascii_case(name))
    }

    /// Records that `id` changed, for the next batched event.
    pub fn touch(&mut self, id: DownloadId) {
        self.dirty.insert(id);
    }
}

impl Entry {
    /// Refuses an operation that would start the download while a failed move
    /// has left some of its files in another folder (see [`Entry::stranded`]).
    pub fn refuse_if_stranded(&self) -> Result<()> {
        if self.stranded.is_some() {
            return Err(CoreError::WrongState(
                "Some of this download's files are still in the folder it was being moved to. Move them back as its error says, then try it again.",
            ));
        }
        Ok(())
    }

    /// The row as it leaves the manager (for the UI, events and callers):
    /// a password in its link is hidden. `row` keeps the real link.
    pub fn summary(&self) -> DownloadSummary {
        let mut row = self.row.clone();
        row.url = wirecatch_logging::redact::url_for_display(&row.url);
        // Some failures name the link they were about.
        if let Some(error) = &mut row.error {
            error.message = wirecatch_logging::redact::text_for_display(&error.message);
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
            chosen_by: self.chosen_by,
            naming: self.naming.clone(),
            media: self.row.media.clone().map(|summary| StoredMedia {
                summary,
                job: self.media_job.clone(),
            }),
            stranded: self.stranded.clone(),
        }
    }

    /// The files and folders an unfinished download keeps next to its final path.
    pub fn partial_paths(&self) -> Vec<std::path::PathBuf> {
        let mut paths = crate::naming::partial_files(self.path()).to_vec();
        if self.media_job.is_some() {
            paths.push(wirecatch_media::work_dir(self.path()));
            paths.push(wirecatch_media::stream_dir(self.path()));
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
    /// stopped it goes back in line instead of staying paused, and a cancelled one as `cancelled` says.
    pub fn requeue_when_stopped(&mut self, cancelled: AfterCancel) {
        if let Some(run) = &mut self.run {
            match run.stop {
                Some(Stop::Pause) => run.stop = Some(Stop::Requeue),
                Some(Stop::Cancel) if cancelled == AfterCancel::Restart => {
                    run.stop = Some(Stop::Restart);
                }
                _ => {}
            }
        }
    }

    /// Clears what only makes sense while running.
    pub fn clear_live_fields(&mut self) {
        self.row.speed = 0;
        self.row.connections = 0;
        self.row.eta_secs = None;
        self.row.processing = None;
    }
}

/// A download that is not running, of `url` whose link `chosen_by` chose,
/// for tests of what a run is given.
#[cfg(test)]
pub(super) fn idle_entry(url: &str, chosen_by: ChosenBy) -> Entry {
    Entry {
        row: DownloadSummary {
            url: url.into(),
            ..wirecatch_test_support::downloads::download(1)
        },
        position: 1,
        origin: Origin::default(),
        chosen_by,
        naming: NameChoice::settled(),
        expect: None,
        run: None,
        resume_first: false,
        through_proxy: None,
        cleanup: None,
        hold: Hold::Free,
        on_existing: None,
        stranded: None,
        media_job: None,
        video_check: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Entry {
        Entry {
            row: DownloadSummary {
                status: DownloadStatus::Downloading,
                ..wirecatch_test_support::downloads::download(1)
            },
            position: 1,
            origin: Origin::default(),
            chosen_by: ChosenBy::Person,
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
                found: None,
                meter: SpeedMeter::default(),
                task: idle_task(),
            }),
            resume_first: false,
            through_proxy: None,
            cleanup: None,
            hold: Hold::Free,
            on_existing: None,
            stranded: None,
            media_job: None,
            video_check: false,
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

    /// A removal that finds a move under way waits for it, and the move's end wakes it without making the download free again.
    #[test]
    fn a_removal_during_a_move_waits_for_it() {
        let mut hold = Hold::default();
        hold.begin_move();
        hold.begin_remove();
        let moved = hold.move_under_way().expect("the removal lost the move");
        assert!(hold.is_removing());
        hold.end_move();
        assert!(moved.is_cancelled(), "the removal was never woken");
        assert!(hold.is_removing() && hold.move_under_way().is_none());
    }
}
