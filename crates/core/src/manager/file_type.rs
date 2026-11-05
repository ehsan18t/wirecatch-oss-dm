//! A server's name that changes the file type of a name the person confirmed: the download pauses and asks which name to save it under (D166).

use wirecatch_protocol::app::{DownloadStatus, FileTypeChoice, FileTypeQuestion, UiEvent};
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use super::file_exists::give_up_the_name;
use super::run::mark_cancelled;
use super::state::State;
use crate::error::{CoreError, Result};

impl Manager {
    /// Every open [`FileTypeQuestion`], oldest first.
    #[must_use]
    pub fn pending_file_types(&self) -> Vec<FileTypeQuestion> {
        self.lock().file_types.values().cloned().collect()
    }

    /// Answers question `id`: the download goes back in line with the name chosen, or is cancelled. Returns the download.
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] for a question that is not open,
    /// [`CoreError::DownloadNotFound`] when its download is gone, and [`CoreError::Closed`] once
    /// the manager has shut down.
    pub fn answer_file_type(&self, id: QuestionId, choice: FileTypeChoice) -> Result<DownloadId> {
        let mut guard = self.lock_open()?;
        let state = &mut *guard;
        let question = state
            .file_types
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        self.emit(UiEvent::ConfirmationClosed { id });
        let download = question.download;
        let entry = state
            .downloads
            .get_mut(&download)
            .filter(|e| !e.hold.is_removing())
            .ok_or(CoreError::DownloadNotFound(download))?;
        // What it asked about has changed: a late answer must not act on the row as it is now.
        if !entry.hold.is_free()
            || entry.run.is_some()
            || entry.row.status != DownloadStatus::Paused
        {
            return Err(CoreError::ConfirmationNotFound(id));
        }
        match choice {
            FileTypeChoice::KeepConfirmed => go_ahead_as(entry, question.confirmed_name),
            FileTypeChoice::UseServers => go_ahead_as(entry, question.server_name),
            FileTypeChoice::Cancel => {
                mark_cancelled(entry);
                self.discard_partial(state, download);
                give_up_the_name(state, download);
            }
        }
        self.save(state, download);
        self.reconcile(state);
        Ok(download)
    }

    /// Pauses download `id` and asks about `server_name`, unless its confirmed name has gone since.
    pub(super) fn ask_about_file_type(
        &self,
        state: &mut State,
        id: DownloadId,
        server_name: String,
    ) {
        self.close_file_type(state, id);
        let question_id = state.ids.pending();
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let Some(confirmed_name) = entry.confirmed_name.clone() else {
            return;
        };
        entry.row.status = DownloadStatus::Paused;
        let question = FileTypeQuestion {
            id: question_id,
            download: id,
            url: wirecatch_logging::redact::url_for_display(&entry.row.url),
            confirmed_name,
            server_name,
        };
        state.file_types.insert(question.id, question.clone());
        self.emit(UiEvent::FileTypeRequested(question));
    }

    pub(super) fn close_file_type(&self, state: &mut State, id: DownloadId) {
        let closed: Vec<QuestionId> = state
            .file_types
            .values()
            .filter(|question| question.download == id)
            .map(|question| question.id)
            .collect();
        for question in closed {
            state.file_types.remove(&question);
            self.emit(UiEvent::ConfirmationClosed { id: question });
        }
    }
}

/// The person's answer is a choice like a name they typed: it wins over the server's from now on.
fn go_ahead_as(entry: &mut super::state::Entry, name: String) {
    entry.naming.name = Some(name);
    entry.confirmed_name = None;
    entry.row.status = DownloadStatus::Queued;
}
