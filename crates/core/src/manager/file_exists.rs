//! A download whose file name is already taken in its folder (D88, D96). When the server answers,
//! or when a stream or media download puts its file in place, a finished file at its path is
//! replaced, kept as the download, kept beside under the next free name, or asked about, as
//! `when_file_exists` says; the question says whether it looks like the same file.

use std::path::Path;
use std::time::UNIX_EPOCH;

use tracing::warn;
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{
    DownloadStatus, FileExistsAnswer, FileExistsChoice, FileExistsQuestion, UiEvent, UserError,
    WhenFileExists,
};
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use super::run::{mark_cancelled, mark_completed};
use super::settle::ServerAnswer;
use super::state::{Asked, Found, OnExisting, State};
use crate::error::{CoreError, Result};
use crate::naming;

impl Manager {
    /// Every open [`FileExistsQuestion`], oldest first.
    #[must_use]
    pub fn pending_file_exists(&self) -> Vec<FileExistsQuestion> {
        self.lock()
            .file_exists
            .values()
            .map(|asked| asked.question.clone())
            .collect()
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
        let Asked { question, path } = state
            .file_exists
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        self.emit(UiEvent::ConfirmationClosed { id });
        let download = question.download;
        state.claimed.release(&path, download);
        let entry = state
            .downloads
            .get(&download)
            .filter(|e| !e.hold.is_removing())
            .ok_or(CoreError::DownloadNotFound(download))?;
        // What it asked about has changed: a late answer must not act on the row as it is now.
        if !entry.hold.is_free()
            || entry.run.is_some()
            || entry.row.status != DownloadStatus::Paused
        {
            return Err(CoreError::ConfirmationNotFound(id));
        }
        let remembered = match answer.choice {
            FileExistsChoice::Replace => {
                replace_at(state, download, &path);
                WhenFileExists::Replace
            }
            FileExistsChoice::KeepBoth => {
                keep_both(state, download);
                WhenFileExists::KeepBoth
            }
            FileExistsChoice::Cancel => {
                if let Some(entry) = state.downloads.get_mut(&download) {
                    mark_cancelled(entry);
                }
                self.discard_partial(state, download);
                give_up_the_name(state, download);
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

    /// The finished file at `path`, where download `id` would save, unless the setting or the person
    /// already chose to keep both or another download holds the path. Touches the file system, so
    /// the lock is held only around it.
    pub(super) fn found_existing(
        &self,
        id: DownloadId,
        remote: &ServerAnswer,
        path: &Path,
    ) -> Option<Found> {
        let (then, history, url) = {
            let state = self.lock();
            let entry = state.downloads.get(&id)?;
            let then = state.settings.when_file_exists;
            let taken_by_another = state.claimed.held_by_another(path, id);
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
            (then, history, entry.row.url.clone())
        };
        let size = naming::file_size(path)?;
        // No download holds this path, so what lies beside the file is left over.
        naming::discard_partials_of_other_links(path, &url);
        let same_size = |total: Option<u64>| total.is_none_or(|total| total == size);
        let same_history = history.is_none_or(|(same_link, total)| same_link && same_size(total));
        Some(Found {
            path: path.to_path_buf(),
            size,
            saved_at: saved_at(path),
            then,
            same: same_size(remote.total) && same_history,
        })
    }

    /// Does what the setting says with the duplicate `found` of download `id`, whose run has
    /// just stopped for it. Another download that took the path since then keeps it, whatever
    /// the setting says.
    pub(super) fn on_found(&self, state: &mut State, id: DownloadId, found: &Found) {
        if state.claimed.held_by_another(&found.path, id) {
            keep_both(state, id);
            return;
        }
        match found.then {
            WhenFileExists::Replace => replace_at(state, id, &found.path),
            WhenFileExists::KeepExisting => keep_existing(state, id, found, self.now()),
            WhenFileExists::Ask => self.ask(state, id, found),
            // Never found: a keep-both setting takes the next free name before asking.
            WhenFileExists::KeepBoth => keep_both(state, id),
        }
    }

    /// Pauses download `id` and asks what to do about `found`. The path stays reserved for it until the question closes, so an answer of Replace never meets another download there.
    fn ask(&self, state: &mut State, id: DownloadId, found: &Found) {
        self.close_file_exists(state, id);
        if !state.claimed.reserve(&found.path, id) {
            keep_both(state, id);
            return;
        }
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        entry.row.status = DownloadStatus::Paused;
        let question = FileExistsQuestion {
            id: state.ids.pending(),
            download: id,
            url: wirecatch_logging::redact::url_for_display(&entry.row.url),
            file_name: naming::file_name_of(&found.path),
            folder: found
                .path
                .parent()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: found.size,
            saved_at: found.saved_at,
            same_file: found.same,
        };
        state.file_exists.insert(
            question.id,
            Asked {
                question: question.clone(),
                path: found.path.clone(),
            },
        );
        self.emit(UiEvent::FileExistsRequested(question));
    }

    /// Closes the open question of download `id`, if it has one: it was resumed, cancelled or
    /// removed instead of answered.
    pub(super) fn close_file_exists(&self, state: &mut State, id: DownloadId) {
        let closed: Vec<QuestionId> = state
            .file_exists
            .values()
            .filter(|asked| asked.question.download == id)
            .map(|asked| asked.question.id)
            .collect();
        for question in closed {
            if let Some(asked) = state.file_exists.remove(&question) {
                state.claimed.release(&asked.path, id);
            }
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

    fn now(&self) -> UnixMillis {
        self.inner.clock.unix_millis()
    }
}

/// Download `id` goes back in line and takes the next free name when the server answers.
fn keep_both(state: &mut State, id: DownloadId) {
    if let Some(entry) = state.downloads.get_mut(&id) {
        entry.on_existing = Some(OnExisting::KeepBoth);
        entry.row.status = DownloadStatus::Queued;
    }
}

/// Download `id`, cancelled before its name was final, stays in the list with nothing written: holding the name would keep the next download of that file from being asked, so it holds another. Tried again, it is judged by the plain name (D96).
pub(super) fn give_up_the_name(state: &mut State, id: DownloadId) {
    let Some(entry) = state.downloads.get(&id) else {
        return;
    };
    if entry.naming.settled {
        return;
    }
    let path = entry.path().to_path_buf();
    let dir = path.parent().unwrap_or(Path::new(""));
    let plain = entry
        .plain_name
        .clone()
        .unwrap_or_else(|| naming::file_name_of(&path));
    let plain_path = dir.join(&plain);
    let free = naming::unclaimed_path(dir, &plain, |taken| {
        state.claimed.contains(taken) || naming::same_path(taken, &plain_path)
    });
    state.claimed.remove(&path, id);
    state.claimed.insert(&free, id);
    if let Some(entry) = state.downloads.get_mut(&id) {
        entry.row.path = free.to_string_lossy().into_owned();
        entry.plain_name = Some(plain);
    }
}

/// Download `id` runs again at `path`, over the file there.
fn replace_at(state: &mut State, id: DownloadId, path: &Path) {
    let Some(entry) = state.downloads.get_mut(&id) else {
        return;
    };
    entry.on_existing = Some(OnExisting::Replace);
    entry.row.status = DownloadStatus::Queued;
    // A media download's work folder is named after its path, and holds the file that waits: moving the path would leave both behind. Placing that file replaces the one found.
    if entry.media_job.is_some() {
        return;
    }
    let provisional = entry.path().to_path_buf();
    settle_at(entry, path);
    if !naming::same_path(&provisional, path) {
        state.claimed.remove(&provisional, id);
        state.claimed.insert(path, id);
    }
}

/// Download `id` is the file `found` already in the folder: completed, with nothing downloaded.
fn keep_existing(state: &mut State, id: DownloadId, found: &Found, now: UnixMillis) {
    let Some(entry) = state.downloads.get_mut(&id) else {
        return;
    };
    let provisional = entry.path().to_path_buf();
    settle_at(entry, &found.path);
    mark_completed(entry, found.size, now);
    state.claimed.remove(&provisional, id);
}

fn settle_at(entry: &mut super::state::Entry, path: &Path) {
    entry.row.path = path.to_string_lossy().into_owned();
    entry.row.file_name = naming::file_name_of(path);
    entry.naming.settled = true;
    entry.plain_name = None;
    entry.expect = None;
    entry.video_check = false;
}

/// When the file at `path` was last changed.
pub(super) fn saved_at(path: &Path) -> Option<UnixMillis> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    i64::try_from(modified.duration_since(UNIX_EPOCH).ok()?.as_millis())
        .ok()
        .map(UnixMillis)
}
