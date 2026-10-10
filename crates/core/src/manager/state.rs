//! The manager's in-memory state. Everything here is guarded by one mutex that is
//! never held across an `.await`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::NaiveDateTime;
use wirecatch_engine::{CancellationToken, Progress, RateLimiter};
use wirecatch_protocol::app::{
    AppSettings, DownloadStatus, DownloadSummary, PendingCapture, Queue,
};
use wirecatch_protocol::extension::NewDownload;
use wirecatch_protocol::wire::QueueSummary;
use wirecatch_protocol::{DownloadId, QueueId};

use crate::error::{CoreError, Result};
use crate::naming::Claimed;
use crate::schedule::Gate;
use crate::speed::SpeedMeter;
use crate::store::{Origin, StoredDownload};

pub(super) struct State {
    pub settings: AppSettings,
    /// In sidebar order.
    pub queues: Vec<QueueSlot>,
    pub downloads: BTreeMap<DownloadId, Entry>,
    pub pending: BTreeMap<u64, Pending>,
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
    /// Present while the engine works on it.
    pub run: Option<Run>,
    /// Was running when the app last stopped, so it gets the first free slot.
    pub resume_first: bool,
}

/// An engine run in progress.
pub(super) struct Run {
    pub id: u64,
    pub cancel: CancellationToken,
    /// Cancelled by the run's task once the engine has returned and closed its files.
    pub exited: CancellationToken,
    pub progress: Arc<Progress>,
    /// Why the run is being stopped, once it is.
    pub stop: Option<Stop>,
    pub meter: SpeedMeter,
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
    /// It is being removed from the list.
    Remove,
    /// The app is closing; it continues on the next start.
    Shutdown,
}

/// A browser capture waiting for the user's confirmation.
pub(super) struct Pending {
    pub capture: PendingCapture,
    pub request: NewDownload,
}

impl State {
    pub fn entry(&self, id: DownloadId) -> Result<&Entry> {
        self.downloads
            .get(&id)
            .ok_or(CoreError::DownloadNotFound(id))
    }

    pub fn entry_mut(&mut self, id: DownloadId) -> Result<&mut Entry> {
        self.downloads
            .get_mut(&id)
            .ok_or(CoreError::DownloadNotFound(id))
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

    /// Paths promised to downloads that have not finished.
    pub fn claimed_paths(&self) -> Claimed {
        let mut claimed = Claimed::default();
        for entry in self.downloads.values() {
            if entry.row.status != DownloadStatus::Completed {
                claimed.insert(entry.path());
            }
        }
        claimed
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
    pub fn path(&self) -> &std::path::Path {
        std::path::Path::new(&self.row.path)
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn stored(&self) -> StoredDownload {
        StoredDownload {
            summary: self.row.clone(),
            position: self.position,
            origin: self.origin.clone(),
        }
    }

    /// Asks a running download to stop; the run's end decides its new status.
    /// Returns `false` if it was not running.
    pub fn stop(&mut self, why: Stop) -> bool {
        match &mut self.run {
            Some(run) => {
                // A stronger reason replaces a weaker one: a download the scheduler was
                // already stopping can still be paused, cancelled or removed.
                if run
                    .stop
                    .is_none_or(|current| why != Stop::Requeue || current == Stop::Requeue)
                {
                    run.stop = Some(why);
                }
                run.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Clears what only makes sense while running.
    pub fn clear_live_fields(&mut self) {
        self.row.speed = 0;
        self.row.connections = 0;
        self.row.eta_secs = None;
    }
}
