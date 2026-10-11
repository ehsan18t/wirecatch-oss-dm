//! The download manager: owns every download, queue and setting, decides what runs,
//! and tells the UI and socket clients about changes.

mod addons;
mod captures;
mod downloads;
#[cfg(test)]
mod fake;
mod handovers;
mod media;
mod media_run;
mod plan;
mod preferences;
mod queues;
mod relocate;
mod restore;
mod run;
mod scheduling;
mod settle;
mod state;
#[cfg(test)]
mod tests;
mod ticker;

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tracing::warn;
use wirecatch_engine::{CancellationToken, RateLimiter};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::UiEvent;
use wirecatch_protocol::wire::Event;

use crate::addons::manifest::MediaAddonSources;
use crate::clock::{Clock, SystemClock};
use crate::error::{CoreError, Result, blocking};
use crate::folders::KnownFolders;
use crate::naming;
use crate::store::Store;
use crate::user_error;
use state::{Entry, State, Stop};

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
}

/// The default [`ManagerConfig::capture_timeout`].
pub const DEFAULT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

/// The default [`ManagerConfig::stop_timeout`].
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The default [`ManagerConfig::media_timeout`].
pub const DEFAULT_MEDIA_TIMEOUT: Duration = Duration::from_secs(120);

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
        }
    }

    /// Installs and updates the media add-on from `sources` instead of the
    /// official releases.
    pub fn with_addon_sources(mut self, sources: MediaAddonSources) -> Self {
        self.addon_sources = sources;
        self
    }

    /// Uses `timeout` as the [`media_timeout`](Self::media_timeout).
    pub fn with_media_timeout(mut self, timeout: Duration) -> Self {
        self.media_timeout = timeout;
        self
    }

    /// Uses `clock` instead of the real clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Uses `timeout` as the [`capture_timeout`](Self::capture_timeout).
    pub fn with_capture_timeout(mut self, timeout: Duration) -> Self {
        self.capture_timeout = timeout;
        self
    }

    /// Uses `timeout` as the [`stop_timeout`](Self::stop_timeout).
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
    media_timeout: Duration,
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
    /// Plays the engine in unit tests.
    #[cfg(test)]
    fake_engine: Mutex<Option<fake::FakeEngine>>,
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
    pub async fn open(config: ManagerConfig) -> Result<Self> {
        let runtime = Handle::current();
        let ui_events = broadcast::Sender::new(UI_EVENT_BUFFER);
        let failures = ui_events.clone();
        let on_failure = Box::new(move |reason: String| {
            let _ = failures.send(UiEvent::StorageFailed(user_error::storage_failed(&reason)));
        });
        let dir = config.data_dir.clone();
        let addon_dir = crate::addons::media_dir(&config.data_dir);
        let temp_dir = config.data_dir.join("tmp");
        let (now, now_millis) = (config.clock.local_now(), config.clock.unix_millis());
        let (addons_at, temp_at) = (addon_dir.clone(), temp_dir.clone());
        // Restoring checks the disk for finished files, so it runs off async threads too.
        let (store, restored) = blocking(&runtime, move || {
            let (store, snapshot) = Store::open(&dir, on_failure)?;
            let around = restore::Surroundings {
                now,
                now_millis,
                finished_size: &naming::finished_size,
            };
            let mut restored = restore::restore(snapshot, around);
            restored.state.addon = addons::AddonSlot::load(&addons_at);
            // Cookies files left by a crash: nothing may keep them.
            let _ = std::fs::remove_dir_all(&temp_at);
            Ok((store, restored))
        })
        .await?;
        store.save_settings(restored.state.settings.clone());
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
            media_timeout: config.media_timeout,
            global_limiter,
            ui_events,
            wire_events: broadcast::Sender::new(WIRE_EVENT_BUFFER),
            rows_changed: Arc::new(Notify::new()),
            queues_changed: Arc::new(Notify::new()),
            stopped: CancellationToken::new(),
            #[cfg(test)]
            fake_engine: Mutex::new(None),
        });
        let manager = Manager { inner };
        manager.spawn_background_tasks();
        manager.reconcile(&mut manager.lock());
        Ok(manager)
    }

    /// Events for the desktop UI.
    ///
    /// Events only describe changes. A receiver that falls behind gets
    /// [`RecvError::Lagged`](broadcast::error::RecvError::Lagged) and has missed some;
    /// it must then reload the whole picture with [`list_downloads`](Self::list_downloads),
    /// [`list_queues`](Self::list_queues), [`get_settings`](Self::get_settings) and
    /// [`pending_confirmations`](Self::pending_confirmations) before applying further
    /// events.
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.inner.ui_events.subscribe()
    }

    /// Events for socket clients (the extension through the bridge, the CLI).
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.wire_events.subscribe()
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
        self.spawn(Manager::run_addon_updates(weak, self.inner.stopped.clone()));
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

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves plain data behind; keep going.
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
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
        // No subscriber is fine: nobody is looking.
        let _ = self.inner.ui_events.send(event);
    }

    fn emit_wire(&self, event: Event) {
        let _ = self.inner.wire_events.send(event);
    }
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
            let scheduled = state.queues.iter().any(|s| s.queue.schedule.is_some());
            scheduled.then(|| crate::schedule::next_minute(state.last_checked))
        };
        drop(manager);
        let minute = async {
            match wake_at {
                Some(at) => clock.sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = minute => {}
            _ = queues_changed.notified() => {}
            _ = stopped.cancelled() => return,
        }
    }
}
