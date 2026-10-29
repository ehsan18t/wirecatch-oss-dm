//! A download whose file is already in its folder (D88). When the server answers, a finished file
//! at the download's path is compared with what the server offers; one that only shares the name
//! leaves the download to the next free name, and a duplicate is replaced, kept as the download,
//! or asked about, as `when_file_exists` says.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tracing::warn;
use wirecatch_protocol::app::{
    DownloadStatus, FileExistsAnswer, FileExistsChoice, FileExistsQuestion, UiEvent, UserError,
    WhenFileExists,
};
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use super::run::{mark_cancelled, mark_completed};
use super::settle::ServerAnswer;
use super::state::{Found, OnExisting, State};
use crate::error::{CoreError, Result};
use crate::naming;

impl Manager {
    /// Every open [`FileExistsQuestion`], oldest first.
    #[must_use]
    pub fn pending_file_exists(&self) -> Vec<FileExistsQuestion> {
        self.lock().file_exists.values().cloned().collect()
    }

    /// Answers question `id`: the download replaces the file in the folder, takes the next free
    /// name, or is cancelled. A remembered Replace or Keep both becomes the setting. Returns the
    /// download.
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] for a question that is not open,
    /// [`CoreError::DownloadNotFound`] when its download is gone, and [`CoreError::Closed`] once
    /// the manager has shut down.
    pub fn answer_file_exists(
        &self,
        id: QuestionId,
        answer: FileExistsAnswer,
    ) -> Result<DownloadId> {
        let mut guard = self.lock_open()?;
        let state = &mut *guard;
        let question = state
            .file_exists
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        self.emit(UiEvent::ConfirmationClosed { id });
        let download = question.download;
        if !state
            .downloads
            .get(&download)
            .is_some_and(|e| !e.removing && e.run.is_none())
        {
            return Err(CoreError::DownloadNotFound(download));
        }
        let remembered = match answer.choice {
            FileExistsChoice::Replace => {
                replace_at(state, download, &question_path(&question));
                WhenFileExists::Replace
            }
            FileExistsChoice::KeepBoth => {
                if let Some(entry) = state.downloads.get_mut(&download) {
                    entry.on_existing = Some(OnExisting::KeepBoth);
                    entry.row.status = DownloadStatus::Queued;
                }
                WhenFileExists::KeepBoth
            }
            FileExistsChoice::Cancel => {
                if let Some(entry) = state.downloads.get_mut(&download) {
                    mark_cancelled(entry);
                    self.discard_partial(entry);
                }
                WhenFileExists::Ask
            }
        };
        if answer.remember && answer.choice != FileExistsChoice::Cancel {
            let mut settings = state.settings.clone();
            settings.when_file_exists = remembered;
            self.apply_settings(state, settings);
        }
        self.save(state, download);
        self.reconcile(state);
        Ok(download)
    }

    /// The file at `path`, where download `id` would save, when it seems to be
    /// the file the server offers and the setting or the person has not already chosen to keep
    /// both. Touches the file system, so the lock is held only around it.
    pub(super) fn found_duplicate(
        &self,
        id: DownloadId,
        remote: &ServerAnswer,
        path: &Path,
        provisional: &Path,
    ) -> Option<Found> {
        let (then, history) = {
            let state = self.lock();
            let entry = state.downloads.get(&id)?;
            let then = state.settings.when_file_exists;
            let taken_by_another =
                !naming::same_path(path, provisional) && state.claimed.contains(path);
            if then == WhenFileExists::KeepBoth
                || entry.on_existing == Some(OnExisting::KeepBoth)
                || taken_by_another
            {
                return None;
            }
            // A finished download in the list says where the file came from.
            let history = state
                .downloads
                .values()
                .find(|other| {
                    other.row.id != id
                        && other.row.status == DownloadStatus::Completed
                        && naming::same_path(other.path(), path)
                })
                .map(|other| (other.row.url == entry.row.url, other.row.total));
            (then, history)
        };
        let size = naming::finished_size(path)?;
        let same_size = |total: Option<u64>| total.is_none_or(|total| total == size);
        let same_history = history.is_none_or(|(same_link, total)| same_link && same_size(total));
        if !(same_size(remote.total) && same_history) {
            return None;
        }
        Some(Found {
            path: path.to_path_buf(),
            size,
            saved_at: saved_at(path),
            then,
        })
    }

    /// Does what the setting says with the duplicate `found` of download `id`, whose run has
    /// just stopped for it.
    pub(super) fn on_found(&self, state: &mut State, id: DownloadId, found: &Found) {
        match found.then {
            WhenFileExists::Replace => replace_at(state, id, &found.path),
            WhenFileExists::KeepExisting => keep_existing(state, id, found, self.now()),
            WhenFileExists::Ask => self.ask(state, id, found),
            // Never found: a keep-both setting takes the next free name before asking.
            WhenFileExists::KeepBoth => {
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.on_existing = Some(OnExisting::KeepBoth);
                    entry.row.status = DownloadStatus::Queued;
                }
            }
        }
    }

    /// Pauses download `id` and asks what to do about `found`.
    fn ask(&self, state: &mut State, id: DownloadId, found: &Found) {
        self.close_file_exists(state, id);
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        entry.row.status = DownloadStatus::Paused;
        let question = FileExistsQuestion {
            id: state.ids.pending(),
            download: id,
            url: entry.row.url.clone(),
            file_name: naming::file_name_of(&found.path),
            folder: found
                .path
                .parent()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: found.size,
            saved_at: found.saved_at,
        };
        state.file_exists.insert(question.id, question.clone());
        self.emit(UiEvent::FileExistsRequested(question));
    }

    /// Closes the open question of download `id`, if it has one: it was resumed, cancelled or
    /// removed instead of answered.
    pub(super) fn close_file_exists(&self, state: &mut State, id: DownloadId) {
        let closed: Vec<QuestionId> = state
            .file_exists
            .values()
            .filter(|q| q.download == id)
            .map(|q| q.id)
            .collect();
        for question in closed {
            state.file_exists.remove(&question);
            self.emit(UiEvent::ConfirmationClosed { id: question });
        }
    }

    /// A file another program saved under download `id`'s name after its name was chosen holds
    /// the path, and the engine kept that file (ENG-11): the finished part and resume files move
    /// to the next free name in the folder, and the download runs again there to finish. It
    /// waits, queued, while `moving` is set.
    pub(super) fn take_next_free_name(&self, id: DownloadId) {
        let manager = self.clone();
        self.spawn(async move {
            let Some(old) = manager
                .lock()
                .downloads
                .get(&id)
                .map(|e| e.path().to_path_buf())
            else {
                return;
            };
            let dir = old.parent().map(Path::to_path_buf).unwrap_or_default();
            let moved = manager.move_files(id, &old, dir.clone()).await;
            if let Err(error) = manager.finish_move(id, &dir, &old, moved) {
                warn!(id = id.0, %error, "could not move a finished download off a name taken meanwhile");
                let mut state = manager.lock();
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.row.status = DownloadStatus::Failed;
                    entry.row.error = Some(UserError {
                        message: error.to_string(),
                        fix: None,
                    });
                }
                manager.save(&mut state, id);
                manager.reconcile(&mut state);
            }
        });
    }

    fn now(&self) -> i64 {
        self.inner.clock.unix_millis()
    }
}

/// Download `id` runs again at `path`, over the file there.
fn replace_at(state: &mut State, id: DownloadId, path: &Path) {
    let category = wirecatch_settings::category_of(&state.settings, &naming::file_name_of(path));
    let Some(entry) = state.downloads.get_mut(&id) else {
        return;
    };
    let provisional = entry.path().to_path_buf();
    settle_at(entry, path);
    entry.row.category = category;
    entry.on_existing = Some(OnExisting::Replace);
    entry.row.status = DownloadStatus::Queued;
    if !naming::same_path(&provisional, path) {
        state.claimed.remove(&provisional);
        state.claimed.insert(path);
    }
}

/// Download `id` is the file `found` already in the folder: completed, with nothing downloaded.
fn keep_existing(state: &mut State, id: DownloadId, found: &Found, now: i64) {
    let Some(entry) = state.downloads.get_mut(&id) else {
        return;
    };
    let provisional = entry.path().to_path_buf();
    settle_at(entry, &found.path);
    mark_completed(entry, found.size, now);
    state.claimed.remove(&provisional);
}

fn settle_at(entry: &mut super::state::Entry, path: &Path) {
    entry.row.path = path.to_string_lossy().into_owned();
    entry.row.file_name = naming::file_name_of(path);
    entry.naming.settled = true;
    entry.expect = None;
    entry.video_check = false;
}

/// When the file at `path` was last changed, in Unix milliseconds.
fn saved_at(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    i64::try_from(modified.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()
}

/// The path a question's answer is about.
fn question_path(question: &FileExistsQuestion) -> PathBuf {
    Path::new(&question.folder).join(&question.file_name)
}
