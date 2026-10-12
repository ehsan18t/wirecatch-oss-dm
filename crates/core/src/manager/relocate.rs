//! Moving a download to another folder: the capture toast's Change button and the
//! Choose folder fix.

use std::io;
use std::path::{Path, PathBuf};

use wirecatch_engine::CancellationToken;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, PortablePath};

use super::Manager;
use super::state::Stop;
use crate::error::{CoreError, Result};
use crate::naming::{self, partial_files};

impl Manager {
    /// Moves a download to `folder` (a full path or a `{Downloads}`-style path).
    ///
    /// A finished download's file is moved. An unfinished download's partial files
    /// move with it, so it continues where it was: a running one is stopped, moved
    /// and started again, and a failed one (for example because its old folder
    /// could not be written) goes back in line. The folder is created if needed and
    /// must be writable. If the name is taken there, ` (1)` and so on is added.
    pub async fn set_download_folder(&self, id: DownloadId, folder: &str) -> Result<()> {
        let dir = self
            .inner
            .folders
            .resolve(&PortablePath(folder.to_string()))?;
        let exited = {
            let mut state = self.lock_open()?;
            let entry = state.entry_mut(id)?;
            if entry.media_job.is_some() && entry.row.status != DownloadStatus::Completed {
                // Its parts live in work folders that are not moved along.
                return Err(CoreError::WrongState(
                    "A video can be moved to another folder once it has finished downloading. To save it somewhere else now, remove it and download it again.",
                ));
            }
            if entry.moving.is_some() {
                return Err(CoreError::WrongState(
                    "This download is already being moved. Wait a moment and try again.",
                ));
            }
            if entry.path().parent() == Some(dir.as_path()) {
                return Ok(());
            }
            entry.moving = Some(CancellationToken::new());
            let exited = entry.run.as_ref().map(|run| run.exited.clone());
            self.stop_run(entry, Stop::Requeue);
            exited
        };
        // The engine must close the files before they can be moved; bounded by the
        // stop timeout.
        if let Some(exited) = exited {
            exited.cancelled().await;
        }
        // Read only now: until the run had exited, the server's answer could still
        // have settled its path. Removal waits while a download is moving, so it
        // is still here.
        let old_path = self
            .lock()
            .downloads
            .get(&id)
            .map(|entry| entry.path().to_path_buf())
            .ok_or(CoreError::DownloadNotFound(id))?;

        let moved = if old_path.parent() == Some(dir.as_path()) {
            Ok(old_path.clone())
        } else {
            self.move_files(id, &old_path, dir.clone()).await
        };
        let mut state = self.lock();
        let Some(entry) = state.downloads.get_mut(&id) else {
            if let Ok(new_path) = &moved {
                state.claimed.release(new_path);
            }
            return moved.map(|_| ());
        };
        if let Some(done) = entry.moving.take() {
            done.cancel();
        }
        match moved {
            Ok(new_path) => {
                let new_name = file_name_of(&new_path);
                if !entry.naming.settled {
                    // The server has not answered yet, so its answer will choose the
                    // path: it must use the folder the user chose now, and the
                    // user's name as it is in that folder.
                    entry.naming.folder = Some(dir.to_string_lossy().into_owned());
                    if entry.naming.name.is_some() {
                        entry.naming.name = Some(new_name.clone());
                    }
                }
                entry.row.path = new_path.to_string_lossy().into_owned();
                entry.row.file_name = new_name;
                if entry.row.status == DownloadStatus::Failed {
                    entry.row.status = DownloadStatus::Queued;
                    entry.row.error = None;
                }
                // The new path was reserved while moving and is the download's
                // own now; the old one is free.
                if new_path != old_path {
                    state.claimed.remove(&old_path);
                    state.claimed.settle(&new_path);
                }
                self.save(&mut state, id);
                self.reconcile(&mut state);
                Ok(())
            }
            Err(e) => {
                self.reconcile(&mut state);
                Err(e)
            }
        }
    }

    /// Checks `dir`, reserves a free path in it and moves the download's files
    /// there. Returns the new path.
    async fn move_files(&self, id: DownloadId, old_path: &Path, dir: PathBuf) -> Result<PathBuf> {
        let name = old_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let claimed = self.lock().claimed.clone();
        let new_path = self
            .blocking(move || {
                prepare_folder(&dir)?;
                Ok(naming::unique_path(&dir, &name, &claimed))
            })
            .await?;
        let completed = {
            let mut state = self.lock();
            if state.claimed.contains(&new_path) {
                return Err(CoreError::WrongState(
                    "Another download just took that name in the new folder. Try again.",
                ));
            }
            let completed = state
                .downloads
                .get(&id)
                .is_some_and(|e| e.row.status == DownloadStatus::Completed);
            if !completed {
                state.claimed.reserve(&new_path);
            }
            completed
        };
        let (from, to) = (old_path.to_path_buf(), new_path.clone());
        let result = self
            .blocking(move || move_download_files(&from, &to, completed))
            .await;
        if result.is_err() && !completed {
            self.lock().claimed.release(&new_path);
        }
        result.map(|()| new_path)
    }
}

/// Creates `dir` if needed and proves it can be written to.
fn prepare_folder(dir: &Path) -> Result<()> {
    let unusable = |source: io::Error| CoreError::File {
        path: dir.display().to_string(),
        source,
    };
    std::fs::create_dir_all(dir).map_err(unusable)?;
    if !dir.is_dir() {
        return Err(unusable(io::ErrorKind::NotADirectory.into()));
    }
    let probe = dir.join(format!(".wirecatch-write-test-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(unusable)?;
    std::fs::remove_file(&probe).map_err(unusable)
}

/// Moves a finished file, or an unfinished download's part and resume files.
fn move_download_files(from: &Path, to: &Path, completed: bool) -> Result<()> {
    if completed {
        return move_file(from, to).map_err(|source| CoreError::File {
            path: from.display().to_string(),
            source,
        });
    }
    let pairs: Vec<(PathBuf, PathBuf)> = partial_files(from)
        .into_iter()
        .zip(partial_files(to))
        .filter(|(old, _)| old.exists())
        .collect();
    for (done, (old, new)) in pairs.iter().enumerate() {
        if let Err(source) = move_file(old, new) {
            // Put back what was already moved, so the download stays whole.
            for (old, new) in &pairs[..done] {
                let _ = move_file(new, old);
            }
            return Err(CoreError::File {
                path: new.display().to_string(),
                source,
            });
        }
    }
    Ok(())
}

/// Renames a file, copying it when the target is on another drive.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if let Ok(()) = std::fs::rename(from, to) {
        Ok(())
    } else {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_a_file_cannot_be_used() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(prepare_folder(&file).is_err());
        let fresh = dir.path().join("new").join("deeper");
        prepare_folder(&fresh).unwrap();
        assert!(fresh.is_dir());
        assert_eq!(
            std::fs::read_dir(&fresh).unwrap().count(),
            0,
            "probe left behind"
        );
    }

    #[test]
    fn partial_files_move_together() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.bin");
        let to = dir.path().join("b").join("a.bin");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        let [part, state] = partial_files(&from);
        std::fs::write(&part, b"data").unwrap();
        std::fs::write(&state, b"state").unwrap();
        move_download_files(&from, &to, false).unwrap();
        let [new_part, new_state] = partial_files(&to);
        assert_eq!(std::fs::read(new_part).unwrap(), b"data");
        assert_eq!(std::fs::read(new_state).unwrap(), b"state");
        assert!(!part.exists() && !state.exists());
    }
}
