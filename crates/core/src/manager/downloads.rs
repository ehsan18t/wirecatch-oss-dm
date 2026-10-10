//! Adding downloads and moving them through their life: pause, resume, cancel,
//! retry, refresh link, remove, and their place in queues.

use std::path::PathBuf;

use wirecatch_engine::Url;
use wirecatch_protocol::app::{AddRequest, DownloadStatus, DownloadSummary, PortablePath, UiEvent};
use wirecatch_protocol::extension::{AddAction, AddedDownload};
use wirecatch_protocol::{DownloadId, QueueId};

use super::Manager;
use super::handovers::{Handover, Target};
use super::run::{delete_files, mark_cancelled};
use super::state::{Entry, Stop};
use crate::error::{CoreError, Result};
use crate::naming::{self, partial_files};
use crate::settings;
use crate::store::Origin;

/// Everything needed to add a download, whichever way it arrived.
pub(super) struct NewEntry {
    pub url: Url,
    /// Preferred file name; the link's name is used if absent.
    pub file_name: Option<String>,
    /// Folder to save in; the category rule decides if absent.
    pub folder: Option<PathBuf>,
    pub queue: QueueId,
    /// `false` adds it paused.
    pub start: bool,
    pub origin: Origin,
    /// The extension's handover id, for captures.
    pub handover: Option<String>,
}

/// What [`Manager::insert`] did.
pub(super) enum Added {
    /// A new download.
    New(DownloadSummary),
    /// The handover was already known; this is the answer given then.
    Known(AddedDownload),
}

/// How a captured download stands, for the extension. Until the engine reports that
/// the server has answered, a running download counts as started.
pub(super) fn capture_action(row: &DownloadSummary) -> AddAction {
    match row.status {
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Completed => {
            AddAction::Started
        }
        _ => AddAction::Queued,
    }
}

impl Manager {
    /// Every download, in sidebar queue order and then in line order.
    pub fn list_downloads(&self) -> Vec<DownloadSummary> {
        let state = self.lock();
        let mut entries: Vec<&Entry> = state.downloads.values().collect();
        let queue_index = |id: QueueId| state.queues.iter().position(|s| s.queue.id == id);
        entries.sort_by_key(|e| (queue_index(e.row.queue), e.position));
        entries.into_iter().map(|e| e.row.clone()).collect()
    }

    /// Adds a download from the app itself (paste, Add dialog, command palette).
    pub async fn add_download(&self, request: AddRequest) -> Result<DownloadSummary> {
        let url = parse_url(&request.url)?;
        let queue = match request.queue {
            Some(queue) => queue,
            None => self.lock_open()?.default_queue(),
        };
        let folder = request
            .folder
            .map(|f| self.inner.folders.resolve(&PortablePath(f)))
            .transpose()?;
        let added = self
            .insert(NewEntry {
                url,
                file_name: request.file_name,
                folder,
                queue,
                start: request.start,
                origin: Origin::default(),
                handover: None,
            })
            .await?;
        match added {
            Added::New(row) => Ok(row),
            Added::Known(_) => Err(CoreError::Internal(
                "a download without a handover was matched to one".into(),
            )),
        }
    }

    /// Pauses a download. A paused download only starts again when resumed.
    pub fn pause(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if !entry.stop(Stop::Pause) && entry.row.status == DownloadStatus::Queued {
            entry.row.status = DownloadStatus::Paused;
            self.save(&mut state, id);
            self.reconcile(&mut state);
        }
        Ok(())
    }

    /// Puts a paused or failed download back in line; it starts when its queue has room.
    pub fn resume(&self, id: DownloadId) -> Result<()> {
        self.requeue(id)
    }

    /// Tries a failed download again, keeping what was already downloaded when the
    /// server allows it.
    pub fn retry(&self, id: DownloadId) -> Result<()> {
        self.requeue(id)
    }

    /// Stops a download and deletes what was downloaded so far. It stays in the
    /// list as cancelled, so it can be tried again.
    pub fn cancel(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if entry.row.status == DownloadStatus::Completed {
            return Err(CoreError::WrongState(
                "This download has already finished, so there is nothing to cancel.",
            ));
        }
        if entry.stop(Stop::Cancel) {
            return Ok(());
        }
        mark_cancelled(entry);
        let files = partial_files(entry.path());
        self.save(&mut state, id);
        self.reconcile(&mut state);
        drop(state);
        self.delete_in_background(files.to_vec());
        Ok(())
    }

    /// Removes downloads from the list, stopping them first. Partial files are
    /// always deleted; the downloaded file only when `delete_file` is set. Ids that
    /// no longer exist are ignored.
    pub async fn remove(&self, ids: &[DownloadId], delete_file: bool) -> Result<()> {
        let mut exits = Vec::new();
        {
            let mut state = self.lock_open()?;
            for id in ids {
                if let Some(entry) = state.downloads.get_mut(id)
                    && let Some(run) = &entry.run
                {
                    exits.push(run.exited.clone());
                    entry.stop(Stop::Remove);
                }
            }
        }
        // The engine must close its files before they can be deleted.
        for exited in exits {
            exited.cancelled().await;
        }

        let mut files = Vec::new();
        let mut removed = Vec::new();
        {
            let mut state = self.lock_open()?;
            for id in ids {
                let Some(entry) = state.downloads.remove(id) else {
                    continue;
                };
                if entry.row.status != DownloadStatus::Completed {
                    files.extend(partial_files(entry.path()));
                }
                if delete_file {
                    files.push(entry.path().to_path_buf());
                }
                state.dirty.remove(id);
                removed.push(*id);
            }
            if removed.is_empty() {
                return Ok(());
            }
            self.inner.store.delete_downloads(removed.clone());
            self.emit(UiEvent::DownloadsRemoved(removed));
            self.reconcile(&mut state);
        }
        self.blocking(move || {
            delete_files(&files);
            Ok(())
        })
        .await
    }

    /// Gives a download a fresh link, for example after the old one expired. What
    /// was already downloaded is kept as long as the server still serves the same file.
    pub fn refresh_link(&self, id: DownloadId, url: &str) -> Result<()> {
        let url = parse_url(url)?;
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if entry.row.status == DownloadStatus::Completed {
            return Err(CoreError::WrongState(
                "This download has already finished, so its link cannot be changed.",
            ));
        }
        entry.row.url = url.to_string();
        entry.row.error = None;
        // A running download restarts on the new link; the engine continues the part file.
        if !entry.stop(Stop::Requeue) {
            entry.row.status = DownloadStatus::Queued;
        }
        self.save(&mut state, id);
        self.reconcile(&mut state);
        Ok(())
    }

    /// Moves downloads to the end of another queue. Running ones restart there, under
    /// that queue's limits, continuing their part files.
    pub fn move_to_queue(&self, ids: &[DownloadId], queue: QueueId) -> Result<()> {
        let mut state = self.lock_open()?;
        state.slot(queue)?;
        for id in ids {
            state.entry(*id)?;
        }
        for id in ids {
            let position = state.ids.position();
            if let Some(entry) = state.downloads.get_mut(id)
                && entry.row.queue != queue
            {
                entry.row.queue = queue;
                entry.position = position;
                entry.stop(Stop::Requeue);
                self.save(&mut state, *id);
            }
        }
        self.reconcile(&mut state);
        Ok(())
    }

    /// Puts the given downloads of `queue` in this order. They take the places in
    /// line they held between them; other downloads keep theirs.
    pub fn reorder(&self, queue: QueueId, ids_in_order: &[DownloadId]) -> Result<()> {
        let mut state = self.lock_open()?;
        state.slot(queue)?;
        let mut places = Vec::with_capacity(ids_in_order.len());
        for id in ids_in_order {
            let entry = state.entry(*id)?;
            if entry.row.queue != queue {
                return Err(CoreError::Invalid(
                    "Only downloads of the same queue can be put in order together.".into(),
                ));
            }
            places.push(entry.position);
        }
        places.sort_unstable();
        places.dedup();
        if places.len() != ids_in_order.len() {
            return Err(CoreError::Invalid(
                "Each download can appear only once in the new order.".into(),
            ));
        }
        for (id, position) in ids_in_order.iter().zip(places) {
            if let Some(entry) = state.downloads.get_mut(id) {
                entry.position = position;
            }
            self.save(&mut state, *id);
        }
        self.reconcile(&mut state);
        Ok(())
    }

    /// Pauses every download that is running or waiting.
    pub fn pause_all(&self) -> Result<()> {
        let mut state = self.lock_open()?;
        let ids: Vec<DownloadId> = state.downloads.keys().copied().collect();
        for id in ids {
            let Some(entry) = state.downloads.get_mut(&id) else {
                continue;
            };
            if !entry.stop(Stop::Pause) && entry.row.status == DownloadStatus::Queued {
                entry.row.status = DownloadStatus::Paused;
                self.save(&mut state, id);
            }
        }
        Ok(())
    }

    /// Puts every paused download back in line.
    pub fn resume_all(&self) -> Result<()> {
        let mut state = self.lock_open()?;
        let ids: Vec<DownloadId> = state
            .downloads
            .values()
            .filter(|e| e.row.status == DownloadStatus::Paused && !e.is_running())
            .map(|e| e.row.id)
            .collect();
        for id in ids {
            if let Some(entry) = state.downloads.get_mut(&id) {
                entry.row.status = DownloadStatus::Queued;
            }
            self.save(&mut state, id);
        }
        self.reconcile(&mut state);
        Ok(())
    }

    fn requeue(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        match entry.row.status {
            DownloadStatus::Completed => {
                Err(CoreError::WrongState("This download has already finished."))
            }
            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Cancelled
                if !entry.is_running() =>
            {
                entry.row.status = DownloadStatus::Queued;
                entry.row.error = None;
                self.save(&mut state, id);
                self.reconcile(&mut state);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Adds a download: picks its file name, category and a free path, then lets
    /// it start if its queue has room. A capture whose handover is already known
    /// (a retry from the extension, or two copies racing) adds nothing.
    pub(super) async fn insert(&self, new: NewEntry) -> Result<Added> {
        let (dir, name, category) = {
            let state = self.lock_open()?;
            state.slot(new.queue)?;
            let name = naming::file_name(new.file_name.as_deref(), &new.url);
            let category = settings::category_of(&state.settings, &name);
            let dir = match &new.folder {
                Some(folder) => folder.clone(),
                None => settings::folder_for(&state.settings, category, &self.inner.folders)?,
            };
            (dir, name, category)
        };

        // Checking the disk blocks, so it happens off the lock and off async threads;
        // if another add claimed the same path meanwhile, look again.
        loop {
            let claimed = self.lock_open()?.claimed_paths();
            let (dir, name) = (dir.clone(), name.clone());
            let path = self
                .blocking(move || Ok(naming::unique_path(&dir, &name, &claimed)))
                .await?;

            let mut state = self.lock_open()?;
            if let Some(known) = new.handover.as_deref().and_then(|h| state.handovers.get(h)) {
                return Ok(Added::Known(known.reply.clone()));
            }
            if state.claimed_paths().contains(&path) {
                continue;
            }
            state.slot(new.queue)?;
            let id = state.ids.download();
            let position = state.ids.position();
            let row = DownloadSummary {
                id,
                url: new.url.to_string(),
                file_name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
                queue: new.queue,
                category,
                status: if new.start {
                    DownloadStatus::Queued
                } else {
                    DownloadStatus::Paused
                },
                total: None,
                done: 0,
                speed: 0,
                connections: 0,
                eta_secs: None,
                error: None,
                added_at: self.inner.clock.unix_millis(),
                completed_at: None,
            };
            state.downloads.insert(
                id,
                Entry {
                    row,
                    position,
                    origin: new.origin,
                    run: None,
                    resume_first: false,
                },
            );
            self.save(&mut state, id);
            self.reconcile(&mut state);
            let row = state.entry(id)?.row.clone();
            if let Some(handover) = &new.handover {
                let reply = AddedDownload {
                    id: Some(id),
                    action: capture_action(&row),
                };
                let target = Target::Download(id);
                state.handovers.insert(handover, Handover { reply, target });
            }
            return Ok(Added::New(row));
        }
    }
}

/// Parses a link, accepting only http and https.
pub(super) fn parse_url(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    Url::parse(trimmed)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| CoreError::InvalidUrl(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_are_accepted() {
        assert!(parse_url(" https://example.com/a.zip ").is_ok());
        assert!(parse_url("http://example.com").is_ok());
        for bad in ["ftp://example.com/a", "file:///etc/passwd", "not a url", ""] {
            assert!(
                matches!(parse_url(bad), Err(CoreError::InvalidUrl(_))),
                "{bad}"
            );
        }
    }
}
