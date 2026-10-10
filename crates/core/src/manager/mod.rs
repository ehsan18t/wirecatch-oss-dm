//! The download manager: owns every download, queue and setting, decides what runs,
//! and tells the UI and socket clients about changes.

mod captures;
mod downloads;
mod handovers;
mod plan;
mod preferences;
mod queues;
mod restore;
mod run;
mod state;
mod ticker;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::{Notify, broadcast};
use wirecatch_engine::{CancellationToken, RateLimiter};
use wirecatch_protocol::app::{DownloadStatus, UiEvent};
use wirecatch_protocol::wire::Event;
use wirecatch_protocol::{DownloadId, QueueId};

use crate::clock::{Clock, SystemClock};
use crate::error::{CoreError, Result, blocking};
use crate::folders::KnownFolders;
use crate::store::Store;
use state::{Entry, State, Stop};

/// How many UI events may wait for a slow subscriber before it starts missing some.
const UI_EVENT_BUFFER: usize = 1024;
/// The same for protocol events.
const WIRE_EVENT_BUFFER: usize = 64;

/// Where the manager keeps its data and how it reads the world around it.
#[derive(Clone)]
pub struct ManagerConfig {
    /// Folder for the database. The desktop app passes the per-user app data folder.
    pub data_dir: PathBuf,
    /// The user's standard folders, for resolving `{Downloads}` and friends.
    pub folders: KnownFolders,
    /// Source of the time of day for schedules and timestamps.
    pub clock: Arc<dyn Clock>,
}

impl ManagerConfig {
    /// A configuration using the real clock.
    pub fn new(data_dir: impl Into<PathBuf>, folders: KnownFolders) -> Self {
        Self {
            data_dir: data_dir.into(),
            folders,
            clock: Arc::new(SystemClock),
        }
    }

    /// Uses `clock` instead of the real clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
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
    state: Mutex<State>,
    store: Store,
    clock: Arc<dyn Clock>,
    folders: KnownFolders,
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
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}

impl Manager {
    /// Opens the manager on `config.data_dir`, restoring every download. Downloads
    /// that were running when the app last stopped continue on their own.
    pub async fn open(config: ManagerConfig) -> Result<Self> {
        let dir = config.data_dir.clone();
        let (store, snapshot) = blocking(move || Store::open(&dir)).await?;
        let restored = restore::restore(snapshot, config.clock.local_now());
        store.save_settings(restored.state.settings.clone());
        store.save_queues(restored.state.queue_list());
        store.save_downloads(restored.changed);

        let global_limiter = Arc::new(RateLimiter::new(restored.state.settings.global_speed_limit));
        let inner = Arc::new(Inner {
            state: Mutex::new(restored.state),
            store,
            clock: config.clock,
            folders: config.folders,
            global_limiter,
            ui_events: broadcast::Sender::new(UI_EVENT_BUFFER),
            wire_events: broadcast::Sender::new(WIRE_EVENT_BUFFER),
            rows_changed: Arc::new(Notify::new()),
            queues_changed: Arc::new(Notify::new()),
            stopped: CancellationToken::new(),
        });
        let manager = Manager { inner };
        manager.spawn_background_tasks();
        manager.reconcile(&mut manager.lock());
        Ok(manager)
    }

    /// Events for the desktop UI.
    pub fn subscribe(&self) -> broadcast::Receiver<UiEvent> {
        self.inner.ui_events.subscribe()
    }

    /// Events for socket clients (the extension through the bridge, the CLI).
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.wire_events.subscribe()
    }

    /// Stops every download so it can continue on the next start, saves everything
    /// and stops the background work. Later operations fail with [`CoreError::Closed`].
    pub async fn shutdown(&self) {
        let exits: Vec<CancellationToken> = {
            let mut state = self.lock();
            state.closed = true;
            state
                .downloads
                .values_mut()
                .filter_map(|entry| {
                    let exited = entry.run.as_ref().map(|run| run.exited.clone());
                    entry.stop(Stop::Shutdown);
                    exited
                })
                .collect()
        };
        self.inner.stopped.cancel();
        for exited in exits {
            exited.cancelled().await;
        }
        self.inner.store.flush().await;
    }

    fn spawn_background_tasks(&self) {
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(ticker::run(
            weak.clone(),
            self.inner.rows_changed.clone(),
            self.inner.stopped.clone(),
        ));
        tokio::spawn(run_scheduler(
            weak,
            self.inner.clock.clone(),
            self.inner.queues_changed.clone(),
            self.inner.stopped.clone(),
        ));
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

    /// Starts and stops downloads so every queue, schedule and limit is honoured.
    fn reconcile(&self, state: &mut State) {
        if state.closed {
            return;
        }
        let plan = self.plan(state);
        for id in plan.stop {
            if let Some(entry) = state.downloads.get_mut(&id) {
                entry.stop(Stop::Requeue);
            }
        }
        for id in plan.start {
            self.start_run(state, id);
        }
    }

    fn plan(&self, state: &mut State) -> plan::Plan {
        let now = self.inner.clock.local_now();
        let since = std::mem::replace(&mut state.last_checked, now);
        let downloads: Vec<plan::DownloadView> =
            state.downloads.values().filter_map(view_of).collect();
        let busy: HashSet<QueueId> = downloads.iter().map(|d| d.queue).collect();
        let queues: Vec<plan::QueueView> = state
            .queues
            .iter_mut()
            .map(|slot| plan::QueueView {
                id: slot.queue.id,
                max_concurrent: slot.queue.max_concurrent,
                open: slot.gate.is_open(
                    slot.queue.schedule.as_ref(),
                    since,
                    now,
                    busy.contains(&slot.queue.id),
                ),
            })
            .collect();
        plan::plan(&queues, &downloads, state.settings.max_concurrent_downloads)
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

/// How the planner sees a download; `None` for ones that neither run nor wait.
fn view_of(entry: &Entry) -> Option<plan::DownloadView> {
    let activity = match &entry.run {
        Some(run) if run.stop.is_some() => plan::Activity::Stopping,
        Some(_) => plan::Activity::Running,
        None if entry.row.status == DownloadStatus::Queued => plan::Activity::Waiting,
        None => return None,
    };
    Some(plan::DownloadView {
        id: entry.row.id,
        queue: entry.row.queue,
        position: entry.position,
        first: entry.resume_first,
        activity,
    })
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
