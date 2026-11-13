//! Choosing a download's final path from the server's answer, on its first run.
//!
//! Until the server answers, a download's name and folder are a guess made when it
//! was added (from the user's choice, the browser's suggestion or the link). The
//! engine calls the resolver built here right after the server answers with the
//! file and before anything is written, so the name can come from the server's
//! `Content-Disposition` and the folder from the real file type, without an extra
//! request. The chosen path is saved at once; later runs use it as is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use wirecatch_product::PRODUCT_NAME;

use tracing::warn;
use wirecatch_engine::{CancellationToken, PathRejected, PathResolver, RemoteFile, Url};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::Category;

use super::Manager;
use super::state::{Entry, Expectation, State, Stop};
use crate::naming;

/// What the server said about the file, as far as choosing its path goes.
#[derive(Debug, Clone)]
pub(super) struct ServerAnswer {
    /// The engine's suggestion: `Content-Disposition`, else the link's name, else
    /// a generic name.
    pub suggested_name: String,
    pub total: Option<u64>,
    pub content_type: Option<String>,
    pub final_url: Url,
}

impl From<&RemoteFile> for ServerAnswer {
    fn from(remote: &RemoteFile) -> Self {
        Self {
            suggested_name: remote.suggested_name.clone(),
            total: remote.total,
            content_type: remote.content_type.clone(),
            final_url: remote.final_url.clone(),
        }
    }
}

/// Where a server's answer puts a download.
struct Place {
    dir: PathBuf,
    name: String,
    category: Category,
    /// Its path until now.
    provisional: PathBuf,
}

/// Chooses a download's path from the server's answer; `Err` refuses the file.
pub(super) type Settle = Arc<dyn Fn(&ServerAnswer) -> Result<PathBuf, String> + Send + Sync>;

/// The engine's resolver for `settle`.
pub(super) fn path_resolver(settle: Settle) -> PathResolver {
    Arc::new(move |remote: &RemoteFile| {
        settle(&ServerAnswer::from(remote)).map_err(PathRejected::new)
    })
}

impl Manager {
    /// Settles the path for run `run_id` of download `id`. On success it cancels
    /// `answered`, which is how a waiting capture learns the server answered.
    pub(super) fn settler(
        &self,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
    ) -> Settle {
        let manager = Arc::downgrade(&self.inner);
        Arc::new(move |answer: &ServerAnswer| {
            let manager = Manager::from_weak(&manager)
                .ok_or_else(|| format!("{PRODUCT_NAME} is closing."))?;
            let path = manager.settle_path(id, run_id, answer)?;
            answered.cancel();
            Ok(path)
        })
    }

    /// Picks the final path for the server's file, claims it, and saves it. Runs on the download's task with the manager's lock held only briefly; it blocks on checking whether names are free and on the database.
    fn settle_path(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
    ) -> Result<PathBuf, String> {
        if let Some(path) =
            off_async_worker(|| self.begin_writing_at_settled_path(id, run_id, remote))?
        {
            return Ok(path);
        }
        let url = self
            .lock()
            .downloads
            .get(&id)
            .map_or_default(|entry| entry.row.url.clone());
        // A path holding only this download's own part and resume files is free
        // for it: the app stopped there before the path was saved, and the engine
        // continues those files.
        let usable = |path: &Path| naming::is_free_on_disk(path) || naming::left_by(path, &url);
        self.settle_path_checking(id, run_id, remote, &usable)
    }

    /// Records that run `run_id` of download `id` is about to write, if its path was already final, and returns that path.
    fn begin_writing_at_settled_path(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
    ) -> Result<Option<PathBuf>, String> {
        let path = {
            let mut state = self.lock();
            let entry = current_run(&state, id, run_id)?;
            if !entry.naming.settled {
                return Ok(None);
            }
            let path = entry.path().to_path_buf();
            let writing = entry.naming.writing;
            if let Some(expect) = &entry.expect {
                check_expectation(expect, remote)?;
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.expect = None;
                    self.save(&mut state, id);
                }
            }
            if writing {
                return Ok(Some(path));
            }
            path
        };
        // Before the record: with it, no part file means the file there is this download's, also after a crash or a failed run. The engine starts over from an empty one.
        let [part, _] = naming::partial_files(&path);
        let made = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                wirecatch_engine::naming::created_hidden(
                    std::fs::OpenOptions::new().create(true).append(true),
                )
                .open(&part)
                .map(drop)
            });
        let mut guard = self.lock();
        let state = &mut *guard;
        current_run(state, id, run_id)?;
        match made {
            Ok(()) => {
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.naming.writing = true;
                }
                self.save(state, id);
                drop(guard);
                self.wait_for_record(id);
            }
            // The engine meets the same trouble and reports it; the record stays off.
            Err(error) => warn!(id = id.0, %error, "could not create a download's part file"),
        }
        Ok(Some(path))
    }

    /// [`settle_path`](Self::settle_path), asking `free_on_disk` whether a path is
    /// free on disk.
    pub(super) fn settle_path_checking(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
        free_on_disk: &dyn Fn(&Path) -> bool,
    ) -> Result<PathBuf, String> {
        off_async_worker(|| self.settle_on_disk(id, run_id, remote, free_on_disk))
    }

    /// Where the server's answer puts run `run_id` of download `id`, unless it stops the run.
    fn place_for_answer(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
    ) -> Result<Place, String> {
        let state = self.lock();
        let entry = current_run(&state, id, run_id)?;
        if let Some(expect) = &entry.expect {
            check_expectation(expect, remote)?;
        }
        let name = naming::settled_name(
            entry.naming.name.as_deref(),
            &remote.suggested_name,
            &remote.final_url,
            entry.plain_name.as_deref().unwrap_or(&entry.row.file_name),
        )
        .map_err(|e| e.to_string())?;
        if entry
            .confirmed_name
            .as_deref()
            .is_some_and(|confirmed| naming::changes_file_type(confirmed, &name))
        {
            drop(state);
            return Err(self.stop_for_file_type(id, run_id, name)?);
        }
        let category = settled_category(&state, entry, &name, remote);
        let dir = match &entry.naming.folder {
            Some(folder) => PathBuf::from(folder),
            None => state
                .folder_for(entry.row.queue, category, &self.inner.folders)
                .map_err(|e| e.to_string())?,
        };
        Ok(Place {
            dir,
            name,
            category,
            provisional: entry.path().to_path_buf(),
        })
    }

    fn settle_on_disk(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
        free_on_disk: &dyn Fn(&Path) -> bool,
    ) -> Result<PathBuf, String> {
        if self.read_as_video(id, run_id, remote)? {
            return Err(READ_AS_VIDEO.into());
        }
        let Place {
            dir,
            name,
            category,
            provisional,
        } = self.place_for_answer(id, run_id, remote)?;

        let first = naming::candidate(&dir, &name, 0);
        self.stop_for_duplicate(id, run_id, remote, &first, category)?;

        // The disk is checked without the lock, which a folder that is slow to
        // answer (an unreachable network share) would otherwise keep from every
        // command. Each name is claimed before its check, so no other download can
        // take it meanwhile, and given back if the disk says it is taken.
        let mut tried = 0;
        let (path, claimed_here) = loop {
            let candidate = naming::candidate(&dir, &name, tried);
            tried += 1;
            let claimed_here = {
                let mut state = self.lock();
                current_run(&state, id, run_id)?;
                // The download's own provisional path is its to keep.
                if naming::same_path(&candidate, &provisional) {
                    false
                } else if state.claimed.reserve(&candidate, id) {
                    true
                } else {
                    continue;
                }
            };
            if free_on_disk(&candidate) {
                break (candidate, claimed_here);
            }
            if claimed_here {
                self.lock().claimed.release(&candidate, id);
            }
        };

        let mut guard = self.lock();
        let state = &mut *guard;
        // The run may have been stopped while the disk was checked.
        if let Err(reason) = current_run(state, id, run_id) {
            if claimed_here {
                state.claimed.release(&path, id);
            }
            return Err(reason);
        }
        if !naming::same_path(&path, &provisional) {
            state.claimed.remove(&provisional, id);
        }
        let Some(entry) = state.downloads.get_mut(&id) else {
            state.claimed.release(&path, id);
            return Err(REMOVED.into());
        };
        state.claimed.insert(&path, id);
        entry.row.path = path.to_string_lossy().into_owned();
        entry.row.file_name = naming::file_name_of(&path);
        entry.row.category = category;
        entry.naming.settled = true;
        entry.naming.writing = true;
        entry.plain_name = None;
        entry.confirmed_name = None;
        entry.expect = None;
        entry.video_check = false;
        self.save(state, id);
        drop(guard);
        self.wait_for_record(id);
        Ok(path)
    }
}

impl Manager {
    /// A media run of download `id` is about to put its finished file in place.
    pub(super) fn begin_placing(&self, id: DownloadId) {
        let mut guard = self.lock();
        let state = &mut *guard;
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.naming.writing = true;
            self.save(state, id);
            drop(guard);
            off_async_worker(|| self.wait_for_record(id));
        }
    }

    /// Blocks until the database has tried to save the record that a run writes at its path. Saves are queued, and a kill that outruns this one leaves the finished file looking like another program's, so the next run saves a second copy. A save that fails or runs late is logged and the run goes on, with that risk.
    pub(super) fn wait_for_record(&self, id: DownloadId) {
        if !self.inner.store.wait_for_writes(RECORD_WAIT) {
            warn!(
                id = id.0,
                ?RECORD_WAIT,
                "the database did not save a download's record in time"
            );
        }
    }
}

/// Outlasts a busy database's usual retries; bounded so one that hangs only delays a run.
const RECORD_WAIT: Duration = Duration::from_secs(30);
const REMOVED: &str = "The download was removed before the server answered.";
/// Ends a run whose web page is read for its video instead (D51); never shown,
/// as the download is removed.
const READ_AS_VIDEO: &str = "The link is a web page, read for its video instead.";
/// Ends a run whose file is already in the folder (D88); never shown, as the download is
/// replaced, kept or asked about instead.
const FILE_EXISTS: &str = "The file is already in the folder.";
/// Ends a run whose server changed the type of the confirmed name (D166); never shown, as the
/// download is asked about instead.
const FILE_TYPE_CHANGED: &str = "The server changed the file's type.";

impl Manager {
    /// Stops run `run_id` of download `id` when the file at `path`, where it would save, seems to
    /// be the one the server offers (D88); `finish` then does what the setting says, on a row of the `category` the answer gives.
    fn stop_for_duplicate(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
        path: &Path,
        category: Category,
    ) -> Result<(), String> {
        let Some(found) = self.found_existing(id, remote, path) else {
            return Ok(());
        };
        let mut state = self.lock();
        current_run(&state, id, run_id)?;
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.offered_file = true;
            entry.row.category = category;
            if let Some(run) = entry.run.as_mut() {
                run.stop = Some(Stop::FileExists);
                run.found = Some(found);
            }
        }
        Err(FILE_EXISTS.into())
    }

    /// Stops run `run_id` of download `id` to ask about `server_name` (D166). `Ok` holds why the run ended.
    fn stop_for_file_type(
        &self,
        id: DownloadId,
        run_id: u64,
        server_name: String,
    ) -> Result<String, String> {
        let mut state = self.lock();
        current_run(&state, id, run_id)?;
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.offered_file = true;
            if let Some(run) = entry.run.as_mut() {
                run.stop = Some(Stop::FileTypeChanged);
                run.server_name = Some(server_name);
            }
        }
        Ok(FILE_TYPE_CHANGED.into())
    }

    /// Whether download `id`, added in the app, answered run `run_id` with a web
    /// page that the media add-on can read for its video (D51); if so its run is
    /// stopped for that, so the download is removed once it has ended and the
    /// page read instead (see `finish`).
    fn read_as_video(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
    ) -> Result<bool, String> {
        let mut state = self.lock();
        let installed = state.addon.tools.is_some();
        current_run(&state, id, run_id)?;
        let Some(entry) = state.downloads.get_mut(&id) else {
            return Err(REMOVED.into());
        };
        if !(entry.video_check && installed && is_web_page(remote)) {
            return Ok(false);
        }
        if let Some(run) = &mut entry.run {
            run.stop = Some(Stop::ReadAsVideo);
        }
        Ok(true)
    }
}

/// Runs `work`, which blocks on the disk, where a folder that does not answer (an unreachable share) stalls only the caller. The engine calls its resolver on the download's task and waits for the path, so `spawn_blocking` would not free the task; `block_in_place` frees the worker's other tasks. It panics on a current-thread runtime, which has no other worker to hand them to.
fn off_async_worker<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|handle| handle.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

/// The category of the file `name` the server answered with.
fn settled_category(state: &State, entry: &Entry, name: &str, remote: &ServerAnswer) -> Category {
    // The browser's word for a direct file: `talk.webm` may be only sound.
    if entry.row.media.is_some() && entry.row.category == Category::Audio {
        return Category::Audio;
    }
    let category = wirecatch_settings::category_of(&state.settings, name);
    match &remote.content_type {
        Some(content_type) if category == Category::Other => {
            wirecatch_settings::category_from_mime(content_type)
        }
        _ => category,
    }
}

/// Whether the server answered with a web page rather than a file.
fn is_web_page(remote: &ServerAnswer) -> bool {
    remote
        .content_type
        .as_deref()
        .is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/html"))
}

/// The download if run `run_id` is still its run and may still take a path.
fn current_run(state: &State, id: DownloadId, run_id: u64) -> Result<&Entry, String> {
    let entry = state
        .downloads
        .get(&id)
        .filter(|e| e.run.as_ref().is_some_and(|run| run.id == run_id))
        .ok_or(REMOVED)?;
    // A run that is stopping (paused, moved to another folder or queue, being
    // removed) must not take a path: whoever stopped it has already decided where
    // the download lives, and its files would be left behind.
    if is_stopping(entry) {
        return Err("The download was stopped before the server answered.".into());
    }
    Ok(entry)
}

/// Whether the server's answer is the file the browser saw. A different size, or
/// a web page where the browser had a file (often a sign-in page), means the app
/// did not get the same thing and must not take the download over.
fn check_expectation(expect: &Expectation, remote: &ServerAnswer) -> Result<(), String> {
    if let (Some(seen), Some(got)) = (expect.size, remote.total)
        && seen != got
    {
        return Err(format!(
            "The server sent a different file than the browser saw ({got} bytes instead of {seen}). Open the page again and refresh the link."
        ));
    }
    let is_page = is_web_page(remote);
    let page_expected = expect
        .name
        .as_deref()
        .and_then(|n| Path::new(n).extension())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"));
    if is_page && !page_expected {
        return Err(
            "The server sent a web page instead of the file, often a sign-in page. Open the page in your browser, sign in if needed, then refresh the link."
                .into(),
        );
    }
    Ok(())
}

/// Whether the download's run has been asked to stop, or it is being moved or removed.
fn is_stopping(entry: &Entry) -> bool {
    !entry.hold.is_free() || entry.run.as_ref().is_some_and(|run| run.stop.is_some())
}
