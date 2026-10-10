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

use wirecatch_engine::{CancellationToken, PathResolver, RemoteFile, Url};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::Category;

use super::Manager;
use super::state::{Entry, Expectation, State};
use crate::{naming, settings};

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

/// Chooses a download's path from the server's answer; `Err` refuses the file.
pub(super) type Settle = Arc<dyn Fn(&ServerAnswer) -> Result<PathBuf, String> + Send + Sync>;

/// The engine's resolver for `settle`.
pub(super) fn path_resolver(settle: Settle) -> PathResolver {
    Arc::new(move |remote: &RemoteFile| settle(&ServerAnswer::from(remote)))
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
            let manager =
                Manager::from_weak(&manager).ok_or_else(|| "Wirecatch is closing.".to_string())?;
            let path = manager.settle_path(id, run_id, answer)?;
            answered.cancel();
            Ok(path)
        })
    }

    /// Picks the final path for the server's file, claims it, and saves it. Runs on
    /// the download's task with the manager's lock held only briefly; the only disk
    /// access is checking whether names are free.
    fn settle_path(
        &self,
        id: DownloadId,
        run_id: u64,
        remote: &ServerAnswer,
    ) -> Result<PathBuf, String> {
        self.settle_path_checking(id, run_id, remote, &naming::is_free_on_disk)
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
        let (dir, name, category, provisional) = {
            let state = self.lock();
            let entry = current_run(&state, id, run_id)?;
            if let Some(expect) = &entry.expect {
                check_expectation(expect, remote)?;
            }
            let name = naming::settled_name(
                entry.naming.name.as_deref(),
                &remote.suggested_name,
                &remote.final_url,
                &entry.row.file_name,
            )
            .map_err(|e| e.to_string())?;
            let mut category = settings::category_of(&state.settings, &name);
            if category == Category::Other
                && let Some(content_type) = &remote.content_type
            {
                category = settings::category_from_mime(content_type);
            }
            let dir = match &entry.naming.folder {
                Some(folder) => PathBuf::from(folder),
                None => settings::folder_for(&state.settings, category, &self.inner.folders)
                    .map_err(|e| e.to_string())?,
            };
            (dir, name, category, entry.path().to_path_buf())
        };

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
                } else if state.claimed.contains(&candidate) {
                    continue;
                } else {
                    state.claimed.insert(&candidate);
                    true
                }
            };
            if free_on_disk(&candidate) {
                break (candidate, claimed_here);
            }
            if claimed_here {
                self.lock().claimed.remove(&candidate);
            }
        };

        let mut guard = self.lock();
        let state = &mut *guard;
        // The run may have been stopped while the disk was checked.
        if let Err(reason) = current_run(state, id, run_id) {
            if claimed_here {
                state.claimed.remove(&path);
            }
            return Err(reason);
        }
        if !naming::same_path(&path, &provisional) {
            state.claimed.remove(&provisional);
        }
        let Some(entry) = state.downloads.get_mut(&id) else {
            return Err(REMOVED.into());
        };
        entry.row.path = path.to_string_lossy().into_owned();
        entry.row.file_name = naming_of(&path);
        entry.row.category = category;
        entry.naming.settled = true;
        entry.expect = None;
        self.save(state, id);
        Ok(path)
    }
}

const REMOVED: &str = "The download was removed before the server answered.";

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
    let is_page = remote
        .content_type
        .as_deref()
        .is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/html"));
    let page_expected = expect.name.as_deref().is_some_and(|n| {
        let n = n.to_ascii_lowercase();
        n.ends_with(".html") || n.ends_with(".htm")
    });
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
    entry.removing
        || entry.moving.is_some()
        || entry.run.as_ref().is_some_and(|run| run.stop.is_some())
}

fn naming_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
