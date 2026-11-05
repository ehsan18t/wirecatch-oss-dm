//! Moving a download to another folder: the capture toast's Change button and the
//! Choose folder fix.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;
use wirecatch_engine::CancellationToken;
use wirecatch_files::FileError;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, PortablePath, UserError};
use wirecatch_records::files::{MoveUnderWay, MovedFile};

use super::Manager;
use super::file_exists::give_up_the_name;
use super::state::{Entry, State, Stop};
use crate::error::{CoreError, Result};
use crate::naming::{self, partial_files_and_temp};

/// Starts the name of a copy on its way to its final name.
const STAGED_PREFIX: &str = ".wirecatch-move-";

impl Manager {
    /// Moves a download to `folder` (a full path or a `{Downloads}`-style path).
    ///
    /// A finished download's file is moved with its subtitle files. An unfinished download's partial files move with it, so it continues where it was: a running one is stopped, moved and started again, and a failed one (for example because its old folder could not be written) goes back in line. The folder is created if needed and must be writable. If the name is taken there, ` (1)` and so on is added.
    ///
    /// # Errors
    ///
    /// [`CoreError::Path`] when `folder` does not resolve, [`CoreError::File`] when it cannot be created or written to or the files cannot be moved there, [`CoreError::FilesSplit`] when, after that, some files already moved could not be put back (the download is then marked failed with that message and waits until it is tried again), [`CoreError::WrongState`] for an unfinished video, a download already being moved or one whose files an earlier move left split, [`CoreError::DownloadNotFound`] when there is no download `id`, and [`CoreError::Closed`] once the manager has shut down.
    ///
    /// Runs to the end even when the caller stops waiting.
    pub async fn set_download_folder(&self, id: DownloadId, folder: &str) -> Result<()> {
        let dir = self
            .inner
            .folders
            .resolve(&PortablePath(folder.to_string()))?;
        let exited = {
            let mut state = self.lock_open()?;
            let entry = state.entry_mut(id)?;
            entry.refuse_if_stranded()?;
            if entry.media_job.is_some() && entry.row.status != DownloadStatus::Completed {
                // Its parts live in work folders that are not moved along.
                return Err(CoreError::WrongState(
                    "A video can be moved to another folder once it has finished downloading. To save it somewhere else now, remove it and download it again.",
                ));
            }
            if !entry.hold.is_free() {
                return Err(CoreError::WrongState(
                    "This download is already being moved. Wait a moment and try again.",
                ));
            }
            if entry.path().parent() == Some(dir.as_path()) {
                if !entry.naming.settled {
                    entry.naming.folder = Some(dir.to_string_lossy().into_owned());
                    self.save(&mut state, id);
                }
                return Ok(());
            }
            entry.hold.begin_move();
            let exited = entry.run.as_ref().map(|run| run.exited.clone());
            self.stop_run(entry, Stop::Requeue);
            self.close_questions_of(&mut state, id);
            exited
        };
        let manager = self.clone();
        self.to_the_end(async move { manager.finish_moving(id, dir, exited).await })
            .await
    }

    /// Moves download `id`, already marked as moving, to `dir` once its run has exited.
    async fn finish_moving(
        &self,
        id: DownloadId,
        dir: PathBuf,
        exited: Option<CancellationToken>,
    ) -> Result<()> {
        // The engine must close the files before they can be moved. The stop timeout does not
        // bound this: `exited` fires only once the run's file work, its marks included, has ended.
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
        self.finish_move(id, &dir, &old_path, moved)
    }

    /// Records how moving download `id` from `old_path` to `dir` ended.
    pub(super) fn finish_move(
        &self,
        id: DownloadId,
        dir: &Path,
        old_path: &Path,
        moved: Result<PathBuf>,
    ) -> Result<()> {
        let mut state = self.lock();
        let Some(entry) = state.downloads.get_mut(&id) else {
            if let Ok(new_path) = &moved {
                state.claimed.release(new_path, id);
            }
            return moved.map(|_| ());
        };
        entry.hold.end_move();
        let record = entry.files.moving.take();
        match moved {
            Ok(new_path) => {
                let finished = entry.row.status == DownloadStatus::Completed;
                let cancelled = entry.row.status == DownloadStatus::Cancelled;
                let files = record.map_or_else(Vec::new, |record| record.files);
                take_moved_path(entry, dir, &new_path);
                entry.files.follow(&files);
                // The new path was reserved while moving and is the download's
                // own now; the old one is free.
                if new_path != old_path {
                    state.claimed.remove(old_path, id);
                    if !finished {
                        state.claimed.insert(&new_path, id);
                    }
                }
                if finished {
                    self.move_rows_of_one_file(&mut state, id, old_path, &new_path, &files);
                }
                if cancelled {
                    give_up_the_name(&mut state, id);
                }
                self.save(&mut state, id);
                self.reconcile(&mut state);
                Ok(())
            }
            Err(e) => {
                if matches!(e, CoreError::FilesSplit { .. }) {
                    // Its part and resume files are split between two folders: it must
                    // not resume until the person has put them back.
                    entry.row.status = DownloadStatus::Failed;
                    entry.row.error = Some(UserError {
                        message: e.to_string(),
                        fix: None,
                    });
                    self.save(&mut state, id);
                } else if record.is_some() {
                    self.save(&mut state, id);
                }
                self.reconcile(&mut state);
                Err(e)
            }
        }
    }

    /// The file of finished download `id` went from `old_path` to `new_path`: the other finished rows of that file go with it (D118).
    fn move_rows_of_one_file(
        &self,
        state: &mut State,
        id: DownloadId,
        old_path: &Path,
        new_path: &Path,
        files: &[MovedFile],
    ) {
        let others: Vec<DownloadId> = state
            .downloads
            .values()
            .filter(|e| {
                e.row.id != id
                    && e.row.status == DownloadStatus::Completed
                    && naming::same_path(e.path(), old_path)
            })
            .map(|e| e.row.id)
            .collect();
        for other in others {
            if let Some(entry) = state.downloads.get_mut(&other) {
                entry.row.path = new_path.to_string_lossy().into_owned();
                entry.row.file_name = naming::file_name_of(new_path);
                entry.files.follow(files);
            }
            self.save(state, other);
        }
    }

    /// Checks `dir`, reserves a free path in it and moves the download's files
    /// there. Returns the new path.
    pub(super) async fn move_files(
        &self,
        id: DownloadId,
        old_path: &Path,
        dir: PathBuf,
    ) -> Result<PathBuf> {
        let name = self
            .lock()
            .downloads
            .get(&id)
            .and_then(|entry| entry.plain_name.clone())
            .unwrap_or_else(|| naming::file_name_of(old_path));
        let manager = self.clone();
        let new_path = self
            .blocking(move || {
                prepare_folder(&dir)?;
                Ok(naming::unique_path(&dir, &name, |path| {
                    manager.lock().claimed.contains(path)
                }))
            })
            .await?;
        let (completed, sidecars) = {
            let mut state = self.lock();
            let completed = state
                .downloads
                .get(&id)
                .is_some_and(|e| e.row.status == DownloadStatus::Completed);
            let sidecars = if completed {
                sidecars_of_file(&state, old_path)
            } else {
                Vec::new()
            };
            // A finished file holds its name on disk, with no claim.
            let free = if completed {
                !state.claimed.contains(&new_path)
            } else {
                state.claimed.reserve(&new_path, id)
            };
            if !free {
                return Err(CoreError::WrongState(
                    "Another download just took that name in the new folder. Try again.",
                ));
            }
            (completed, sidecars)
        };
        let (from, to) = (old_path.to_path_buf(), new_path.clone());
        let manager = self.clone();
        #[cfg(test)]
        let mover: Mover = self.fake_mover().unwrap_or_else(|| Arc::new(move_file));
        #[cfg(not(test))]
        let mover: Mover = Arc::new(move_file);
        let result = self
            .blocking(move || {
                manager.move_on_record(id, (&from, &to), completed, &sidecars, &*mover)
            })
            .await;
        if let Err(e) = &result {
            let mut state = self.lock();
            if matches!(e, CoreError::FilesSplit { .. }) {
                // Some files are still at the new path: it stays reserved for them.
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.stranded = Some(new_path.clone());
                }
            } else if !completed {
                state.claimed.release(&new_path, id);
            }
        }
        result.map(|()| new_path)
    }

    /// Records and moves the files of download `id` as it goes `between` two paths. Blocks on the disk and the database.
    fn move_on_record(
        &self,
        id: DownloadId,
        between: (&Path, &Path),
        completed: bool,
        sidecars: &[PathBuf],
        move_one: &(dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync),
    ) -> Result<()> {
        let (from, to) = between;
        let files = files_to_move(from, to, completed, sidecars, |sidecar| {
            self.reserve_sidecar_target(id, sidecar, from, to)
        });
        self.record_move(id, to, &files);
        let pairs: Vec<(PathBuf, PathBuf)> = files
            .iter()
            .map(|file| (file.from.clone(), file.to.clone()))
            .collect();
        let result = move_together(&pairs, move_one);
        // A finished file holds its name on disk, with no claim.
        if completed {
            let mut state = self.lock();
            for file in files.iter().skip(1) {
                state.claimed.release(&file.to, id);
            }
        }
        result
    }

    /// Picks and reserves where `sidecar` of download `id` goes when its file moves from `from` to `to`. Touches the file system.
    fn reserve_sidecar_target(
        &self,
        id: DownloadId,
        sidecar: &Path,
        from: &Path,
        to: &Path,
    ) -> PathBuf {
        loop {
            let target =
                sidecar_target(sidecar, from, to, |path| self.lock().claimed.contains(path));
            if self.lock().claimed.reserve(&target, id) {
                return target;
            }
        }
    }

    /// Puts the record of a move of download `id` to `to` on disk. Blocks on the database.
    fn record_move(&self, id: DownloadId, to: &Path, files: &[MovedFile]) {
        let mut guard = self.lock();
        let state = &mut *guard;
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.files.moving = Some(MoveUnderWay {
                path: to.to_path_buf(),
                files: files.to_vec(),
            });
            // Its row is off the saved list, so a save would not reach the disk (D171).
            if entry.hold.is_removing() {
                let deletion = entry.pending_deletion();
                state.pending_deletions.insert(id, deletion);
                self.save_pending_deletions(state);
            }
            self.save(state, id);
        }
        drop(guard);
        self.wait_for_record(id);
    }
}

/// The subtitle files listed by the finished rows of the file at `path`. A listed name may since be another download's, or another finished row's file; those stay.
fn sidecars_of_file(state: &State, path: &Path) -> Vec<PathBuf> {
    let finished = || {
        state
            .downloads
            .values()
            .filter(|e| e.row.status == DownloadStatus::Completed)
    };
    finished()
        .filter(|e| naming::same_path(e.path(), path))
        .flat_map(|e| e.files.sidecars.iter().cloned())
        .filter(|sidecar| {
            !state.claimed.contains(sidecar)
                && !finished().any(|e| naming::same_path(e.path(), sidecar))
        })
        .collect()
}

/// The download's path and name are `new_path`, in `dir`.
fn take_moved_path(entry: &mut Entry, dir: &Path, new_path: &Path) {
    let new_name = entry
        .plain_name
        .clone()
        .unwrap_or_else(|| naming::file_name_of(new_path));
    if !entry.naming.settled {
        // The server has not answered yet, so its answer will choose the path: it must use the folder the user chose now, and the user's name as it is in that folder.
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
}

/// Finishes the move a kill interrupted, as far as its `record` and the disk tell (D132). Once a file has reached its new place the others follow, being the small ones; before that the download stays where it is. Blocks on the disk.
pub(super) fn finish_interrupted_move(entry: &mut Entry, record: &MoveUnderWay) {
    let dir = record.path.parent().unwrap_or(Path::new(""));
    discard_staged(dir);
    if !record.files.iter().any(has_arrived) {
        return;
    }
    match finish_files(&record.files) {
        // A finished file is whole at its new place; only a subtitle file can have stayed.
        Some((file, source)) if entry.row.status != DownloadStatus::Completed => {
            let stranded: Vec<String> = record
                .files
                .iter()
                .filter(|file| file.to.exists())
                .map(|file| {
                    format!(
                        "{} (it belongs at {})",
                        file.to.display(),
                        file.from.display()
                    )
                })
                .collect();
            let split = CoreError::FilesSplit {
                path: file.from.display().to_string(),
                source,
                stuck: stranded.join(", "),
            };
            entry.row.status = DownloadStatus::Failed;
            entry.row.error = Some(UserError {
                message: split.to_string(),
                fix: None,
            });
            entry.stranded = Some(record.path.clone());
        }
        _ => {
            take_moved_path(entry, dir, &record.path);
            entry.files.follow(&files_that_arrived(&record.files));
        }
    }
}

/// Finishes the move of a finished row a removal took off the list, so one copy of each file is left (D171). Blocks on the disk.
pub(super) fn finish_removed_move(files: &[MovedFile]) {
    if files.iter().any(has_arrived) {
        _ = finish_files(files);
    }
}

/// Takes each of `files` to its new place. What cannot go is logged; returns the first of those.
fn finish_files(files: &[MovedFile]) -> Option<(&MovedFile, io::Error)> {
    let mut stuck = None;
    for file in files {
        let moved = if has_arrived(file) {
            match std::fs::remove_file(&file.from) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else if file.from.exists() {
            move_file(&file.from, &file.to)
        } else {
            Ok(())
        };
        if let Err(source) = moved {
            warn!(from = %file.from.display(), to = %file.to.display(), error = %source, "could not finish a move a kill cut short");
            stuck.get_or_insert((file, source));
        }
    }
    stuck
}

/// Those of `files` now only at their new place. A file at the new place of one that could not follow is someone else's, and must not be listed as the download's.
pub(super) fn files_that_arrived(files: &[MovedFile]) -> Vec<MovedFile> {
    files
        .iter()
        .filter(|file| !file.from.exists() && file.to.exists())
        .cloned()
        .collect()
}

/// Whether `file` is whole at its new place: a copy takes its final name only when whole, and one of another size is some other program's.
fn has_arrived(file: &MovedFile) -> bool {
    let size = |path: &Path| std::fs::metadata(path).map(|meta| meta.len());
    match (size(&file.from), size(&file.to)) {
        (Err(_), Ok(_)) => true,
        (Ok(from), Ok(to)) => from == to,
        (_, Err(_)) => false,
    }
}

/// Deletes the copies under way that a kill left in `dir`. No move runs yet at a start, so none of them is in use.
pub(super) fn discard_staged(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGED_PREFIX)
        {
            wirecatch_files::discard::file(&entry.path());
        }
    }
}

/// The files that move when a download goes from `from` to `to`, in the order they move, each of `sidecars` to where `sidecar_to` says. Touches the file system.
fn files_to_move(
    from: &Path,
    to: &Path,
    completed: bool,
    sidecars: &[PathBuf],
    sidecar_to: impl Fn(&Path) -> PathBuf,
) -> Vec<MovedFile> {
    if !completed {
        return partial_files_and_temp(from)
            .into_iter()
            .zip(partial_files_and_temp(to))
            .filter(|(old, _)| old.exists())
            .map(|(from, to)| MovedFile { from, to })
            .collect();
    }
    let mut files = vec![MovedFile {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
    }];
    files.extend(
        sidecars
            .iter()
            .filter(|sidecar| sidecar.exists())
            .map(|sidecar| MovedFile {
                from: sidecar.clone(),
                to: sidecar_to(sidecar),
            }),
    );
    files
}

/// Where `sidecar` of the file at `from` goes when that file moves to `to`: its name follows the file's, and is free there.
fn sidecar_target(
    sidecar: &Path,
    from: &Path,
    to: &Path,
    claimed: impl Fn(&Path) -> bool,
) -> PathBuf {
    let stem = |path: &Path| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let name = naming::file_name_of(sidecar);
    let name = name
        .strip_prefix(&stem(from))
        .map_or_else(|| name.clone(), |rest| format!("{}{rest}", stem(to)));
    naming::unique_path(to.parent().unwrap_or(Path::new("")), &name, claimed)
}

/// Moves one file from the first path to the second.
type Mover = Arc<dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync>;

/// Creates `dir` if needed and proves it can be written to.
fn prepare_folder(dir: &Path) -> Result<()> {
    let unusable = |source: io::Error| CoreError::File(FileError::new(dir, source));
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

/// Moves each `(old, new)` pair of a download's files with `move_one`. When one
/// cannot be moved, those already moved are put back, so the download stays
/// whole in its old folder. A file that cannot be put back either is logged,
/// and the error says the download's files are split between the two folders.
fn move_together(
    pairs: &[(PathBuf, PathBuf)],
    mut move_one: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<()> {
    for (done, (old, new)) in pairs.iter().enumerate() {
        if let Err(source) = move_one(old, new) {
            let mut stuck = Vec::new();
            for (old, new) in &pairs[..done] {
                if let Err(error) = move_one(new, old) {
                    warn!(from = %new.display(), to = %old.display(), %error, "could not move a download's file back after a failed move");
                    stuck.push(format!(
                        "{} (it belongs at {})",
                        new.display(),
                        old.display()
                    ));
                }
            }
            return Err(if stuck.is_empty() {
                CoreError::File(FileError::new(new, source))
            } else {
                CoreError::FilesSplit {
                    path: old.display().to_string(),
                    source,
                    stuck: stuck.join(", "),
                }
            });
        }
    }
    Ok(())
}

/// Moves a file to `to`, never over a file there (CORE-9): renamed in one step where it can be, else copied.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match wirecatch_engine::naming::rename_no_replace(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        // Another drive, or a file that cannot be renamed.
        Err(_) => move_by_copy(from, to, &|source, copy| io::copy(source, copy).map(|_| ())),
    }
}

/// Copies one open file to another.
type CopyBytes<'a> = dyn Fn(&mut std::fs::File, &mut std::fs::File) -> io::Result<()> + 'a;

/// Copies `from` under a temporary name beside `to` and renames it there when whole, so a file under the final name is always a whole one, even after a kill. When the original cannot be deleted (open in another program, say), the copy goes again, so the file is never left in two places.
fn move_by_copy(from: &Path, to: &Path, copy_bytes: &CopyBytes<'_>) -> io::Result<()> {
    let mut source = std::fs::File::open(from)?;
    let (staged, mut copy) = create_staged(to)?;
    let whole = copy_bytes(&mut source, &mut copy)
        .and_then(|()| copy.sync_all())
        .and_then(|()| {
            drop(copy);
            wirecatch_files::zone::copy_mark(from, &staged);
            wirecatch_engine::naming::rename_no_replace(&staged, to)
        });
    if let Err(e) = whole {
        discard(&staged);
        return Err(e);
    }
    drop(source);
    std::fs::remove_file(from).inspect_err(|_| discard(to))
}

/// A new temporary file in the folder of `to`. Short, so a name near the system's limit still moves, and its own, so two moves to one folder never share it.
fn create_staged(to: &Path) -> io::Result<(PathBuf, std::fs::File)> {
    let dir = to.parent().unwrap_or(Path::new(""));
    let mut n = 0u64;
    loop {
        let staged = dir.join(format!("{STAGED_PREFIX}{n}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => return Ok((staged, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Deletes the copy `move_file` made, which must not stay beside the original.
fn discard(copy: &Path) {
    if let Err(error) = std::fs::remove_file(copy) {
        warn!(path = %copy.display(), %error, "could not delete the copy of a file that could not be moved");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CORE-9: a move never replaces a file at its target, which a download took its free name
    /// before another program saved there.
    #[test]
    fn a_move_never_replaces_a_file_at_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.zip"), dir.path().join("b.zip"));
        std::fs::write(&from, b"downloaded").unwrap();
        std::fs::write(&to, b"someone else's").unwrap();
        let error = move_file(&from, &to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(&from).unwrap(), b"downloaded");
    }

    /// CORE-9: a file that cannot be taken from where it is (open in another program without
    /// sharing) stays there alone; no copy is left at the target.
    #[cfg(windows)]
    #[test]
    fn a_file_held_open_is_not_copied_and_left_twice() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.zip"), dir.path().join("b.zip"));
        std::fs::write(&from, b"downloaded").unwrap();
        // Shared for reading only: it can be copied, but neither renamed nor deleted.
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&from)
            .unwrap();
        assert!(move_file(&from, &to).is_err());
        assert!(!to.exists(), "a second copy was left at the target");
        assert!(from.exists());
        assert_eq!(names_in(dir.path()), ["a.zip"]);
    }

    fn names_in(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_copy_takes_its_final_name_only_when_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.zip"), dir.path().join("b.zip"));
        std::fs::write(&from, b"downloaded").unwrap();
        move_by_copy(&from, &to, &|source, copy| {
            // Where a kill would leave it.
            io::copy(source, copy)?;
            assert!(
                !to.exists(),
                "the final name appeared before the copy was whole"
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"downloaded");
        assert_eq!(names_in(dir.path()), ["b.zip"]);
    }

    #[test]
    fn a_copy_that_fails_leaves_only_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.zip"), dir.path().join("b.zip"));
        std::fs::write(&from, b"downloaded").unwrap();
        let error = move_by_copy(&from, &to, &|_, copy| {
            use std::io::Write;
            copy.write_all(b"down")?;
            Err(io::ErrorKind::StorageFull.into())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert_eq!(names_in(dir.path()), ["a.zip"]);
    }

    #[test]
    fn a_copy_never_replaces_a_file_saved_at_its_target_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.zip"), dir.path().join("b.zip"));
        std::fs::write(&from, b"downloaded").unwrap();
        std::fs::write(&to, b"someone else's").unwrap();
        let error = move_by_copy(&from, &to, &|source, copy| {
            io::copy(source, copy).map(|_| ())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(&from).unwrap(), b"downloaded");
        let mut names = names_in(dir.path());
        names.sort();
        assert_eq!(names, ["a.zip", "b.zip"]);
    }

    #[test]
    fn two_copies_to_one_folder_each_get_their_own_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("b.zip");
        let (one, _held) = create_staged(&to).unwrap();
        let (two, _also_held) = create_staged(&to).unwrap();
        assert_ne!(one, two);
    }

    #[cfg(windows)]
    #[test]
    fn a_copy_keeps_the_mark_of_the_web() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("setup.exe"), dir.path().join("moved.exe"));
        std::fs::write(&from, b"MZ").unwrap();
        wirecatch_files::zone::mark(&from, "https://example.com/setup.exe", None);
        let stream = |path: &Path| format!("{}:Zone.Identifier", path.display());
        let mark = std::fs::read(stream(&from)).unwrap();
        move_by_copy(&from, &to, &|source, copy| {
            io::copy(source, copy).map(|_| ())
        })
        .unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"MZ");
        assert_eq!(std::fs::read(stream(&to)).unwrap(), mark);
    }

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
        let [part, state] = naming::partial_files(&from);
        std::fs::write(&part, b"data").unwrap();
        std::fs::write(&state, b"state").unwrap();
        let pairs: Vec<(PathBuf, PathBuf)> =
            files_to_move(&from, &to, false, &[], Path::to_path_buf)
                .into_iter()
                .map(|file| (file.from, file.to))
                .collect();
        move_together(&pairs, move_file).unwrap();
        let [new_part, new_state] = naming::partial_files(&to);
        assert_eq!(std::fs::read(new_part).unwrap(), b"data");
        assert_eq!(std::fs::read(new_state).unwrap(), b"state");
        assert!(!part.exists() && !state.exists());
    }

    /// A file that cannot be moved back after a failed move leaves the download
    /// split between two folders: the error says so and names the new folder.
    #[test]
    fn a_file_that_cannot_be_moved_back_is_reported() {
        let old = Path::new("old");
        let new = Path::new("new");
        let pairs = [
            (old.join("a.bin.part"), new.join("a (1).bin.part")),
            (old.join("a.bin.wcstate"), new.join("a (1).bin.wcstate")),
        ];
        let mut calls = 0;
        let error = move_together(&pairs, |_, _| {
            calls += 1;
            // The first move works; the second and the move back fail.
            if calls == 1 {
                Ok(())
            } else {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        })
        .unwrap_err();
        assert_eq!(calls, 3);
        let message = error.to_string();
        assert!(matches!(error, CoreError::FilesSplit { .. }), "{message}");
        // The file that failed, then each stranded file and the name it must have.
        let failed = format!("Could not move {} ", old.join("a.bin.wcstate").display());
        assert!(message.starts_with(&failed), "{message}");
        let stuck = format!(
            "{} (it belongs at {})",
            new.join("a (1).bin.part").display(),
            old.join("a.bin.part").display()
        );
        assert!(message.contains(&stuck), "{message}");
    }
}
