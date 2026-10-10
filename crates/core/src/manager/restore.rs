//! Rebuilding the manager's state from the database at startup.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use wirecatch_protocol::app::DownloadStatus;

use super::state::{Counters, Entry, QueueSlot, State};
use crate::settings::{self, DEFAULT_QUEUE_ID};
use crate::store::{Snapshot, StoredDownload};

/// The rebuilt state, plus downloads whose stored row needed fixing.
pub(super) struct Restored {
    pub state: State,
    pub changed: Vec<StoredDownload>,
}

/// Rebuilds the state. A first start gets the default settings and queue. A
/// download that was running goes back in line ahead of the others, and a queue
/// that a schedule had started keeps running, so interrupted work continues.
pub(super) fn restore(snapshot: Snapshot, now: NaiveDateTime) -> Restored {
    let settings = snapshot.settings.unwrap_or_else(settings::default_settings);
    let mut queues: Vec<QueueSlot> = snapshot.queues.into_iter().map(QueueSlot::new).collect();
    if !queues.iter().any(|s| s.queue.is_default) {
        queues.insert(0, QueueSlot::new(settings::default_queue()));
    }
    let default_queue = queues
        .iter()
        .find(|s| s.queue.is_default)
        .map_or(DEFAULT_QUEUE_ID, |s| s.queue.id);

    let mut ids = Counters {
        queue: queues.iter().map(|s| s.queue.id.0).max().unwrap_or(0),
        ..Counters::default()
    };
    let mut downloads = BTreeMap::new();
    let mut changed = Vec::new();
    for stored in snapshot.downloads {
        let mut entry = Entry {
            row: stored.summary,
            position: stored.position,
            origin: stored.origin,
            run: None,
            resume_first: false,
            cleanup: None,
            removing: false,
        };
        let mut fixed = false;
        if !queues.iter().any(|s| s.queue.id == entry.row.queue) {
            entry.row.queue = default_queue;
            fixed = true;
        }
        if matches!(
            entry.row.status,
            DownloadStatus::Connecting | DownloadStatus::Downloading
        ) {
            entry.row.status = DownloadStatus::Queued;
            entry.resume_first = true;
            if let Some(slot) = queues.iter_mut().find(|s| s.queue.id == entry.row.queue) {
                slot.gate.resume_run();
            }
        }
        ids.download = ids.download.max(entry.row.id.0);
        ids.position = ids.position.max(entry.position);
        if fixed {
            changed.push(entry.stored());
        }
        downloads.insert(entry.row.id, entry);
    }

    Restored {
        state: State {
            settings,
            queues,
            downloads,
            pending: BTreeMap::new(),
            handovers: Default::default(),
            dirty: Default::default(),
            last_checked: now,
            ids,
            closed: false,
        },
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Origin;
    use wirecatch_protocol::app::{Category, DownloadSummary};
    use wirecatch_protocol::{DownloadId, QueueId};

    fn stored(id: i64, queue: i64, status: DownloadStatus) -> StoredDownload {
        StoredDownload {
            summary: DownloadSummary {
                id: DownloadId(id),
                url: "https://example.com/f".into(),
                file_name: "f".into(),
                path: "f".into(),
                queue: QueueId(queue),
                category: Category::Other,
                status,
                total: Some(10),
                done: 5,
                speed: 0,
                connections: 0,
                eta_secs: None,
                error: None,
                added_at: 0,
                completed_at: None,
                waiting: None,
            },
            position: id * 10,
            origin: Origin::default(),
        }
    }

    #[test]
    fn first_start_gets_defaults() {
        let now = NaiveDateTime::default();
        let restored = restore(Snapshot::default(), now);
        assert_eq!(restored.state.queue_list(), vec![settings::default_queue()]);
        assert_eq!(restored.state.settings, settings::default_settings());
    }

    #[test]
    fn running_downloads_go_first_and_orphans_join_the_default_queue() {
        let snapshot = Snapshot {
            settings: None,
            queues: vec![settings::default_queue()],
            downloads: vec![
                stored(1, 1, DownloadStatus::Queued),
                stored(2, 1, DownloadStatus::Downloading),
                stored(3, 9, DownloadStatus::Paused),
            ],
        };
        let restored = restore(snapshot, NaiveDateTime::default());
        let state = &restored.state;
        let running = &state.downloads[&DownloadId(2)];
        assert_eq!(running.row.status, DownloadStatus::Queued);
        assert!(running.resume_first);
        assert!(!state.downloads[&DownloadId(1)].resume_first);
        assert_eq!(state.downloads[&DownloadId(3)].row.queue, QueueId(1));
        assert_eq!(restored.changed.len(), 1);
        assert_eq!(
            (state.ids.download, state.ids.position, state.ids.queue),
            (3, 30, 1)
        );
    }
}
