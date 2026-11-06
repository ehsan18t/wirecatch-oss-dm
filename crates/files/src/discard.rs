//! Deleting files and folders the app is done with, and moving a person's
//! files to the trash. A failure is logged and stops nothing: what is left
//! behind takes disk space but the app never reads it again.

use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

/// Deletes the file at `path`, if there is one. Blocks on the disk.
pub fn file(path: &Path) {
    logged(path, std::fs::remove_file(path));
}

/// Deletes the folder at `path` with everything in it, if there is one. Only
/// for a folder the app made, as [`crate::remove_own_folder`] says. Blocks on
/// the disk.
pub fn dir(path: &Path) {
    logged(path, crate::remove_own_folder(path));
}

/// Moves each file of `paths` to the Recycle Bin, or to the trash on Linux, and
/// answers how many files stayed where they were because they could not go
/// there (D231). A folder or a missing path is left alone and not counted.
/// Blocks on the disk.
#[must_use]
pub fn to_trash(paths: &[PathBuf]) -> usize {
    let mut left = 0;
    let mut files = Vec::new();
    for path in paths {
        match std::fs::symlink_metadata(path) {
            // By now a folder may have taken a file's name.
            Ok(found) if found.is_dir() => {}
            Ok(_) => files.push(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                warn!(path = %path.display(), error = %e, "could not read a file to move to the trash; it is left in place");
                left += 1;
            }
        }
    }
    #[cfg(any(test, feature = "testing"))]
    if !test_trash::REAL.load(std::sync::atomic::Ordering::Acquire) {
        files.iter().for_each(|path| test_trash::take(path));
        return left;
    }
    // A thread of its own: `trash` panics when its thread is already in COM's multithreaded apartment.
    let moved = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("trash".into())
            .spawn_scoped(scope, || files.iter().filter(|path| !trashed(path)).count())
            .map(|spawned| spawned.join().unwrap_or(files.len()))
    });
    match moved {
        Ok(refused) => left + refused,
        Err(e) => {
            warn!(error = %e, "could not start moving files to the trash; they are left in place");
            left + files.len()
        }
    }
}

/// Whether `path` went to the trash; a file the trash cannot take beside it stays.
fn trashed(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    if !beside::has_trash_beside(path) {
        warn!(path = %path.display(), "no trash on the file's own drive; it is left in place");
        return false;
    }
    match trash::delete(path) {
        Ok(()) => true,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "could not move a file to the trash; it is left in place");
            false
        }
    }
}

/// Whether a file has a trash on its own device. Across devices `trash` copies the
/// file and then deletes it, which takes long for a large file and leaves half a
/// copy when it fails.
#[cfg(target_os = "linux")]
mod beside {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    use std::path::{Path, PathBuf};

    pub fn has_trash_beside(path: &Path) -> bool {
        let Some(path) = path
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
        else {
            return false;
        };
        let Ok(file) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        let device = file.dev();
        if home_trash().is_some_and(|trash| device_of_nearest(&trash) == Some(device)) {
            return true;
        }
        // `trash` uses the drive's own trash, `.Trash-<uid>` at its top, before the home one.
        let (Some(top), Ok(me)) = (drive_top(&path, device), std::fs::metadata("/proc/self"))
        else {
            return false;
        };
        let own = top.join(format!(".Trash-{}", me.uid()));
        let mut folder = std::fs::DirBuilder::new();
        folder.recursive(true).mode(0o700);
        ["files", "info"]
            .iter()
            .all(|part| folder.create(own.join(part)).is_ok())
            && device_of_nearest(&own) == Some(device)
    }

    /// Where `trash` keeps the home trash.
    fn home_trash() -> Option<PathBuf> {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        set("XDG_DATA_HOME")
            .map(|data| PathBuf::from(data).join("Trash"))
            .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".local/share/Trash")))
    }

    fn device_of_nearest(path: &Path) -> Option<u64> {
        path.ancestors()
            .find_map(|folder| std::fs::metadata(folder).ok())
            .map(|found| found.dev())
    }

    /// The highest folder above `path` still on `device`: the top of its drive.
    fn drive_top(path: &Path, device: u64) -> Option<PathBuf> {
        path.ancestors()
            .skip(1)
            .take_while(|folder| std::fs::metadata(folder).is_ok_and(|found| found.dev() == device))
            .last()
            .map(Path::to_path_buf)
    }
}

/// What tests use instead of the trash, which is the person's own: a file is deleted and noted.
#[cfg(any(test, feature = "testing"))]
pub mod test_trash {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Mutex, PoisonError};

    pub(super) static REAL: AtomicBool = AtomicBool::new(false);
    static TAKEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn take(path: &Path) {
        super::file(path);
        TAKEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_path_buf());
    }

    /// Whether [`super::to_trash`] took `path`.
    #[must_use]
    pub fn took(path: &Path) -> bool {
        TAKEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|taken| taken == path)
    }
}

fn logged(path: &Path, deleted: io::Result<()>) {
    match deleted {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            warn!(path = %path.display(), error = %e, "could not delete a file or folder");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn a_file_moved_to_the_trash_can_be_restored_from_there() {
        test_trash::REAL.store(true, std::sync::atomic::Ordering::Release);
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("finished.bin");
        std::fs::write(&file, b"kept").unwrap();
        let taken_name = folder.path().join("taken-by-a-folder");
        std::fs::create_dir(&taken_name).unwrap();

        let left = to_trash(&[
            file.clone(),
            folder.path().join("never-there.bin"),
            taken_name.clone(),
        ]);

        assert_eq!(left, 0);

        assert!(!file.exists());
        assert!(taken_name.is_dir(), "a folder went to the trash");
        let folder_name = folder.path().file_name().unwrap();
        let ours: Vec<_> = trash::os_limited::list()
            .unwrap()
            .into_iter()
            .filter(|item| {
                item.name == OsStr::new("finished.bin")
                    && item.original_parent.ends_with(folder_name)
            })
            .collect();
        assert_eq!(ours.len(), 1, "the file is not in the trash");
        trash::os_limited::purge_all(ours).unwrap();
    }
}
