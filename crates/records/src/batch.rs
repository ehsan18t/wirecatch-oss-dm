use serde::{Deserialize, Serialize};
use wirecatch_protocol::{DownloadId, QueueId};

/// What a queue's downloads ended as since the queue last started working, each list in the
/// order they ended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcomes {
    pub completed: Vec<DownloadId>,
    pub failed: Vec<DownloadId>,
}

impl Outcomes {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.completed.is_empty() && self.failed.is_empty()
    }

    /// Takes `id` out of both lists. Returns whether it was in one.
    pub fn drop_id(&mut self, id: DownloadId) -> bool {
        let before = self.completed.len() + self.failed.len();
        self.completed.retain(|d| *d != id);
        self.failed.retain(|d| *d != id);
        before != self.completed.len() + self.failed.len()
    }
}

/// A queue's batch as it is saved, so the downloads that ended before a restart still count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredBatch {
    pub queue: QueueId,
    #[serde(flatten)]
    pub outcomes: Outcomes,
}
