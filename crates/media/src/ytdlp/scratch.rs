//! A temporary folder of its own for each run of yt-dlp, readable only by the current user and deleted with what is in it: yt-dlp's temporary files go there, and the PhantomJS stand-in lets a script touch only the files directly in it (D281).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{debug, warn};

static FOLDERS: AtomicU64 = AtomicU64::new(0);

pub(super) struct Scratch {
    path: PathBuf,
    /// Already deleted, so dropping it does nothing.
    deleted: bool,
}

impl Scratch {
    /// A new folder in `temp_dir`, which exists.
    pub(super) async fn create(temp_dir: &Path) -> std::io::Result<Self> {
        let n = FOLDERS.fetch_add(1, Ordering::Relaxed);
        // Short: yt-dlp's Windows build unpacks itself here, under paths of its own.
        let path = temp_dir.join(format!("r{}-{n}", std::process::id()));
        tokio::fs::create_dir(&path).await?;
        let scratch = Self {
            path,
            deleted: false,
        };
        let restricted = scratch.path.clone();
        wirecatch_engine::task::spawn_blocking(move || {
            wirecatch_files::owner_only::restrict(&restricted)
        })
        .await
        .map_err(std::io::Error::other)??;
        Ok(scratch)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Deletes the folder as soon as the run is done, off the async threads.
    pub(super) async fn delete(mut self) {
        let path = self.path.clone();
        let removed = wirecatch_engine::task::spawn_blocking(move || {
            wirecatch_files::remove_own_folder(&path)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        match removed {
            Ok(()) => self.deleted = true,
            // Left to `Drop`, which tries again and logs.
            Err(e) => {
                debug!(path = %self.path.display(), error = %e, "could not delete a run's temporary folder yet");
            }
        }
    }
}

/// Deleted when dropped: the fallback when a run is cancelled, panics or [`Scratch::delete`] could not delete it. Off the async threads when a runtime is there, since the folder may hold yt-dlp's unpacked build.
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.deleted {
            return;
        }
        let path = std::mem::take(&mut self.path);
        let remove = move || {
            if let Err(e) = wirecatch_files::remove_own_folder(&path) {
                warn!(path = %path.display(), error = %e, "could not delete a run's temporary folder");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(wirecatch_engine::task::spawn_blocking_on(&runtime, remove)),
            Err(_) => remove(),
        }
    }
}
