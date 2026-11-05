//! Rebuilding the manager's state from the database at startup.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDateTime;
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{AppSettings, DownloadStatus, Queue, Setting};
use wirecatch_protocol::proxy::ProxyUse;
use wirecatch_protocol::rules::QueueReason;
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_records::files::{MoveUnderWay, MovedFile, PendingDeletion};
use wirecatch_settings::DEFAULT_QUEUE_ID;
use wirecatch_settings::proxy::{ComputerProxy, SavedProxyServer};
use wirecatch_sites::local_network::ChosenBy;
use wirecatch_sites::public_suffix::SuffixList;
use wirecatch_store::{Snapshot, Store, StoredDownload};

use super::addons::AddonSlot;
use super::handovers::Handovers;
use super::questions;
use super::relocate::{discard_staged, files_that_arrived, finish_interrupted_move};
use super::run::mark_completed;
use super::state::{Counters, Entry, Hold, Partial, QueueSlot, State};
use super::suffix_list::SuffixCheck;
use crate::batches::Batches;
use crate::naming::Claimed;

/// The rebuilt state, plus downloads whose stored row needed fixing.
pub(super) struct Restored {
    pub state: State,
    pub changed: Vec<StoredDownload>,
    /// The settings differ from what was stored (a first start, or a number brought into
    /// range), so they need saving. Otherwise the stored settings stay as they are, with
    /// anything another build saved that this one cannot read.
    pub settings_changed: bool,
    /// Deletions on record were finished, so the record needs saving empty.
    pub deletions_finished: bool,
}

impl Restored {
    /// Saves what restoring changed.
    pub fn save_changes(&mut self, store: &Store) {
        if self.deletions_finished {
            store.save_pending_deletions(Vec::new());
        }
        // Only when changed, so what another build saved and this one cannot read (an accent added later, say) survives this build opening them.
        if self.settings_changed {
            store.save_settings(self.state.settings.clone());
        }
        store.save_queues(self.state.queue_list());
        store.save_downloads(std::mem::take(&mut self.changed));
    }
}

/// What restoring needs to know about the world outside the database.
pub(super) struct Surroundings<'a> {
    /// Local time, for schedules.
    pub now: NaiveDateTime,
    /// The moment, for timestamps.
    pub now_millis: UnixMillis,
    /// The size of the finished file at a path, if it is there with no part or
    /// resume file next to it.
    pub finished_size: &'a dyn Fn(&Path) -> Option<u64>,
    /// The Public Suffix List the stored settings are read with.
    pub suffixes: Arc<SuffixList>,
}

/// Rebuilds the state. A first start gets the default settings and queue. A
/// download that was running goes back in line ahead of the others, and a queue
/// that a schedule had started keeps running, so interrupted work continues. A
/// download whose file finished just before the app stopped (the file is there and
/// its partial files are gone) is marked completed instead of downloaded again.
pub(super) fn restore(snapshot: Snapshot, around: &Surroundings<'_>) -> Restored {
    let (settings, settings_changed) = match snapshot.settings {
        None => (wirecatch_settings::default_settings(), true),
        Some(stored) => {
            let kept = wirecatch_settings::within_limits(stored.clone(), &around.suffixes);
            let changed = kept != stored;
            (kept, changed)
        }
    };
    let mut queues = restore_queues(snapshot.queues);
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
    let (downloads, changed) = restore_downloads(
        snapshot.downloads,
        &mut queues,
        default_queue,
        &mut ids,
        around,
        &snapshot.pending_deletions,
    );

    let mut settings = settings;
    let profiles = snapshot.proxy_servers;
    // A password stays only with its profile; its id stays used, so no new profile takes it.
    ids.proxy_server = profiles
        .iter()
        .map(|p| p.id.0)
        .chain(snapshot.proxy_logins.keys().map(|id| id.0))
        .fold(snapshot.last_ids.proxy_server, i64::max);
    let mut proxy_logins = snapshot.proxy_logins;
    proxy_logins.retain(|id, _| profiles.iter().any(|p| p.id == *id));
    let settings_changed =
        forget_missing_profiles(&mut settings, &mut queues, &profiles) || settings_changed;
    let mut state = State {
        settings,
        queues,
        downloads: BTreeMap::new(),
        claimed: Claimed::default(),
        running: BTreeSet::default(),
        unfinished: BTreeSet::default(),
        arriving: Vec::new(),
        system_proxy: None,
        computer_proxy: ComputerProxy::Read,
        pending: questions::captures(),
        media: questions::videos(),
        file_exists: BTreeMap::new(),
        file_types: BTreeMap::new(),
        addon: AddonSlot::default(),
        suffixes: around.suffixes.clone(),
        suffix_check: SuffixCheck::default(),
        handovers: Handovers::default(),
        dirty: BTreeSet::default(),
        removals: Vec::new(),
        pending_deletions: BTreeMap::new(),
        batches: Batches::default(),
        proxy_logins,
        proxy_servers: profiles,
        last_checked: around.now,
        ids,
        closed: false,
    };
    for entry in downloads {
        state.insert_entry(entry);
    }
    state.batches = Batches::restore(state.downloads.values().map(|e| &e.row), snapshot.batches);
    finish_deletions(&state, &snapshot.pending_deletions);
    Restored {
        state,
        changed,
        settings_changed,
        deletions_finished: !snapshot.pending_deletions.is_empty(),
    }
}

/// The stored downloads as they continue, and the rows that need saving again.
fn restore_downloads(
    stored_rows: Vec<StoredDownload>,
    queues: &mut [QueueSlot],
    default_queue: QueueId,
    ids: &mut Counters,
    around: &Surroundings<'_>,
    deletions: &[PendingDeletion],
) -> (Vec<Entry>, Vec<StoredDownload>) {
    let mut downloads = Vec::with_capacity(stored_rows.len());
    let mut changed = Vec::new();
    let mut moved_files = Vec::new();
    for stored in stored_rows {
        let moved_file = moved_file_of(&stored);
        let (entry, fixed) = restore_entry(stored, queues, default_queue, around);
        moved_files.extend(moved_file.filter(|file| file.ended_at(&entry)));
        ids.download = ids.download.max(entry.row.id.0);
        ids.position = ids.position.max(entry.position);
        if fixed {
            changed.push(entry.stored());
        }
        downloads.push(entry);
    }
    for file in &moved_files {
        changed.extend(file.move_other_rows(&mut downloads));
    }
    changed.extend(follow_removed_moves(&mut downloads, deletions));
    (downloads, changed)
}

/// A finished row removed during its move took its record with it: the other rows of the file finish that move, as far as the disk tells (D179). Returns those that moved, as saved.
fn follow_removed_moves(
    downloads: &mut [Entry],
    deletions: &[PendingDeletion],
) -> Vec<StoredDownload> {
    let mut moved = Vec::new();
    for deletion in deletions.iter().filter(|deletion| deletion.finished) {
        let Some(path) = &deletion.moving_to else {
            continue;
        };
        let record = MoveUnderWay {
            path: path.clone(),
            files: deletion.moving_files.clone(),
        };
        for entry in downloads.iter_mut().filter(|e| {
            e.row.status == DownloadStatus::Completed
                && crate::naming::same_path(e.path(), &deletion.path)
        }) {
            finish_interrupted_move(entry, &record);
            if entry.path() == path.as_path() {
                moved.push(entry.stored());
            }
        }
    }
    moved
}

/// Deletes what a kill cut short (D134).
fn finish_deletions(state: &State, deletions: &[PendingDeletion]) {
    // A download at that path came back because a kill beat its row's deletion, and those files are its own, unless the row is the cancelled one the record is for.
    let spared = |path: &Path| {
        state
            .claimed
            .holder(path)
            .and_then(|holder| state.downloads.get(&holder))
            .is_some_and(|holder| holder.row.status != DownloadStatus::Cancelled)
    };
    for deletion in deletions {
        if let Some(target) = &deletion.moving_to {
            discard_staged(target.parent().unwrap_or(Path::new("")));
        }
        if deletion.finished || spared(&deletion.path) {
            continue;
        }
        Partial::of(deletion).delete();
        if let Some(target) = deletion.moving_to.as_ref().filter(|to| !spared(to)) {
            Partial::at(target, deletion.media).delete();
        }
    }
}

/// A finished file with a move on record, as a row saved it.
struct FileMoved {
    id: DownloadId,
    old_path: String,
    files: Vec<MovedFile>,
    new_path: PathBuf,
}

fn moved_file_of(stored: &StoredDownload) -> Option<FileMoved> {
    let record = stored.files.moving.as_ref()?;
    Some(FileMoved {
        id: stored.summary.id,
        old_path: stored.summary.path.clone(),
        files: record.files.clone(),
        new_path: record.path.clone(),
    })
}

impl FileMoved {
    /// Whether the move finished at this start, with the file now at its new path.
    fn ended_at(&self, entry: &Entry) -> bool {
        entry.row.status == DownloadStatus::Completed && entry.path() == self.new_path
    }

    /// The other finished rows of the file go with it (D118); returns them as saved.
    fn move_other_rows(&self, downloads: &mut [Entry]) -> Vec<StoredDownload> {
        let arrived = files_that_arrived(&self.files);
        downloads
            .iter_mut()
            .filter(|e| {
                e.row.id != self.id
                    && e.row.status == DownloadStatus::Completed
                    && e.row.path == self.old_path
            })
            .map(|e| {
                e.row.path = self.new_path.to_string_lossy().into_owned();
                e.row.file_name = crate::naming::file_name_of(&self.new_path);
                e.files.follow(&arrived);
                e.stored()
            })
            .collect()
    }
}

/// A profile that cannot be read was dropped with its settings, so what used it falls back:
/// the app to the computer's proxy, a queue to the app's. Says whether the app's changed.
fn forget_missing_profiles(
    settings: &mut AppSettings,
    queues: &mut [QueueSlot],
    profiles: &[SavedProxyServer],
) -> bool {
    let saved = |proxy: ProxyUse| wirecatch_settings::proxy::check_use(proxy, profiles).is_ok();
    for slot in queues {
        if slot.queue.settings.proxy.own().is_some_and(|p| !saved(p)) {
            slot.queue.settings.proxy = Setting::Global;
        }
    }
    if saved(settings.proxy.proxy) {
        return false;
    }
    settings.proxy.proxy = ProxyUse::System;
    true
}

/// The stored queues within today's limits, with the default queue first when
/// none of them is the default.
fn restore_queues(stored: Vec<Queue>) -> Vec<QueueSlot> {
    let mut queues: Vec<QueueSlot> = stored
        .into_iter()
        .map(|queue| {
            QueueSlot::new(Queue {
                settings: wirecatch_settings::queue_within_limits(queue.settings),
                ..queue
            })
        })
        .collect();
    if !queues.iter().any(|s| s.queue.is_default) {
        queues.insert(0, QueueSlot::new(wirecatch_settings::default_queue()));
    }
    queues
}

/// One stored download as it continues, and whether its stored row needed
/// fixing: one whose queue is gone joins `default_queue`, one whose file
/// finished is completed, and one that was running goes back in line first.
fn restore_entry(
    stored: StoredDownload,
    queues: &[QueueSlot],
    default_queue: QueueId,
    around: &Surroundings<'_>,
) -> (Entry, bool) {
    let media_job = stored.media.and_then(|m| m.job);
    let mut entry = Entry {
        // D51: a link the person added is still read for its video if its server has not answered with a file.
        video_check: media_job.is_none()
            && !stored.naming.settled
            && stored.chosen_by == ChosenBy::Person,
        media_job,
        row: stored.summary,
        position: stored.position,
        origin: stored.origin,
        chosen_by: stored.chosen_by,
        naming: stored.naming,
        expect: stored.answers.expect,
        run: None,
        resume_first: false,
        through_proxy: None,
        cleanup: None,
        hold: Hold::Free,
        on_existing: stored.answers.on_existing,
        plain_name: stored.answers.plain_name,
        confirmed_name: stored.answers.confirmed_name,
        next_run: stored.answers.next_run,
        offered_file: false,
        stranded: stored.stranded,
        files: stored.files,
    };
    if let Some(plain) = &entry.plain_name {
        entry.row.file_name.clone_from(plain);
    }
    let mut fixed = false;
    if let Some(record) = entry.files.moving.take() {
        finish_interrupted_move(&mut entry, &record);
        fixed = true;
    }
    if !queues.iter().any(|s| s.queue.id == entry.row.queue) {
        entry.row.queue = default_queue;
        entry.row.queue_reason = QueueReason::DefaultQueue;
        fixed = true;
    }
    let done_with = matches!(
        entry.row.status,
        DownloadStatus::Completed | DownloadStatus::Cancelled
    );
    // Until a run began writing, whatever is at the path is someone else's. One of another size than the server's is not this download's either.
    if !done_with
        && entry.stranded.is_none()
        && entry.naming.writing
        && let Some(size) = (around.finished_size)(entry.path())
        && entry.row.total.is_none_or(|total| total == size)
    {
        mark_completed(&mut entry, size, around.now_millis);
        let [_, resume_file, temp] = crate::naming::partial_files_and_temp(entry.path());
        wirecatch_files::discard::file(&resume_file);
        wirecatch_files::discard::file(&temp);
        fixed = true;
    }
    if matches!(
        entry.row.status,
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Processing
    ) {
        entry.row.status = DownloadStatus::Queued;
        entry.resume_first = true;
    }
    (entry, fixed)
}

#[cfg(test)]
pub(super) fn first_start() -> State {
    let surroundings = Surroundings {
        now: NaiveDateTime::default(),
        now_millis: UnixMillis(0),
        finished_size: &|_| None,
        suffixes: SuffixList::built_in(),
    };
    restore(Snapshot::default(), &surroundings).state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::Origin;
    use wirecatch_protocol::app::{DownloadSummary, Setting};
    use wirecatch_records::files::DownloadFiles;
    use wirecatch_sites::local_network::ChosenBy;
    use wirecatch_store::{LastIds, NameChoice};

    fn nowhere(finished_size: &dyn Fn(&Path) -> Option<u64>) -> Surroundings<'_> {
        Surroundings {
            now: NaiveDateTime::default(),
            now_millis: UnixMillis(7),
            finished_size,
            suffixes: SuffixList::built_in(),
        }
    }

    #[test]
    fn a_download_whose_file_finished_is_completed_not_rerun() {
        let writing = |id, status| {
            let mut row = stored(id, 1, status);
            row.naming.writing = true;
            row.summary.total = None;
            row
        };
        let snapshot = Snapshot {
            downloads: vec![
                writing(1, DownloadStatus::Downloading),
                writing(2, DownloadStatus::Cancelled),
                writing(3, DownloadStatus::Paused),
                stored(4, 1, DownloadStatus::Queued),
            ],
            ..Snapshot::default()
        };
        // Every path looks finished; only a download whose run began writing may be marked.
        let restored = restore(snapshot, &nowhere(&|_| Some(42)));
        let row = |id| &restored.state.downloads[&DownloadId(id)].row;
        for id in [1, 3] {
            assert_eq!(row(id).status, DownloadStatus::Completed);
            assert_eq!((row(id).done, row(id).total), (42, Some(42)));
            assert_eq!(row(id).completed_at, Some(UnixMillis(7)));
        }
        assert_eq!(row(2).status, DownloadStatus::Cancelled);
        assert_eq!(row(4).status, DownloadStatus::Queued);
        assert!(!restored.state.downloads[&DownloadId(1)].resume_first);
        assert_eq!(restored.changed.len(), 2);
    }

    #[test]
    fn a_file_of_another_size_than_the_servers_does_not_finish_a_download() {
        let mut row = stored(1, 1, DownloadStatus::Paused);
        row.naming.writing = true;
        row.summary.total = Some(42);
        let snapshot = Snapshot {
            downloads: vec![row],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| Some(5)));
        assert_eq!(
            restored.state.downloads[&DownloadId(1)].row.status,
            DownloadStatus::Paused
        );
    }

    /// The scheduler visits only unfinished downloads: a finished one is never in the set, and one that finishes leaves it at the next prune.
    #[test]
    fn only_unfinished_downloads_are_visited() {
        let snapshot = Snapshot {
            downloads: vec![
                stored(1, 1, DownloadStatus::Completed),
                stored(2, 1, DownloadStatus::Queued),
                stored(3, 1, DownloadStatus::Paused),
            ],
            ..Snapshot::default()
        };
        let mut state = restore(snapshot, &nowhere(&|_| None)).state;
        let visited = |state: &State| state.unfinished().map(|e| e.row.id.0).collect::<Vec<_>>();
        assert_eq!(visited(&state), [2, 3]);
        let finished = state.downloads.get_mut(&DownloadId(3)).unwrap();
        mark_completed(finished, 10, UnixMillis(8));
        assert_eq!(visited(&state), [2]);
        state.prune_finished();
        assert_eq!(
            state.unfinished.iter().map(|id| id.0).collect::<Vec<_>>(),
            [2]
        );
    }

    /// The delete after a cancel that a kill cut short, and the part file of `row`.
    fn cancelled_row(dir: &Path) -> (StoredDownload, PathBuf) {
        let path = dir.join("a.bin");
        let [part, _] = crate::naming::partial_files(&path);
        std::fs::write(&part, b"part").unwrap();
        let mut row = stored(1, 1, DownloadStatus::Cancelled);
        row.summary.path = path.to_string_lossy().into_owned();
        (row, part)
    }

    #[test]
    fn a_start_finishes_the_delete_a_kill_cut_short_after_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let (row, part) = cancelled_row(dir.path());
        let snapshot = Snapshot {
            pending_deletions: vec![PendingDeletion {
                path: PathBuf::from(&row.summary.path),
                media: false,
                moving_to: None,
                moving_files: Vec::new(),
                finished: false,
            }],
            downloads: vec![row],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        assert!(!part.exists(), "the part file stayed");
        assert!(restored.deletions_finished);
    }

    /// Another program, such as the command line, may have saved that name since.
    #[test]
    fn a_cancelled_download_with_no_record_keeps_a_part_file_it_did_not_leave() {
        let dir = tempfile::tempdir().unwrap();
        let (row, part) = cancelled_row(dir.path());
        let snapshot = Snapshot {
            downloads: vec![row],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        assert!(part.exists(), "someone else's part file was deleted");
        assert!(!restored.deletions_finished);
    }

    /// A download that came back holds its path: the record is from before the row left the list.
    #[test]
    fn a_deletion_on_record_spares_a_download_at_that_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let [part, _] = crate::naming::partial_files(&path);
        std::fs::write(&part, b"part").unwrap();
        let mut row = stored(1, 1, DownloadStatus::Paused);
        row.summary.path = path.to_string_lossy().into_owned();
        let snapshot = Snapshot {
            downloads: vec![row],
            pending_deletions: vec![PendingDeletion {
                path: path.clone(),
                media: false,
                moving_to: None,
                moving_files: Vec::new(),
                finished: false,
            }],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        assert!(part.exists(), "the download's own file was deleted");
        assert!(restored.deletions_finished);
    }

    /// A kill during a removal that waited on a move: the row is gone, and the record is all that names the target.
    #[test]
    fn a_start_clears_what_a_removals_move_left_in_its_target() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.bin"), target.path().join("a.bin"));
        let [old_part, _] = crate::naming::partial_files(&from);
        let [new_part, _] = crate::naming::partial_files(&to);
        let staged = target.path().join(".wirecatch-move-0");
        for file in [&old_part, &new_part, &staged] {
            std::fs::write(file, b"left").unwrap();
        }
        let snapshot = Snapshot {
            pending_deletions: vec![PendingDeletion {
                path: from,
                media: false,
                moving_to: Some(to),
                moving_files: Vec::new(),
                finished: false,
            }],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        assert!(!staged.exists(), "the copy under way stayed");
        assert!(!new_part.exists(), "the moved part file stayed");
        assert!(!old_part.exists(), "the part file stayed");
        assert!(restored.deletions_finished);
    }

    /// A finished file's move leaves only its copy to clear: a part file beside either path is someone else's.
    #[test]
    fn a_start_clears_only_the_copy_of_a_finished_files_move() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.bin"), target.path().join("a.bin"));
        let [old_part, _] = crate::naming::partial_files(&from);
        let [new_part, _] = crate::naming::partial_files(&to);
        let staged = target.path().join(".wirecatch-move-0");
        for file in [&from, &old_part, &new_part, &staged] {
            std::fs::write(file, b"left").unwrap();
        }
        let snapshot = Snapshot {
            pending_deletions: vec![PendingDeletion {
                path: from.clone(),
                media: false,
                moving_to: Some(to),
                moving_files: Vec::new(),
                finished: true,
            }],
            ..Snapshot::default()
        };
        restore(snapshot, &nowhere(&|_| None));
        assert!(!staged.exists(), "the copy under way stayed");
        assert!(from.exists() && old_part.exists() && new_part.exists());
    }

    /// The row that moved the file has gone with its record of the move: the other row of that file follows it, and its subtitle file with it.
    #[test]
    fn a_kill_during_a_removals_move_takes_the_files_other_row_along() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.bin"), target.path().join("a.bin"));
        let (old_srt, new_srt) = (source.path().join("a.srt"), target.path().join("a.srt"));
        std::fs::write(&to, b"the file").unwrap();
        std::fs::write(&old_srt, b"words").unwrap();
        let mut row = stored(2, 1, DownloadStatus::Completed);
        row.summary.path = from.to_string_lossy().into_owned();
        row.files.sidecars = vec![old_srt.clone()];
        let snapshot = Snapshot {
            downloads: vec![row],
            pending_deletions: vec![PendingDeletion {
                path: from.clone(),
                media: false,
                moving_to: Some(to.clone()),
                moving_files: vec![pair(&from, &to), pair(&old_srt, &new_srt)],
                finished: true,
            }],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        let entry = &restored.state.downloads[&DownloadId(2)];
        assert_eq!(entry.path(), to);
        assert_eq!(entry.files.sidecars, std::slice::from_ref(&new_srt));
        assert!(new_srt.exists() && !old_srt.exists());
        assert_eq!(restored.changed.len(), 1, "the row must be saved again");
    }

    /// The file never got there, so the other row of it stays where it is.
    #[test]
    fn a_kill_before_a_removals_move_leaves_the_files_other_row() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.bin"), target.path().join("a.bin"));
        std::fs::write(&from, b"the file").unwrap();
        let mut row = stored(2, 1, DownloadStatus::Completed);
        row.summary.path = from.to_string_lossy().into_owned();
        let snapshot = Snapshot {
            downloads: vec![row],
            pending_deletions: vec![PendingDeletion {
                path: from.clone(),
                media: false,
                moving_to: Some(to.clone()),
                moving_files: vec![pair(&from, &to)],
                finished: true,
            }],
            ..Snapshot::default()
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
        assert_eq!(restored.state.downloads[&DownloadId(2)].path(), from);
        assert_eq!(restored.changed.len(), 0);
    }

    /// A kill beat the row's deletion and the row came back: its files are its own, wherever its move got to.
    #[test]
    fn a_deletion_on_record_spares_a_download_that_came_back_from_its_move() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.bin"), target.path().join("a.bin"));
        let [old_part, _] = crate::naming::partial_files(&from);
        let [new_part, _] = crate::naming::partial_files(&to);
        let staged = target.path().join(".wirecatch-move-0");
        for file in [&old_part, &new_part, &staged] {
            std::fs::write(file, b"kept").unwrap();
        }
        let mut row = stored(1, 1, DownloadStatus::Paused);
        row.summary.path = from.to_string_lossy().into_owned();
        let snapshot = Snapshot {
            downloads: vec![row],
            pending_deletions: vec![PendingDeletion {
                path: from,
                media: false,
                moving_to: Some(to),
                moving_files: Vec::new(),
                finished: false,
            }],
            ..Snapshot::default()
        };
        restore(snapshot, &nowhere(&|_| None));
        assert!(
            old_part.exists() && new_part.exists(),
            "its file was deleted"
        );
        assert!(
            !staged.exists(),
            "no move runs at a start, so the copy is nobody's"
        );
    }

    /// The proxy a run is shown going through comes from the system's setting as last read, not read again under the manager's lock.
    #[test]
    fn a_run_shows_the_system_proxy_as_last_read() {
        let mut state = restore(Snapshot::default(), &nowhere(&|_| None)).state;
        let queue = state.default_queue();
        let mut read = wirecatch_engine::ManualProxies::default();
        read.all = Some("http://proxy.example:8080".into());
        state.system_proxy = Some(read);
        let (label, through) = state.proxy_route(queue, "https://example.com/f");
        assert_eq!(label, "System proxy (proxy.example:8080)");
        assert_eq!(through.as_deref(), Some(label.as_str()));
        state.system_proxy = None;
        assert_eq!(
            state.proxy_route(queue, "https://example.com/f").0,
            "No proxy"
        );
    }

    fn stored(id: i64, queue: i64, status: DownloadStatus) -> StoredDownload {
        StoredDownload {
            summary: DownloadSummary {
                queue: QueueId(queue),
                status,
                total: Some(10),
                done: 5,
                ..wirecatch_test_support::downloads::download(id)
            },
            position: id * 10,
            origin: Origin::default(),
            chosen_by: ChosenBy::Website,
            naming: NameChoice::settled(),
            media: None,
            stranded: None,
            answers: wirecatch_records::answers::Answers::default(),
            files: DownloadFiles::default(),
        }
    }

    /// The row `id`, with a record of a move of `files` to `to`.
    fn moving(
        status: DownloadStatus,
        from: &Path,
        to: &Path,
        files: Vec<MovedFile>,
    ) -> StoredDownload {
        let mut row = stored(1, 1, status);
        row.summary.path = from.to_string_lossy().into_owned();
        row.naming.writing = true;
        row.summary.total = None;
        row.files.moving = Some(MoveUnderWay {
            path: to.to_path_buf(),
            files,
        });
        row
    }

    fn pair(from: &Path, to: &Path) -> MovedFile {
        MovedFile {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        }
    }

    fn restored_with(rows: Vec<StoredDownload>) -> Restored {
        let snapshot = Snapshot {
            downloads: rows,
            ..Snapshot::default()
        };
        restore(snapshot, &nowhere(&crate::naming::finished_size))
    }

    /// Killed with the part file moved and the resume file not: another program's file at the old path must not pass for the finished download.
    #[test]
    fn a_kill_during_a_move_finishes_it_and_never_completes_on_a_file_left_at_the_old_path() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.bin"), dir.path().join("a (1).bin"));
        std::fs::write(&from, b"another program's").unwrap();
        let [old_part, old_state] = crate::naming::partial_files(&from);
        let [new_part, new_state] = crate::naming::partial_files(&to);
        std::fs::write(&new_part, b"part").unwrap();
        std::fs::write(&old_state, b"state").unwrap();

        let restored = restored_with(vec![moving(
            DownloadStatus::Downloading,
            &from,
            &to,
            vec![pair(&old_part, &new_part), pair(&old_state, &new_state)],
        )]);

        let entry = &restored.state.downloads[&DownloadId(1)];
        assert_eq!(Path::new(&entry.row.path), to);
        assert_eq!(entry.row.status, DownloadStatus::Queued);
        assert_eq!(std::fs::read(&new_state).unwrap(), b"state");
        assert!(!old_state.exists());
        assert_eq!(std::fs::read(&from).unwrap(), b"another program's");
        assert_eq!(entry.files.moving, None);
        assert_eq!(restored.changed.len(), 1);
        assert_eq!(restored.changed[0].files.moving, None);
    }

    #[test]
    fn a_kill_during_a_copy_keeps_the_download_and_deletes_the_copy() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let from = source.path().join("a.bin");
        let to = target.path().join("a.bin");
        std::fs::write(&from, b"whole").unwrap();
        let staged = target.path().join(".wirecatch-move-0");
        std::fs::write(&staged, b"wh").unwrap();

        let restored = restored_with(vec![moving(
            DownloadStatus::Completed,
            &from,
            &to,
            vec![pair(&from, &to)],
        )]);

        assert!(!staged.exists(), "the copy was left behind");
        assert_eq!(std::fs::read(&from).unwrap(), b"whole");
        let entry = &restored.state.downloads[&DownloadId(1)];
        assert_eq!(Path::new(&entry.row.path), from);
        assert_eq!(entry.files.moving, None);
    }

    /// Killed after the copy took its final name and before the original went, with the subtitles still to move.
    #[test]
    fn a_kill_after_a_copy_finishes_the_move_for_every_row_of_the_file() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.mp4"), target.path().join("a.mp4"));
        let (old_srt, new_srt) = (
            source.path().join("a.en.srt"),
            target.path().join("a.en.srt"),
        );
        std::fs::write(&from, b"video").unwrap();
        std::fs::write(&to, b"video").unwrap();
        std::fs::write(&old_srt, b"words").unwrap();
        let mut row = moving(
            DownloadStatus::Completed,
            &from,
            &to,
            vec![pair(&from, &to), pair(&old_srt, &new_srt)],
        );
        row.files.sidecars = vec![old_srt.clone()];
        let mut other = stored(2, 1, DownloadStatus::Completed);
        other.summary.path.clone_from(&row.summary.path);

        let restored = restored_with(vec![row, other]);

        assert!(!from.exists() && !old_srt.exists());
        assert_eq!(std::fs::read(&new_srt).unwrap(), b"words");
        let entry = &restored.state.downloads[&DownloadId(1)];
        assert_eq!(Path::new(&entry.row.path), to);
        assert_eq!(entry.files.sidecars, [new_srt]);
        assert_eq!(
            Path::new(&restored.state.downloads[&DownloadId(2)].row.path),
            to
        );
        assert_eq!(restored.changed.len(), 2);
    }

    /// Another program took the subtitle file's new name: "Remove and delete" must never reach that file through the list.
    #[test]
    fn a_subtitle_file_that_cannot_follow_stays_listed_where_it_is() {
        let (source, target) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (source.path().join("a.mp4"), target.path().join("a.mp4"));
        let (old_srt, new_srt) = (
            source.path().join("a.en.srt"),
            target.path().join("a.en.srt"),
        );
        std::fs::write(&to, b"video").unwrap();
        std::fs::write(&old_srt, b"words").unwrap();
        std::fs::write(&new_srt, b"another program's").unwrap();
        let mut row = moving(
            DownloadStatus::Completed,
            &from,
            &to,
            vec![pair(&from, &to), pair(&old_srt, &new_srt)],
        );
        row.files.sidecars = vec![old_srt.clone()];
        let mut other = stored(2, 1, DownloadStatus::Completed);
        other.summary.path.clone_from(&row.summary.path);
        other.files.sidecars = vec![old_srt.clone()];

        let restored = restored_with(vec![row, other]);

        for id in [1, 2] {
            let entry = &restored.state.downloads[&DownloadId(id)];
            assert_eq!(Path::new(&entry.row.path), to);
            assert_eq!(entry.files.sidecars, std::slice::from_ref(&old_srt));
        }
        assert_eq!(std::fs::read(&old_srt).unwrap(), b"words");
        assert_eq!(std::fs::read(&new_srt).unwrap(), b"another program's");
    }

    /// Another program took the resume file's new name during the outage: the download's files stay split and it says so.
    #[test]
    fn a_move_that_cannot_be_finished_leaves_the_download_failed_and_stranded() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.bin"), dir.path().join("a (1).bin"));
        std::fs::write(&from, b"another program's").unwrap();
        let [old_part, old_state] = crate::naming::partial_files(&from);
        let [new_part, new_state] = crate::naming::partial_files(&to);
        std::fs::write(&new_part, b"part").unwrap();
        std::fs::write(&old_state, b"state").unwrap();
        std::fs::write(&new_state, b"not this download's").unwrap();

        let restored = restored_with(vec![moving(
            DownloadStatus::Downloading,
            &from,
            &to,
            vec![pair(&old_part, &new_part), pair(&old_state, &new_state)],
        )]);

        let entry = &restored.state.downloads[&DownloadId(1)];
        assert_eq!(entry.row.status, DownloadStatus::Failed);
        assert_eq!(entry.stranded.as_deref(), Some(to.as_path()));
        assert_eq!(Path::new(&entry.row.path), from);
        assert!(old_state.exists());
        assert_eq!(std::fs::read(&new_state).unwrap(), b"not this download's");
    }

    #[test]
    fn first_start_gets_defaults() {
        let restored = restore(Snapshot::default(), &nowhere(&|_| None));

        assert_eq!(
            restored.state.queue_list(),
            vec![wirecatch_settings::default_queue()]
        );
        assert_eq!(
            restored.state.settings,
            wirecatch_settings::default_settings()
        );
        assert!(
            restored.settings_changed,
            "a first start saves its defaults"
        );
    }

    #[test]
    fn settings_already_in_range_are_not_saved_again() {
        let snapshot = Snapshot {
            settings: Some(wirecatch_settings::default_settings()),
            ..Snapshot::default()
        };
        assert!(!restore(snapshot, &nowhere(&|_| None)).settings_changed);
    }

    // Stored values skip the checks a change goes through. A stored 0 connections ran as one
    // until the engine began refusing it; then every download failed with advice to try again.
    #[test]
    fn stored_numbers_out_of_range_are_brought_into_range() {
        let mut stored_settings = wirecatch_settings::default_settings();
        stored_settings.connections_per_download = 0;
        stored_settings.max_concurrent_downloads = 0;
        stored_settings.global_speed_limit = Some(0);
        let mut queue = wirecatch_settings::default_queue();
        queue.settings.max_concurrent = Setting::Own(0);
        queue.settings.speed_limit = Setting::Own(0);
        let snapshot = Snapshot {
            settings: Some(stored_settings),
            queues: vec![queue],
            ..Snapshot::default()
        };

        let restored = restore(snapshot, &nowhere(&|_| None));

        let loaded = &restored.state.settings;
        assert!(restored.settings_changed, "brought into range, so saved");
        assert_eq!(loaded.connections_per_download, 1);
        assert_eq!(loaded.max_concurrent_downloads, 1);
        assert_eq!(loaded.global_speed_limit, None);
        let queue = &restored.state.queue_list()[0].settings;
        assert_eq!(
            (queue.max_concurrent, queue.speed_limit),
            (Setting::Own(1), Setting::Global)
        );
    }

    // An older version kept sites as typed, so a Unicode host never matched the punycode host
    // the browser reports for the same site.
    #[test]
    fn stored_ignored_sites_are_kept_the_way_the_list_keeps_them() {
        let mut stored_settings = wirecatch_settings::default_settings();
        stored_settings.extension.ignored_sites = vec![
            "bücher.de".into(),
            "https://Example.com/page".into(),
            "example.com".into(),
            "xn--bcher-kva.de".into(),
            "not a site".into(),
        ];
        let snapshot = Snapshot {
            settings: Some(stored_settings),
            queues: vec![wirecatch_settings::default_queue()],
            ..Snapshot::default()
        };

        let restored = restore(snapshot, &nowhere(&|_| None));

        assert_eq!(
            restored.state.settings.extension.ignored_sites,
            vec!["xn--bcher-kva.de", "example.com"]
        );
    }

    #[test]
    fn running_downloads_go_first_and_orphans_join_the_default_queue() {
        let snapshot = Snapshot {
            settings: None,
            last_ids: LastIds::default(),
            queues: vec![wirecatch_settings::default_queue()],
            downloads: vec![
                stored(1, 1, DownloadStatus::Queued),
                stored(2, 1, DownloadStatus::Downloading),
                stored(3, 9, DownloadStatus::Paused),
            ],
            batches: Vec::new(),
            pending_deletions: Vec::new(),
            proxy_logins: wirecatch_settings::proxy::Logins::new(),
            proxy_servers: Vec::new(),
            recoveries: Vec::new(),
            not_private: false,
        };
        let restored = restore(snapshot, &nowhere(&|_| None));
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
