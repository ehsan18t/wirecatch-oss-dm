//! Choosing a download's final path from the server's answer, on its first run.
//!
//! Until the server answers, a download's name and folder are a guess made when it
//! was added (from the user's choice, the browser's suggestion or the link). The
//! engine calls the resolver built here right after the server answers with the
//! file and before anything is written, so the name can come from the server's
//! `Content-Disposition` and the folder from the real file type, without an extra
//! request. The chosen path is saved at once; later runs use it as is.

use std::path::PathBuf;
use std::sync::Arc;

use wirecatch_engine::{CancellationToken, PathResolver, RemoteFile};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::Category;

use super::Manager;
use super::state::Expectation;
use crate::{naming, settings};

impl Manager {
    /// The resolver for run `run_id` of download `id`. On success it cancels
    /// `answered`, which is how a waiting capture learns the server answered.
    pub(super) fn path_resolver(
        &self,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
    ) -> PathResolver {
        let manager = Arc::downgrade(&self.inner);
        Arc::new(move |remote: &RemoteFile| {
            let manager =
                Manager::from_weak(&manager).ok_or_else(|| "Wirecatch is closing.".to_string())?;
            let path = manager.settle_path(id, run_id, remote)?;
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
        remote: &RemoteFile,
    ) -> Result<PathBuf, String> {
        let mut guard = self.lock();
        let state = &mut *guard;
        let Some(entry) = state
            .downloads
            .get_mut(&id)
            .filter(|e| e.run.as_ref().is_some_and(|run| run.id == run_id))
        else {
            return Err("The download was removed before the server answered.".into());
        };
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

        // The provisional name is given up first, so the download may keep it.
        state.claimed.remove(entry.path());
        let path = naming::unique_path(&dir, &name, &state.claimed);
        state.claimed.insert(&path);
        entry.row.path = path.to_string_lossy().into_owned();
        entry.row.file_name = naming_of(&path);
        entry.row.category = category;
        entry.naming.settled = true;
        entry.expect = None;
        self.save(state, id);
        Ok(path)
    }
}

/// Whether the server's answer is the file the browser saw. A different size, or
/// a web page where the browser had a file (often a sign-in page), means the app
/// did not get the same thing and must not take the download over.
fn check_expectation(expect: &Expectation, remote: &RemoteFile) -> Result<(), String> {
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

fn naming_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
