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

/// Moves each file of `paths` to the Recycle Bin, or to the trash on Linux. A
/// folder is left alone, and a file that cannot go there stays where it is
/// (D231). Blocks on the disk.
pub fn to_trash(paths: &[PathBuf]) {
    let files = paths.iter().filter(|path| is_file(path));
    #[cfg(any(test, feature = "testing"))]
    if !test_trash::REAL.load(std::sync::atomic::Ordering::Acquire) {
        files.for_each(|path| test_trash::take(path));
        return;
    }
    // A thread of its own: `trash` panics when its thread is already in COM's multithreaded apartment.
    let moved = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("trash".into())
            .spawn_scoped(scope, || files.for_each(|path| trashed(path)))
            .map(|_| ())
    });
    if let Err(e) = moved {
        warn!(error = %e, "could not start moving files to the trash; they are left in place");
    }
}

/// Whether `path` is there and is not a folder: by now a folder may have taken a file's name.
fn is_file(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(found) => !found.is_dir(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "could not read a file to move to the trash; it is left in place");
            false
        }
    }
}

fn trashed(path: &Path) {
    if let Err(e) = trash::delete(path) {
        warn!(path = %path.display(), error = %e, "could not move a file to the trash; it is left in place");
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

        to_trash(&[
            file.clone(),
            folder.path().join("never-there.bin"),
            taken_name.clone(),
        ]);

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
