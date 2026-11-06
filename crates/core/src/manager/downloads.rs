//! Adding downloads and moving them through their life: pause, resume, cancel,
//! retry, refresh link, remove, and their place in queues.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::warn;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_protocol::app::{
    AddRequest, Category, DownloadStatus, DownloadSummary, PortablePath, UiEvent,
};
use wirecatch_protocol::extension::{AddAction, AddedDownload};
use wirecatch_protocol::rules::QueueReason;
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_records::activity::{Activity, Happened};
use wirecatch_records::files::DownloadFiles;
use wirecatch_sites::local_network::ChosenBy;
use wirecatch_store::NameChoice;

use super::Manager;
use super::arrival::{Fetch, Kind, NewEntry, Wanted};
use super::file_exists::give_up_the_name;
use super::handovers::{Handover, Target};
use super::placement::{Candidate, QueueRequest};
use super::run::mark_cancelled;
use super::state::{AfterCancel, Entry, Hold, NextRun, Partial, Seen, State, Stop};
use crate::error::{CoreError, Result};
use crate::headers;
use crate::naming;

fn remember_handover(state: &mut State, handover: &str, row: &DownloadSummary) {
    let action = capture_action(row);
    // A started capture is only taken over once the server answers;
    // until then a duplicate add waits for that outcome.
    let deciding = (action == AddAction::Started).then(CancellationToken::new);
    let record = Handover {
        reply: AddedDownload {
            id: Some(row.id),
            action,
        },
        target: Target::Download(row.id),
        deciding,
    };
    state.handovers.insert(handover, record);
}

/// How a captured download stands right after it was added: started if it is
/// running (provisionally, until the server answers) or already finished, queued
/// if its queue cannot run it yet.
pub(super) fn capture_action(row: &DownloadSummary) -> AddAction {
    match row.status {
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Completed => {
            AddAction::Started
        }
        _ => AddAction::Queued,
    }
}

/// Where a download's fresh link comes from ([`Manager::refresh_link`]), which decides whose
/// link it is (D39, D43.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFrom {
    /// The person, where nothing a website chose is shown (the main window): the link becomes
    /// theirs, whoever chose the old one, so it may reach their own network.
    Person,
    /// A page that also shows what websites chose, such as a download's own window, where a
    /// script could have written the link: it keeps the rule of whoever chose the download's
    /// link, so a website's download stays under a website's rule.
    SharedPage,
}

/// What [`Manager::remove`] does with a completed download's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveFiles {
    Keep,
    Delete,
}

impl Manager {
    /// Every download, grouped by queue: the queues in the order
    /// [`list_queues`](Self::list_queues) returns them, and within each queue in
    /// line order (the order [`reorder`](Self::reorder) sets, new downloads last).
    /// The UI may rely on this order.
    #[must_use]
    pub fn list_downloads(&self) -> Vec<DownloadSummary> {
        let state = self.lock();
        let mut entries: Vec<&Entry> = state.visible().collect();
        let queue_index = |id: QueueId| state.queues.iter().position(|s| s.queue.id == id);
        entries.sort_by_key(|e| (queue_index(e.row.queue), e.position));
        entries
            .into_iter()
            .map(super::state::Entry::summary)
            .collect()
    }

    /// One download, as [`list_downloads`](Self::list_downloads) shows it.
    ///
    /// # Errors
    ///
    /// [`CoreError::DownloadNotFound`] when there is no download `id`.
    pub fn get_download(&self, id: DownloadId) -> Result<DownloadSummary> {
        Ok(self.lock().entry(id)?.summary())
    }

    /// What happened to download `id` after it was added, oldest first (D236).
    ///
    /// # Errors
    ///
    /// [`CoreError::DownloadNotFound`] when there is no download `id`.
    pub fn activity(&self, id: DownloadId) -> Result<Vec<Happened>> {
        Ok(self.lock().entry(id)?.activity.events().to_vec())
    }

    /// Adds a download from the app itself (paste, Add dialog, command palette) and
    /// returns its id. Its row arrives with the next `DownloadsChanged` event and is
    /// in [`list_downloads`](Self::list_downloads) at once.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidUrl`] for a link that is not http, https or FTP;
    /// [`CoreError::QueueNotFound`] for an unknown queue; [`CoreError::Path`] when the
    /// folder does not resolve; [`CoreError::Invalid`] for a file name that cannot be
    /// used; and [`CoreError::Closed`] once the manager has shut down.
    pub async fn add_download(&self, request: AddRequest) -> Result<DownloadId> {
        let url = parse_url(request.url.as_str())?;
        let folder = request
            .folder
            .map(|f| self.inner.folders.resolve(&PortablePath(f)))
            .transpose()?;
        let placed = {
            let state = self.lock_open()?;
            let chosen = request.file_name.as_deref().map(naming::without_folders);
            let name = naming::file_name(chosen.as_deref(), &url)?;
            let candidate = Candidate {
                url: &url,
                file_name: &name,
                category: None,
            };
            state.place(QueueRequest::in_app(request.queue), &candidate)?
        };
        let added = self
            .insert(NewEntry {
                fetch: Fetch::typed(url),
                wanted: Wanted {
                    file_name: request.file_name,
                    folder,
                    ..Wanted::default()
                },
                placed,
                start: request.start,
                kind: Kind::File { video_check: true },
            })
            .await?;
        match added {
            Some(row) => Ok(row.id),
            None => Err(CoreError::Internal(
                "a download without a handover was matched to one".into(),
            )),
        }
    }

    /// Pauses a download. A paused download only starts again when resumed.
    ///
    /// # Errors
    ///
    /// [`CoreError::DownloadNotFound`] when there is no download `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn pause(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if !self.stop_run(entry, Stop::Pause) && entry.row.status == DownloadStatus::Queued {
            entry.row.status = DownloadStatus::Paused;
            self.save(&mut state, id);
            self.reconcile(&mut state);
        }
        Ok(())
    }

    /// Puts a paused or failed download back in line; it starts when its queue has room.
    ///
    /// # Errors
    ///
    /// [`CoreError::WrongState`] for a download that has finished,
    /// [`CoreError::DownloadNotFound`] when there is no download `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn resume(&self, id: DownloadId) -> Result<()> {
        self.requeue(id)
    }

    /// Tries a failed download again, keeping what was already downloaded when the
    /// server allows it.
    ///
    /// # Errors
    ///
    /// The errors of [`resume`](Self::resume).
    pub fn retry(&self, id: DownloadId) -> Result<()> {
        self.requeue(id)
    }

    /// Stops a download and deletes what was downloaded so far. It stays in the
    /// list as cancelled, so it can be tried again.
    ///
    /// # Errors
    ///
    /// [`CoreError::WrongState`] for a download that has finished,
    /// [`CoreError::DownloadNotFound`] when there is no download `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn cancel(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if entry.row.status == DownloadStatus::Completed {
            return Err(CoreError::WrongState(
                "This download has already finished, so there is nothing to cancel.",
            ));
        }
        self.close_questions_of(&mut state, id);
        let entry = state.entry_mut(id)?;
        if self.stop_run(entry, Stop::Cancel) {
            return Ok(());
        }
        if entry.row.status != DownloadStatus::Cancelled {
            mark_cancelled(entry);
            self.discard_partial(&mut state, id);
            give_up_the_name(&mut state, id);
            self.save(&mut state, id);
            self.reconcile(&mut state);
        }
        Ok(())
    }

    /// Removes downloads from the list, stopping them first. The partial files of an unfinished download are always deleted; the file of a completed download, with its subtitle files, only with [`RemoveFiles::Delete`]. Ids that no longer exist are ignored.
    ///
    /// # Errors
    ///
    /// [`CoreError::Closed`] once the manager has shut down. Files that cannot be
    /// deleted are logged, not reported.
    ///
    /// Runs to the end even when the caller stops waiting.
    pub async fn remove(&self, ids: &[DownloadId], completed: RemoveFiles) -> Result<()> {
        // From here on nothing can resume, move or start them, so once their runs
        // have stopped they stay stopped.
        let under_way = {
            let mut state = self.lock_open()?;
            self.begin_removing(&mut state, ids);
            removal_under_way(&mut state)
        };
        let (manager, ids) = (self.clone(), ids.to_vec());
        self.to_the_end(async move {
            let _under_way = under_way;
            manager.finish_removing(&ids, completed).await
        })
        .await
    }

    /// Marks downloads as being removed and takes them off the saved list, so a quit before the removal ends cannot bring them back. Their files stay until [`finish_removing`](Self::finish_removing), which a quit waits for; the next start finishes what a kill cut short (D134).
    pub(super) fn begin_removing(&self, state: &mut State, ids: &[DownloadId]) {
        let mut begun = Vec::new();
        for id in ids {
            if let Ok(entry) = state.entry_mut(*id) {
                entry.hold.begin_remove();
                begun.push(*id);
                if entry.row.status != DownloadStatus::Completed || entry.files.moving.is_some() {
                    let deletion = entry.pending_deletion();
                    state.pending_deletions.insert(*id, deletion);
                }
            }
        }
        // Before the rows go: a kill between the two leaves a row, whose files the next start spares.
        self.save_pending_deletions(state);
        self.inner.store.delete_downloads(begun);
    }

    /// Stops the runs of downloads being removed and waits for those and for their moves.
    async fn wait_until_stopped(&self, ids: &[DownloadId]) {
        // The engine must close its files before they can be deleted, and a move to
        // another folder must finish so the right files are deleted. Each wait for a
        // run is bounded by the stop timeout; a move takes as long as the disk.
        loop {
            let mut waits = Vec::new();
            {
                let mut state = self.lock();
                for id in ids {
                    let Some(entry) = state.downloads.get_mut(id).filter(|e| e.hold.is_removing())
                    else {
                        continue;
                    };
                    waits.extend(entry.hold.move_under_way());
                    if let Some(run) = &entry.run {
                        waits.push(run.exited.clone());
                        self.stop_run(entry, Stop::Remove);
                    }
                }
            }
            if waits.is_empty() {
                break;
            }
            for done in waits {
                done.cancelled().await;
            }
        }
    }

    /// Removes downloads already marked as being removed, once their runs and moves have ended. Carries on after a shutdown began, as the removal was already promised.
    async fn finish_removing(&self, ids: &[DownloadId], completed: RemoveFiles) -> Result<()> {
        self.wait_until_stopped(ids).await;

        let mut finished_files = Vec::new();
        let mut subtitles = Vec::new();
        let (mut partials, mut removed, mut leaving) = (Vec::new(), Vec::new(), Vec::new());
        {
            let mut state = self.lock();
            for id in ids {
                if !state
                    .downloads
                    .get(id)
                    .is_some_and(|e| e.hold.is_removing())
                {
                    continue;
                }
                self.close_questions_of(&mut state, *id);
                let Some(entry) = state.remove_entry(*id) else {
                    continue;
                };
                leaving.push((entry.path().to_path_buf(), *id));
                if entry.row.status == DownloadStatus::Completed {
                    if completed == RemoveFiles::Delete {
                        finished_files.push(entry.path().to_path_buf());
                        subtitles.push((
                            *id,
                            entry.path().to_path_buf(),
                            entry.files.sidecars.clone(),
                        ));
                    }
                } else {
                    // An unfinished download has no file of its own at its final
                    // path yet; whatever is there belongs to someone else.
                    // A move may have changed the path since the removal began.
                    let deletion = entry.pending_deletion();
                    partials.extend(Partial::both_places(&deletion));
                    leaving.extend(deletion.moving_to.iter().map(|to| (to.clone(), *id)));
                    state.pending_deletions.insert(*id, deletion);
                }
                removed.push(*id);
            }
            if removed.is_empty() {
                return Ok(());
            }
            self.save_pending_deletions(&state);
            finished_files = files_no_row_lists(&state, finished_files);
            // Only with the file itself: another row may keep it.
            for (id, file, beside) in subtitles {
                if finished_files.contains(&file) {
                    let beside = files_no_row_lists(&state, beside);
                    leaving.extend(beside.iter().map(|path| (path.clone(), id)));
                    finished_files.extend(beside);
                }
            }
            // After that check, which reads a claim as another download's. A new download's name must not meet files about to be deleted.
            leaving.retain(|(path, id)| state.claimed.reserve(path, *id));
            self.emit(UiEvent::DownloadsRemoved(removed.clone()));
            // Their queue may be done now; the ticker says so.
            self.inner.rows_changed.notify_one();
            self.reconcile(&mut state);
        }
        #[cfg(test)]
        let before_deleting = self.before_deleting();
        self.blocking(move || {
            #[cfg(test)]
            if let Some(hook) = before_deleting {
                hook();
            }
            // Never as a folder: by now a folder may have taken the finished file's name.
            for file in &finished_files {
                wirecatch_files::discard::file(file);
            }
            for partial in &partials {
                partial.delete();
            }
            Ok(())
        })
        .await?;
        let mut state = self.lock();
        state
            .pending_deletions
            .retain(|id, _| !removed.contains(id));
        self.save_pending_deletions(&state);
        for (path, id) in &leaving {
            state.claimed.release(path, *id);
        }
        Ok(())
    }

    pub(super) fn save_pending_deletions(&self, state: &State) {
        self.inner
            .store
            .save_pending_deletions(state.pending_deletions.values().cloned().collect());
    }

    /// Hides downloads at once, as [`remove`](Self::remove) does, and finishes
    /// removing them in the background.
    pub(super) fn remove_in_background(&self, ids: Vec<DownloadId>) {
        let under_way = {
            let mut state = self.lock();
            self.begin_removing(&mut state, &ids);
            removal_under_way(&mut state)
        };
        let manager = self.clone();
        self.spawn(async move {
            let _under_way = under_way;
            if let Err(e) = manager.finish_removing(&ids, RemoveFiles::Keep).await {
                warn!(error = %e, "could not finish removing a download");
            }
        });
    }

    /// Gives a download a fresh link, for example after the old one expired, from `from`, which
    /// decides whose link it is and so whether it may reach the local network (D39, D43.5). What
    /// was already downloaded is kept as long as the server can continue a download and still serves the same file, except for
    /// a video read from a page, whose work folder belongs to one link and is cleared for another.
    /// A link to another site (scheme, host or port) does not get the browser's
    /// cookies and credentials for the old one; the user agent and harmless
    /// headers such as the preferred language are kept.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidUrl`] for a link that is not http, https or FTP,
    /// [`CoreError::WrongState`] for a download that has finished or whose files
    /// a failed move left split between two folders,
    /// [`CoreError::DownloadNotFound`] when there is no download `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn refresh_link(&self, id: DownloadId, url: &str, from: LinkFrom) -> Result<()> {
        let url = parse_url(url)?;
        let mut state = self.lock_open()?;
        let entry = state.entry_mut(id)?;
        if entry.row.status == DownloadStatus::Completed {
            return Err(CoreError::WrongState(
                "This download has already finished, so its link cannot be changed.",
            ));
        }
        entry.refuse_if_stranded()?;
        self.close_questions_of(&mut state, id);
        let entry = state.entry_mut(id)?;
        let same_site = Url::parse(&entry.row.url).is_ok_and(|old| old.origin() == url.origin());
        if !same_site {
            entry.origin.headers = headers::for_another_site(&entry.origin.headers);
        }
        entry.row.url = url.to_string();
        // A fresh link serves the file as it is now, not as the browser saw it then.
        entry.expect = None;
        if from == LinkFrom::Person {
            entry.chosen_by = ChosenBy::Person;
        }
        entry.row.error = None;
        // A running download restarts on the new link; the engine continues the part file.
        if !self.stop_run(entry, Stop::Requeue) {
            entry.row.status = DownloadStatus::Queued;
        }
        self.save(&mut state, id);
        self.reconcile(&mut state);
        Ok(())
    }

    /// Moves downloads to the end of another queue. Running ones restart there, under
    /// that queue's limits, continuing their part files.
    ///
    /// # Errors
    ///
    /// [`CoreError::QueueNotFound`] or [`CoreError::DownloadNotFound`] when the queue
    /// or one of the downloads does not exist, in which case nothing moves; and
    /// [`CoreError::Closed`] once the manager has shut down.
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
                entry.row.queue_reason = QueueReason::Picked;
                entry.position = position;
                self.stop_run(entry, Stop::Requeue);
                self.save(&mut state, *id);
            }
        }
        self.reconcile(&mut state);
        Ok(())
    }

    /// Puts the given downloads of `queue` in this order. They take the places in
    /// line they held between them; other downloads keep theirs.
    ///
    /// # Errors
    ///
    /// [`CoreError::QueueNotFound`] or [`CoreError::DownloadNotFound`] for an unknown
    /// queue or download, [`CoreError::Invalid`] when a download is in another queue or
    /// appears twice, and [`CoreError::Closed`] once the manager has shut down. Nothing
    /// changes on an error.
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
    ///
    /// # Errors
    ///
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn pause_all(&self) -> Result<()> {
        let mut state = self.lock_open()?;
        let ids: Vec<DownloadId> = state.downloads.keys().copied().collect();
        for id in ids {
            let Ok(entry) = state.entry_mut(id) else {
                continue;
            };
            // Not a choice about this download: one it is already stopping by stays.
            if entry
                .run
                .as_ref()
                .and_then(|run| run.stop)
                .is_some_and(Stop::is_users_choice)
            {
                continue;
            }
            if !self.stop_run(entry, Stop::Pause) && entry.row.status == DownloadStatus::Queued {
                entry.row.status = DownloadStatus::Paused;
                self.save(&mut state, id);
            }
        }
        // A paused download no longer waits for its queue: clear its reason now.
        self.reconcile(&mut state);
        Ok(())
    }

    /// Puts every paused download back in line.
    ///
    /// # Errors
    ///
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn resume_all(&self) -> Result<()> {
        let mut state = self.lock_open()?;
        let mut paused = Vec::new();
        for entry in state
            .downloads
            .values_mut()
            .filter(|e| !e.hold.is_removing())
        {
            if entry.is_running() {
                // Still stopping after a pause: it goes back in line once stopped.
                entry.requeue_when_stopped(AfterCancel::Stay);
            } else if entry.row.status == DownloadStatus::Paused {
                entry.row.status = DownloadStatus::Queued;
                paused.push(entry.row.id);
            }
        }
        for id in paused {
            self.close_questions_of(&mut state, id);
            self.save(&mut state, id);
        }
        self.reconcile(&mut state);
        Ok(())
    }

    /// Puts a paused, failed or cancelled download back in line. One that is still
    /// stopping goes back in line as soon as it has stopped. A cancelled download
    /// starts over.
    fn requeue(&self, id: DownloadId) -> Result<()> {
        let mut state = self.lock_open()?;
        state.entry_mut(id)?;
        self.close_questions_of(&mut state, id);
        // Trying again means the person has dealt with files a failed move left.
        state.release_stranded(id);
        let entry = state.entry_mut(id)?;
        if entry.is_running() {
            entry.requeue_when_stopped(AfterCancel::Restart);
            return Ok(());
        }
        match entry.row.status {
            DownloadStatus::Completed => {
                Err(CoreError::WrongState("This download has already finished."))
            }
            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Cancelled => {
                // The partial files of a cancelled download were deleted when it was
                // cancelled; delete again if that happened before a restart.
                if entry.row.status == DownloadStatus::Cancelled && entry.cleanup.is_none() {
                    self.discard_partial(&mut state, id);
                }
                let entry = state.entry_mut(id)?;
                entry.row.status = DownloadStatus::Queued;
                entry.row.error = None;
                self.save(&mut state, id);
                self.reconcile(&mut state);
                Ok(())
            }
            DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing => Ok(()),
        }
    }

    /// Adds a download: picks its file name, category and a path no other download holds, then lets it start if its queue has room. Returns the new row, or `None` for a capture whose handover is already known (a retry from the extension, or two copies racing), which adds nothing.
    pub(super) async fn insert(&self, new: NewEntry) -> Result<Option<DownloadSummary>> {
        let (dir, name, category) = self.destination(&new)?;
        // If another add claimed the same path meanwhile, look again.
        loop {
            // Asked of the live set, under a short lock per name tried: a copy of the set for
            // each add makes a bulk add quadratic (CORE-10).
            let (dir, name, manager) = (dir.clone(), name.clone(), self.clone());
            let path = self
                .blocking(move || {
                    // Not `unique_path`: a file on disk is for the setting to judge once the name is final (D96), and a name changed for it here would hide it from that.
                    Ok(naming::unclaimed_path(&dir, &name, |path: &Path| {
                        manager.lock().claimed.contains(path)
                    }))
                })
                .await?;

            let mut state = self.lock_open()?;
            if new
                .kind
                .handover()
                .is_some_and(|h| state.handovers.get(h).is_some())
            {
                return Ok(None);
            }
            // Two captures of one link can both have found no download to join, off the lock.
            if let Some(handover) = new.kind.handover()
                && self
                    .join_unfinished_locked(&mut state, handover, new.fetch.url.as_str())
                    .is_some()
            {
                return Ok(None);
            }
            if state.claimed.contains(&path) {
                continue;
            }
            state.slot(new.placed.queue)?;
            return self.add_entry(&mut state, new, &path, category).map(Some);
        }
    }

    /// Where a new download goes: its folder, its file name and its category.
    fn destination(&self, new: &NewEntry) -> Result<(PathBuf, String, Category)> {
        let state = self.lock_open()?;
        state.slot(new.placed.queue)?;
        let wanted = &new.wanted;
        let chosen = wanted.file_name.as_deref().map(naming::without_folders);
        let name = naming::file_name(
            chosen.as_deref().or(wanted.hint_name.as_deref()),
            &new.fetch.url,
        )?;
        let category = wanted
            .category
            .unwrap_or_else(|| wirecatch_settings::category_of(&state.settings, &name));
        let dir = match &wanted.folder {
            Some(folder) => folder.clone(),
            None => state.folder_for(new.placed.queue, category, &self.inner.folders)?,
        };
        Ok((dir, name, category))
    }

    /// Puts `new` in the list at `path`, lets it start if its queue has room, and remembers its handover; returns its row.
    fn add_entry(
        &self,
        state: &mut State,
        new: NewEntry,
        path: &Path,
        category: Category,
    ) -> Result<DownloadSummary> {
        let NewEntry {
            fetch,
            wanted,
            placed,
            start,
            kind,
        } = new;
        let (handover, expect, media, video_check) = kind.into_parts();
        let id = state.ids.download();
        self.inner.store.save_last_ids(state.ids.last());
        let position = state.ids.position();
        let row = DownloadSummary {
            id,
            url: fetch.url.to_string(),
            file_name: crate::naming::file_name_of(path),
            path: path.to_string_lossy().into_owned(),
            queue: placed.queue,
            queue_reason: placed.reason,
            category,
            status: if start {
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
            waiting: None,
            media: media.as_ref().map(|m| m.summary.clone()),
            processing: None,
            proxy: None,
        };
        let media_job = media.and_then(|m| m.job);
        // The path is a guess until the server answers with its own name; a
        // media job's is final.
        state.insert_entry(Entry {
            seen: Seen::of(&row),
            row,
            position,
            origin: fetch.origin,
            chosen_by: fetch.chosen_by,
            naming: NameChoice {
                settled: media_job.is_some(),
                writing: false,
                name: wanted.file_name,
                folder: wanted.folder.map(|f| f.to_string_lossy().into_owned()),
            },
            expect,
            run: None,
            resume_first: false,
            through_proxy: None,
            cleanup: None,
            hold: Hold::Free,
            on_existing: None,
            plain_name: None,
            confirmed_name: wanted.confirmed_name,
            next_run: NextRun::Continues,
            offered_file: false,
            stranded: None,
            files: DownloadFiles::default(),
            media_job,
            video_check,
            activity: Activity::default(),
        });
        self.save(state, id);
        self.reconcile(state);
        let row = state.entry(id)?.summary();
        if let Some(handover) = &handover {
            remember_handover(state, handover, &row);
        }
        Ok(row)
    }
}

/// Parses a link, accepting http, https and the FTP family (`ftp`, `ftps` for
/// implicit TLS, `ftpes` for explicit TLS), which the engine downloads too.
pub(super) fn parse_url(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    Url::parse(trimmed)
        .ok()
        .filter(|url| {
            wirecatch_engine::url::is_supported(url)
                && url.host_str().is_some_and(|h| !h.is_empty())
        })
        .map(wirecatch_engine::url::with_normal_host)
        .ok_or_else(|| CoreError::InvalidUrl(trimmed.to_string()))
}

/// Parses a web link (http or https only), for media pages and streams.
pub(super) fn parse_web_url(raw: &str) -> Result<Url> {
    parse_url(raw)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| CoreError::InvalidUrl(raw.trim().to_string()))
}

/// Keeps a quit waiting until what it returns is dropped.
fn removal_under_way(state: &mut State) -> impl Send + use<> {
    state.removals.retain(|done| !done.is_cancelled());
    let done = CancellationToken::new();
    state.removals.push(done.clone());
    done.drop_guard()
}

/// `files` of removed finished downloads, each once, without those another finished row lists or another download holds (D118).
fn files_no_row_lists(state: &State, mut files: Vec<PathBuf>) -> Vec<PathBuf> {
    if files.is_empty() {
        return files;
    }
    let mut seen: HashSet<String> = state
        .downloads
        .values()
        .filter(|e| e.row.status == DownloadStatus::Completed)
        .map(|e| naming::key(e.path()))
        .collect();
    files.retain(|file| !state.claimed.contains(file) && seen.insert(naming::key(file)));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// So a refusal of a local address and a `<local>` queue rule read what a connection will reach.
    #[test]
    fn an_ftp_links_host_reaches_every_check_in_its_normal_form() {
        let host = |raw: &str| parse_url(raw).unwrap().host_str().unwrap().to_owned();
        assert_eq!(host("ftps://%31%32%37.0.0.1/a"), "127.0.0.1");
        assert_eq!(host("ftps://2130706433/a"), "127.0.0.1");
        assert_eq!(host("ftps://FILES.Home.Arpa/a.zip"), "files.home.arpa");
    }

    #[test]
    fn web_and_ftp_links_are_accepted() {
        assert!(parse_url(" https://example.com/a.zip ").is_ok());
        assert!(parse_url("http://example.com").is_ok());
        for ftp in [
            "ftp://example.com/a",
            "ftps://example.com/a",
            "ftpes://u:p@example.com/a",
        ] {
            assert!(parse_url(ftp).is_ok(), "{ftp}");
            assert!(parse_web_url(ftp).is_err(), "{ftp}");
        }
        for bad in [
            "sftp://example.com/a",
            "file:///etc/passwd",
            "not a url",
            "",
            "http://",
        ] {
            assert!(
                matches!(parse_url(bad), Err(CoreError::InvalidUrl(_))),
                "{bad}"
            );
        }
    }
}
