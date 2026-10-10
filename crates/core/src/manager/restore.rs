//! Rebuilding the manager's state from the database at startup.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDateTime;
use wirecatch_protocol::app::DownloadStatus;

use super::state::{Counters, Entry, QueueSlot, State};
use crate::settings::{self, DEFAULT_QUEUE_ID};
use crate::store::{Origin, Snapshot, StoredDownload};

fn mark_completed(entry: &mut Entry, size: u64, now_millis: i64) {
    entry.row.status = DownloadStatus::Completed;
    entry.row.total = Some(size);
    entry.row.done = size;
    entry.row.completed_at = Some(now_millis);
    entry.row.error = None;
    entry.origin = Origin::default();
}

/// The rebuilt state, plus downloads whose stored row needed fixing.
pub(super) struct Restored {
    pub state: State,
    pub changed: Vec<StoredDownload>,
}

/// What restoring needs to know about the world outside the database.
pub(super) struct Surroundings<'a> {
    /// Local time, for schedules.
    pub now: NaiveDateTime,
    /// Unix milliseconds, for timestamps.
    pub now_millis: i64,
    /// The size of the finished file at a path, if it is there with no part or
    /// resume file next to it.
    pub finished_size: &'a dyn Fn(&Path) -> Option<u64>,
}

/// Rebuilds the state. A first start gets the default settings and queue. A
/// download that was running goes back in line ahead of the others, and a queue
/// that a schedule had started keeps running, so interrupted work continues. A
/// download whose file finished just before the app stopped (the file is there and
/// its partial files are gone) is marked completed instead of downloaded again.
pub(super) fn restore(snapshot: Snapshot, around: Surroundings<'_>) -> Restored {
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
        download: snapshot.last_ids.download,
        queue: queues
            .iter()
            .map(|s| s.queue.id.0)
            .fold(snapshot.last_ids.queue, i64::max),
        ..Counters::default()
    };
    let mut downloads = Vec::with_capacity(snapshot.downloads.len());
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
            moving: None,
        };
        let mut fixed = false;
        if !queues.iter().any(|s| s.queue.id == entry.row.queue) {
            entry.row.queue = default_queue;
            fixed = true;
        }
        let settled = matches!(
            entry.row.status,
            DownloadStatus::Completed | DownloadStatus::Cancelled
        );
        if !settled && let Some(size) = (around.finished_size)(entry.path()) {
            mark_completed(&mut entry, size, around.now_millis);
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
        downloads.push(entry);
    }

    let mut state = State {
        settings,
        queues,
        downloads: BTreeMap::new(),
        claimed: Default::default(),
        running: Default::default(),
        pending: BTreeMap::new(),
        handovers: Default::default(),
        dirty: Default::default(),
        last_checked: around.now,
        ids,
        closed: false,
    };
    for entry in downloads {
        state.insert_entry(entry);
    }
    Restored { state, changed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::{Category, DownloadSummary};
    use wirecatch_protocol::{DownloadId, QueueId};

    fn nowhere(finished_size: &dyn Fn(&Path) -> Option<u64>) -> Surroundings<'_> {
        Surroundings {
            now: NaiveDateTime::default(),
            now_millis: 7,
            finished_size,
        }
    }

    #[test]
    fn a_download_whose_file_finished_is_completed_not_rerun() {
        let snapshot = Snapshot {
            downloads: vec![
                stored(1, 1, DownloadStatus::Downloading),
                stored(2, 1, DownloadStatus::Cancelled),
                stored(3, 1, DownloadStatus::Paused),
            ],
            ..Snapshot::default()
        };
        // Every path looks finished; only unsettled downloads may be marked.
        let restored = restore(snapshot, nowhere(&|_| Some(42)));
        let row = |id| &restored.state.downloads[&DownloadId(id)].row;
        for id in [1, 3] {
            assert_eq!(row(id).status, DownloadStatus::Completed);
            assert_eq!((row(id).done, row(id).total), (42, Some(42)));
            assert_eq!(row(id).completed_at, Some(7));
        }
        assert_eq!(row(2).status, DownloadStatus::Cancelled);
        assert!(!restored.state.downloads[&DownloadId(1)].resume_first);
        assert_eq!(restored.changed.len(), 2);
    }

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
        let restored = restore(Snapshot::default(), nowhere(&|_| None));

        assert_eq!(restored.state.queue_list(), vec![settings::default_queue()]);
        assert_eq!(restored.state.settings, settings::default_settings());
    }

    #[test]
    fn running_downloads_go_first_and_orphans_join_the_default_queue() {
        let snapshot = Snapshot {
            settings: None,
            last_ids: Default::default(),
            queues: vec![settings::default_queue()],
            downloads: vec![
                stored(1, 1, DownloadStatus::Queued),
                stored(2, 1, DownloadStatus::Downloading),
                stored(3, 9, DownloadStatus::Paused),
            ],
        };
        let restored = restore(snapshot, nowhere(&|_| None));
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
