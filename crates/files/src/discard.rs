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
        for path in &files {
            test_trash::take(path);
        }
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

/// Whether `path` went to the trash.
fn trashed(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    if !home_trash::holds_its_drive(path) {
        warn!(path = %path.display(), "the file is not on the home trash's drive; it is left in place");
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

/// Whether a file shares the home trash's mount and device (D231). On another
/// mount `trash` picks the drive's own trash, which it accepts without checking
/// who owns it, or copies the file across drives to the home trash.
#[cfg(target_os = "linux")]
mod home_trash {
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Path, PathBuf};

    pub fn holds_its_drive(path: &Path) -> bool {
        let (Some(file), Some(trash)) = (
            path.parent()
                .and_then(existing)
                .zip(path.file_name())
                .map(|(parent, name)| parent.join(name)),
            location().and_then(|trash| existing(&trash)),
        ) else {
            return false;
        };
        let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
            return false;
        };
        let mounts = super::mount_points(&mounts);
        let device = |path: &Path| {
            std::fs::symlink_metadata(path)
                .map(|found| found.dev())
                .ok()
        };
        super::mount_of(&file, &mounts) == super::mount_of(&trash, &mounts)
            && device(&file).is_some()
            && device(&file) == trash.ancestors().find_map(device)
    }

    /// Where `trash` keeps the home trash.
    pub(super) fn location() -> Option<PathBuf> {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        set("XDG_DATA_HOME")
            .map(|data| PathBuf::from(data).join("Trash"))
            .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".local/share/Trash")))
    }

    /// `path` with its nearest existing folder resolved, as `trash` resolves it.
    fn existing(path: &Path) -> Option<PathBuf> {
        let found = path.ancestors().find(|folder| folder.exists())?;
        let rest = path.strip_prefix(found).ok()?;
        Some(std::fs::canonicalize(found).ok()?.join(rest))
    }
}

/// Every mount point `/proc/mounts` lists, its octal escapes read.
#[cfg(any(target_os = "linux", test))]
fn mount_points(table: &str) -> Vec<PathBuf> {
    table
        .lines()
        .filter_map(|line| line.split(' ').nth(1))
        .map(|field| {
            let mut point = String::new();
            let mut rest = field;
            while let Some(at) = rest.find('\\') {
                point.push_str(&rest[..at]);
                let code = rest
                    .get(at + 1..at + 4)
                    .and_then(|digits| u8::from_str_radix(digits, 8).ok());
                if let Some(byte) = code {
                    point.push(char::from(byte));
                    rest = &rest[at + 4..];
                } else {
                    point.push('\\');
                    rest = &rest[at + 1..];
                }
            }
            point.push_str(rest);
            PathBuf::from(point)
        })
        .collect()
}

/// The mount `path` is on: the longest mount point it lies under, as `trash` finds it.
#[cfg(any(target_os = "linux", test))]
fn mount_of<'a>(path: &Path, mounts: &'a [PathBuf]) -> Option<&'a Path> {
    mounts
        .iter()
        .filter(|point| path.starts_with(point))
        .max_by_key(|point| point.as_os_str().len())
        .map(PathBuf::as_path)
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
    fn a_file_is_on_the_longest_mount_point_above_it() {
        let table = "/dev/sda1 / ext4 rw 0 0\n/dev/sdb1 /mnt/my\\040drive ext4 rw 0 0\n/dev/sda2 /home ext4 rw 0 0\n";
        let mounts = mount_points(table);
        assert_eq!(
            mounts,
            [
                PathBuf::from("/"),
                PathBuf::from("/mnt/my drive"),
                PathBuf::from("/home")
            ]
        );
        let on = |path: &str| mount_of(Path::new(path), &mounts).map(Path::to_path_buf);
        assert_eq!(
            on("/home/a/.local/share/Trash"),
            Some(PathBuf::from("/home"))
        );
        assert_eq!(on("/home/a/Downloads/x.bin"), Some(PathBuf::from("/home")));
        assert_eq!(
            on("/mnt/my drive/x.bin"),
            Some(PathBuf::from("/mnt/my drive"))
        );
        assert_eq!(on("/homework/x.bin"), Some(PathBuf::from("/")));
    }

    #[test]
    fn a_file_moved_to_the_trash_can_be_restored_from_there() {
        test_trash::REAL.store(true, std::sync::atomic::Ordering::Release);
        let folder = beside_the_home_trash();
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

    /// A folder the trash takes from: on Linux on the home trash's drive, since
    /// `/tmp` is a mount of its own on many systems.
    fn beside_the_home_trash() -> tempfile::TempDir {
        #[cfg(target_os = "linux")]
        {
            let trash = home_trash::location().unwrap();
            let data = trash.parent().unwrap();
            std::fs::create_dir_all(data).unwrap();
            tempfile::tempdir_in(data).unwrap()
        }
        #[cfg(not(target_os = "linux"))]
        tempfile::tempdir().unwrap()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_on_another_drive_than_the_home_trash_is_kept_and_counted() {
        test_trash::REAL.store(true, std::sync::atomic::Ordering::Release);
        let mounts = mount_points(&std::fs::read_to_string("/proc/mounts").unwrap());
        let trash = home_trash::location().unwrap();
        let trash_mount = trash
            .ancestors()
            .find_map(|folder| std::fs::canonicalize(folder).ok())
            .and_then(|found| mount_of(&found, &mounts).map(Path::to_path_buf));
        let other = [std::env::temp_dir(), PathBuf::from("/dev/shm")]
            .into_iter()
            .filter_map(|folder| std::fs::canonicalize(folder).ok())
            .find(|folder| mount_of(folder, &mounts).map(Path::to_path_buf) != trash_mount)
            .and_then(|folder| tempfile::tempdir_in(folder).ok());
        let Some(folder) = other else {
            eprintln!("skipped: no writable folder on another mount than the home trash");
            return;
        };
        let file = folder.path().join("finished.bin");
        std::fs::write(&file, b"kept").unwrap();

        assert_eq!(to_trash(std::slice::from_ref(&file)), 1);
        assert!(file.exists(), "a file on another drive left its folder");
    }
}
