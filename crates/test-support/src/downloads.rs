//! A download as the manager lists it, for tests that need one with a few fields set.

use wirecatch_protocol::app::{Category, DownloadStatus, DownloadSummary};
use wirecatch_protocol::rules::QueueReason;
use wirecatch_protocol::{DownloadId, QueueId};

/// Download `id`, queued in queue 1 as `file-{id}.zip`, with nothing downloaded yet.
#[must_use]
pub fn download(id: i64) -> DownloadSummary {
    DownloadSummary {
        id: DownloadId(id),
        url: format!("https://example.com/file-{id}.zip"),
        file_name: format!("file-{id}.zip"),
        path: format!("file-{id}.zip"),
        queue: QueueId(1),
        queue_reason: QueueReason::DefaultQueue,
        category: Category::Archive,
        status: DownloadStatus::Queued,
        total: None,
        done: 0,
        speed: 0,
        connections: 0,
        eta_secs: None,
        error: None,
        waiting: None,
        media: None,
        added_at: wirecatch_protocol::UnixMillis(0),
        completed_at: None,
        processing: None,
        proxy: None,
    }
}
