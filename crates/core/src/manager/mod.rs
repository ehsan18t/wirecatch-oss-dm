//! The download manager: owns every download, queue and setting, decides what runs,
//! and tells the UI and socket clients about changes.

mod added_pages;
mod addons;
mod arrival;
mod captures;
mod downloads;
#[cfg(test)]
mod fake;
mod handovers;
mod media;

mod media_run;
mod placement;
mod policy;
mod preferences;
mod proxy;
mod queues;
mod relocate;
mod restore;
mod run;
mod scheduling;
mod settle;
mod sign_in;
mod startup;
mod state;
mod suffix_list;
#[cfg(test)]
mod tests;
mod ticker;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tracing::warn;
use wirecatch_addons::manifest::MediaAddonSources;
use wirecatch_engine::{CancellationToken, RateLimiter, Url};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{StartupNotice, UiEvent};
use wirecatch_protocol::extension::{BrowserNote, NoteId, NotePressed};
use wirecatch_protocol::wire::Event;
use wirecatch_sites::public_suffix::SuffixList;
use wirecatch_store::{Recovery, Store};

use crate::clock::{Clock, SystemClock};
use crate::error::{CoreError, Result, blocking};
use crate::folders::KnownFolders;
use crate::naming;
use crate::user_error;
pub use downloads::LinkFrom;
use state::{Entry, State, Stop};
pub use suffix_list::PUBLIC_SUFFIX_LIST_URL;

/// How many UI events may wait for a slow subscriber before it starts missing some.
const UI_EVENT_BUFFER: usize = 1024;
/// The same for protocol events.
const WIRE_EVENT_BUFFER: usize = 64;
/// Longest wait at shutdown for the database to save the last changes and close.
const DATABASE_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Where the manager keeps its data and how it reads the world around it.
#[derive(Clone)]
pub struct ManagerConfig {
    /// Folder for the database. The desktop app passes the per-user app data folder.
    /// The manager makes the folder and the database readable only by the current
    /// user (an owner-only access list on Windows, modes 0700 and 0600 on Linux),
    /// because the database holds browser cookies.
    pub data_dir: PathBuf,
    /// The user's standard folders, for resolving `{Downloads}` and friends.
    pub folders: KnownFolders,
    /// Source of the time of day for schedules and timestamps.
    pub clock: Arc<dyn Clock>,
    /// How long a download may take to stop after being asked (pause, remove,
    /// shutdown). One still going after this, for example stuck waiting on a server
    /// that never answers, is abandoned so nothing waits on it forever.
    pub stop_timeout: Duration,
    /// How long a captured download may wait for the server's first answer before
    /// the app gives up and the browser keeps the download. Shorter than the
    /// extension's own wait for the app's reply.
    pub capture_timeout: Duration,
    /// Where the media add-on comes from: the pinned official releases, or a
    /// local server in tests.
    pub addon_sources: MediaAddonSources,
    /// How long the media add-on's yt-dlp may take to read a page.
    pub media_timeout: Duration,
    /// The pauses before a page's video download reads the page again for
    /// fresh addresses, after the server refused one or said it expired. One
    /// entry per automatic try; once they are used up, the download fails.
    pub media_refresh_delays: Vec<Duration>,
    /// Where a newer Public Suffix List is fetched from (D44), or `None` to keep the list
    /// built in, as [`new`](Self::new) does so no test reaches the internet. The app
    /// fetches it from publicsuffix.org ([`with_official_suffix_list`](Self::with_official_suffix_list)).
    pub suffix_list_source: Option<Url>,
    /// Where the proxy's Test connection sends its request (D71.5), or `None`, as
    /// [`new`](Self::new) does so no test reaches the internet: a test then finds nothing to
    /// reach. The app sends it to [`PROXY_TEST_URL`](crate::PROXY_TEST_URL)
    /// ([`with_official_proxy_test`](Self::with_official_proxy_test)).
    pub proxy_test_url: Option<Url>,
}

/// The default [`ManagerConfig::capture_timeout`].
pub const DEFAULT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

/// The default [`ManagerConfig::stop_timeout`].
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The default [`ManagerConfig::media_timeout`].
pub const DEFAULT_MEDIA_TIMEOUT: Duration = Duration::from_secs(120);

/// The default [`ManagerConfig::media_refresh_delays`]: six more tries, after
/// 2, 10 and 30 seconds, then 1, 2 and 4 minutes.
///
/// A video server sometimes refuses a perfectly good address for a while, and
/// an address read again from the page usually works at once, so the first try
/// comes quickly. YouTube has been seen refusing one video for several minutes
/// and then sending it as usual, so the later tries spread over about eight
/// minutes in all, with the row saying when the next one is, rather than
/// giving up after a minute and asking the person to press Try again. The gaps
/// grow so the page is not read so often that the site takes it for a robot.
pub const DEFAULT_MEDIA_REFRESH_DELAYS: [Duration; 6] = [
    Duration::from_secs(2),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
];

impl ManagerConfig {
    /// A configuration using the real clock.
    pub fn new(data_dir: impl Into<PathBuf>, folders: KnownFolders) -> Self {
        Self {
            data_dir: data_dir.into(),
            folders,
            clock: Arc::new(SystemClock),
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            capture_timeout: DEFAULT_CAPTURE_TIMEOUT,
            addon_sources: MediaAddonSources::official(),
            media_timeout: DEFAULT_MEDIA_TIMEOUT,
            media_refresh_delays: DEFAULT_MEDIA_REFRESH_DELAYS.to_vec(),
            suffix_list_source: None,
            proxy_test_url: None,
        }
    }

    /// Tests a proxy setting with a request to `url`; see
    /// [`proxy_test_url`](Self::proxy_test_url).
    #[must_use]
    pub fn with_proxy_test_url(mut self, url: Url) -> Self {
        self.proxy_test_url = Some(url);
        self
    }

    /// Tests a proxy setting with a request to [`PROXY_TEST_URL`](crate::PROXY_TEST_URL), as
    /// the app does.
    #[must_use]
    pub fn with_official_proxy_test(mut self) -> Self {
        // A constant a test reads, so it always parses.
        self.proxy_test_url = Url::parse(crate::PROXY_TEST_URL).ok();
        self
    }

    /// Fetches a newer Public Suffix List from `source`; see
    /// [`suffix_list_source`](Self::suffix_list_source).
    #[must_use]
    pub fn with_suffix_list_source(mut self, source: Url) -> Self {
        self.suffix_list_source = Some(source);
        self
    }

    /// Fetches a newer Public Suffix List from where publicsuffix.org publishes it,
    /// [`PUBLIC_SUFFIX_LIST_URL`], as the app does (D44).
    #[must_use]
    pub fn with_official_suffix_list(mut self) -> Self {
        // A constant a test reads, so it always parses.
        self.suffix_list_source = Url::parse(PUBLIC_SUFFIX_LIST_URL).ok();
        self
    }

    /// Installs and updates the media add-on from `sources` instead of the
    /// official releases.
    #[must_use]
    pub fn with_addon_sources(mut self, sources: MediaAddonSources) -> Self {
        self.addon_sources = sources;
        self
    }

    /// Uses `timeout` as the [`media_timeout`](Self::media_timeout).
    #[must_use]
    pub fn with_media_timeout(mut self, timeout: Duration) -> Self {
        self.media_timeout = timeout;
        self
    }

    /// Uses `delays` as the [`media_refresh_delays`](Self::media_refresh_delays).
    #[must_use]
    pub fn with_media_refresh_delays(mut self, delays: Vec<Duration>) -> Self {
        self.media_refresh_delays = delays;
        self
    }

    /// Uses `clock` instead of the real clock.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Uses `timeout` as the [`capture_timeout`](Self::capture_timeout).
    #[must_use]
    pub fn with_capture_timeout(mut self, timeout: Duration) -> Self {
        self.capture_timeout = timeout;
        self
    }

    /// Uses `timeout` as the [`stop_timeout`](Self::stop_timeout).
    #[must_use]
    pub fn with_stop_timeout(mut self, timeout: Duration) -> Self {
        self.stop_timeout = timeout;
        self
    }
}

/// The download manager. Cheap to clone; every clone is the same manager.
///
/// Operations take effect at once and report what changed through [`subscribe`]
/// (for the desktop UI) and [`subscribe_events`] (for socket clients). Method names
/// match the desktop app's Tauri commands.
///
/// [`subscribe`]: Manager::subscribe
/// [`subscribe_events`]: Manager::subscribe_events
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

struct Inner {
    /// The runtime the manager was opened on. Every task is spawned here, so
    /// methods also work when called from a thread outside any runtime (Tauri runs
    /// synchronous commands on its main thread).
    runtime: Handle,
    state: Mutex<State>,
    store: Store,
    clock: Arc<dyn Clock>,
    stop_timeout: Duration,
    capture_timeout: Duration,
    folders: KnownFolders,
    /// The media add-on's folder.
    addon_dir: PathBuf,
    addon_sources: MediaAddonSources,
    /// A private folder for short-lived files (cookies handed to yt-dlp).
    temp_dir: PathBuf,
    /// The data folder, where a newer Public Suffix List is kept.
    data_dir: PathBuf,
    suffix_list_source: Option<Url>,
    proxy_test_url: Option<Url>,
    /// Held by a check of the Public Suffix List, so two never overlap.
    suffix_checking: tokio::sync::Mutex<()>,
    media_timeout: Duration,
    media_refresh_delays: Arc<[Duration]>,
    /// Shared by every download for the global speed limit.
    global_limiter: Arc<RateLimiter>,
    ui_events: broadcast::Sender<UiEvent>,
    wire_events: broadcast::Sender<Event>,
    /// Wakes the ticker when a row changes or a download starts.
    rows_changed: Arc<Notify>,
    /// Wakes the scheduler when queues change.
    queues_changed: Arc<Notify>,
    /// Stops the background tasks.
    stopped: CancellationToken,
    /// What to tell the person about this start, if anything.
    startup_notices: Vec<StartupNotice>,
    /// Where a damaged download list was set aside at this start, if one was.
    set_aside_list: Option<PathBuf>,
    /// Plays the engine in unit tests.
    #[cfg(test)]
    fake_engine: Mutex<Option<fake::FakeEngine>>,
    /// Moves files in unit tests, so a move can be made to fail.
    #[cfg(test)]
    fake_mover: Mutex<Option<fake::FakeMover>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}

impl Manager {
    /// Opens the manager on `config.data_dir`, restoring every download. Downloads
    /// that were running when the app last stopped continue on their own.
    ///
    /// Must be called from inside a Tokio runtime; the manager keeps a handle to it
    /// and runs all its work there, so its methods may then be called from any thread.
    ///
    /// # Errors
    ///
    /// [`CoreError::Store`] when the download list was saved by a newer Wirecatch,
    /// another copy has it open, or it cannot be opened or created in `config.data_dir`. A damaged list is not an error: it is
    /// set aside and a new one started.
    pub async fn open(config: ManagerConfig) -> Result<Self> {
        let runtime = Handle::current();
        let ui_events = broadcast::Sender::new(UI_EVENT_BUFFER);
        let failures = ui_events.clone();
        let on_failure = Box::new(move |reason: String| {
            publish(
                &failures,
                UiEvent::StorageFailed(user_error::storage_failed(&reason)),
            );
        });
        let dir = config.data_dir.clone();
        let addon_dir = wirecatch_addons::media_dir(&config.data_dir);
        let temp_dir = config.data_dir.join("tmp");
        let (now, now_millis) = (config.clock.local_now(), config.clock.unix_millis());
        let (addons_at, temp_at) = (addon_dir.clone(), temp_dir.clone());
        // Restoring checks the disk for finished files, so it runs off async threads too.
        let notices_dir = dir.clone();
        let (store, restored, recoveries, not_private) = blocking(&runtime, move || {
            let (store, mut snapshot) = Store::open(&dir, on_failure)?;
            let recoveries = std::mem::take(&mut snapshot.recoveries);
            let not_private = snapshot.not_private.take();
            let around = restore::Surroundings {
                now,
                now_millis,
                finished_size: &naming::finished_size,
                suffixes: SuffixList::load(&dir, now_millis),
            };
            let mut restored = restore::restore(snapshot, &around);
            restored.state.addon = addons::AddonSlot::load(&addons_at);
            restored.state.suffix_check = suffix_list::SuffixCheck::load(&dir);
            // Cookies files left by a crash: nothing may keep them.
            wirecatch_files::discard::dir(&temp_at);
            Ok((store, restored, recoveries, not_private))
        })
        .await?;
        // Written back only when restoring changed them, so what another build saved and this one
        // cannot read (an accent added later, say) survives this build opening them.
        if restored.settings_changed {
            store.save_settings(restored.state.settings.clone());
        }
        store.save_queues(restored.state.queue_list());
        store.save_downloads(restored.changed);

        let global_limiter = Arc::new(RateLimiter::new(restored.state.settings.global_speed_limit));
        let inner = Arc::new(Inner {
            runtime,
            state: Mutex::new(restored.state),
            store,
            clock: config.clock,
            stop_timeout: config.stop_timeout,
            capture_timeout: config.capture_timeout,
            folders: config.folders,
            addon_dir,
            addon_sources: config.addon_sources,
            temp_dir,
            data_dir: config.data_dir,
            suffix_list_source: config.suffix_list_source,
            proxy_test_url: config.proxy_test_url,
            suffix_checking: tokio::sync::Mutex::new(()),
            media_timeout: config.media_timeout,
            media_refresh_delays: config.media_refresh_delays.into(),
            global_limiter,
            ui_events,
            wire_events: broadcast::Sender::new(WIRE_EVENT_BUFFER),
            rows_changed: Arc::new(Notify::new()),
            queues_changed: Arc::new(Notify::new()),
            stopped: CancellationToken::new(),
            startup_notices: startup::list_recovered(&recoveries)
                .into_iter()
                .chain(not_private.map(|reason| startup::not_private(&notices_dir, &reason)))
                .collect(),
            set_aside_list: recoveries.iter().find_map(|r| match r {
                Recovery::Damaged { kept_at } => Some(kept_at.clone()),
                _ => None,
            }),
            #[cfg(test)]
            fake_engine: Mutex::new(None),
            #[cfg(test)]
            fake_mover: Mutex::new(None),
        });
        let manager = Manager { inner };
        manager.spawn_background_tasks();
        manager.reconcile(&mut manager.lock());
        Ok(manager)
    }

    /// What the person should be told about this start, such as a damaged download list that was set aside, or a data folder other accounts may read. Empty on a normal start.
    #[must_use]
    pub fn startup_notices(&self) -> &[StartupNotice] {
        &self.inner.startup_notices
    }

    /// Where a damaged download list was set aside at this start, for showing it in its folder. `None` when nothing was.
    #[must_use]
    pub fn set_aside_list(&self) -> Option<&Path> {
        self.inner.set_aside_list.as_deref()
    }

    /// Events for the desktop UI.
    ///
    /// Events only describe changes. A receiver that falls behind gets
    /// [`RecvError::Lagged`](broadcast::error::RecvError::Lagged) and has missed some;
    /// it must then reload the whole picture with [`list_downloads`](Self::list_downloads),
    /// [`list_queues`](Self::list_queues), [`get_settings`](Self::get_settings) and
    /// [`pending_confirmations`](Self::pending_confirmations) before applying further
    /// events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.inner.ui_events.subscribe()
    }

    /// Events for socket clients (the extension through the bridge, the CLI).
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.wire_events.subscribe()
    }

    /// Asks the desktop to show its main window ([`UiEvent::WindowRequested`]), for a client such as the extension's "Open Wirecatch".
    pub fn request_window(&self) {
        self.emit(UiEvent::WindowRequested);
    }

    /// Asks every connected browser to show `note` (D65) and says how many may answer: the
    /// connections listening for the app's events. None means no browser can show it.
    #[must_use = "no browser listening means the note must be told another way"]
    pub fn request_note(&self, note: BrowserNote) -> usize {
        let listening = self.inner.wire_events.receiver_count();
        self.emit_wire(Event::NoteRequested(note));
        listening
    }

    /// A browser showed the note `id` ([`UiEvent::NoteShown`]).
    pub fn note_shown(&self, id: NoteId) {
        self.emit(UiEvent::NoteShown(id));
    }

    /// The person pressed one of a browser note's buttons ([`UiEvent::NotePressed`]).
    pub fn note_pressed(&self, pressed: NotePressed) {
        self.emit(UiEvent::NotePressed(pressed));
    }

    /// Asks the desktop to show its main window on the browser settings ([`UiEvent::BrowserSettingsRequested`]), for the popup's settings button (D61).
    pub fn request_browser_settings(&self) {
        self.emit(UiEvent::BrowserSettingsRequested);
    }

    /// Stops every download so it can continue on the next start, saves everything,
    /// closes the database and stops the background work. Once it returns, the data
    /// folder can be deleted or moved (the database file is closed). Later operations
    /// fail with [`CoreError::Closed`].
    pub async fn shutdown(&self) {
        let exits: Vec<CancellationToken> = {
            let mut state = self.lock();
            state.closed = true;
            state
                .downloads
                .values_mut()
                .filter_map(|entry| {
                    let exited = entry.run.as_ref().map(|run| run.exited.clone());
                    self.stop_run(entry, Stop::Shutdown);
                    exited
                })
                .collect()
        };
        self.inner.stopped.cancel();
        for exited in exits {
            exited.cancelled().await;
        }
        self.save_batches_at_shutdown();
        self.inner.store.close(DATABASE_CLOSE_TIMEOUT).await;
    }

    fn spawn_background_tasks(&self) {
        let weak = Arc::downgrade(&self.inner);
        self.spawn(ticker::run(
            weak.clone(),
            self.inner.rows_changed.clone(),
            self.inner.stopped.clone(),
        ));
        self.spawn(run_scheduler(
            weak.clone(),
            self.inner.clock.clone(),
            self.inner.queues_changed.clone(),
            self.inner.stopped.clone(),
        ));
        self.spawn(sign_in::run_sign_in_expiry(
            weak.clone(),
            self.inner.clock.clone(),
            self.inner.stopped.clone(),
        ));
        self.spawn(Manager::run_addon_updates(
            weak.clone(),
            self.inner.stopped.clone(),
        ));
        self.spawn(Manager::run_suffix_list_updates(
            weak,
            self.inner.stopped.clone(),
        ));
    }

    /// Asks a running download to stop, and makes sure it does: a run still going
    /// after the stop timeout is abandoned, and its end is recorded as if it had
    /// stopped. Returns `false` if the download was not running.
    fn stop_run(&self, entry: &mut Entry, why: Stop) -> bool {
        let first_request = entry.run.as_ref().is_some_and(|run| run.stop.is_none());
        if !entry.stop(why) {
            return false;
        }
        if first_request && let Some(run) = &entry.run {
            let (exited, task) = (run.exited.clone(), run.task.clone());
            let timeout = self.inner.stop_timeout;
            let id = entry.row.id;
            self.spawn(async move {
                if tokio::time::timeout(timeout, exited.cancelled())
                    .await
                    .is_err()
                {
                    warn!(
                        id = id.0,
                        ?timeout,
                        "download did not stop in time; abandoning it"
                    );
                    task.abort();
                }
            });
        }
        true
    }

    /// Starts a task on the manager's runtime, whichever thread this is called from.
    fn spawn<F>(&self, task: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.inner.runtime.spawn(task)
    }

    /// Runs blocking work (disk access) on the manager's runtime and waits for it.
    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        blocking(&self.inner.runtime, work).await
    }

    fn from_weak(weak: &Weak<Inner>) -> Option<Self> {
        weak.upgrade().map(|inner| Manager { inner })
    }

    /// Locks the manager's state.
    ///
    /// A lock poisoned by a panic in another holder is taken over as it is.
    /// Nothing done under this lock is meant to panic, so poisoning means a bug
    /// that the panic itself has already reported. Wirecatch's release builds
    /// abort on panic (`panic = "abort"`), so there a poisoned lock cannot be
    /// seen at all; in debug and test builds the state is plain data, and
    /// refusing the lock as well would only add a second, less telling panic.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The state, unless the manager has shut down.
    fn lock_open(&self) -> Result<MutexGuard<'_, State>> {
        let state = self.lock();
        if state.closed {
            return Err(CoreError::Closed);
        }
        Ok(state)
    }

    /// Saves one download and queues its row for the next event.
    fn save(&self, state: &mut State, id: DownloadId) {
        if let Some(entry) = state.downloads.get(&id) {
            self.inner.store.save_downloads(vec![entry.stored()]);
            state.touch(id);
            self.inner.rows_changed.notify_one();
        }
    }

    fn emit(&self, event: UiEvent) {
        publish(&self.inner.ui_events, event);
    }

    fn emit_wire(&self, event: Event) {
        publish(&self.inner.wire_events, event);
    }
}

/// Sends `event` to every subscriber. With none, nobody is looking, so nobody
/// misses it.
fn publish<T>(events: &broadcast::Sender<T>, event: T) {
    let (Ok(_) | Err(broadcast::error::SendError(_))) = events.send(event);
}

/// Re-evaluates schedules whenever one can change: at each minute boundary while
/// any queue has a schedule, and whenever queues are edited.
async fn run_scheduler(
    weak: Weak<Inner>,
    clock: Arc<dyn Clock>,
    queues_changed: Arc<Notify>,
    stopped: CancellationToken,
) {
    loop {
        let Some(manager) = Manager::from_weak(&weak) else {
            return;
        };
        let wake_at = {
            let mut state = manager.lock();
            manager.reconcile(&mut state);
            let scheduled = state
                .queues
                .iter()
                .any(|s| s.queue.settings.schedule.is_some());
            scheduled.then(|| wirecatch_queues::schedule::next_minute(state.last_checked))
        };
        drop(manager);
        let minute = async {
            match wake_at {
                Some(at) => clock.sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = minute => {}
            () = queues_changed.notified() => {}
            () = stopped.cancelled() => return,
        }
    }
}
