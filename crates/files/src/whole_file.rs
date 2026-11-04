//! Writing a file so that a crash or a power cut leaves the old one or the new
//! one, never a part of either.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Writes `bytes` to `name` in `dir`, which is made if missing: first to a file
/// beside it, flushed to the disk, which then replaces `name` in one step. When
/// any step fails, `name` is left as it was. Blocks on the disk.
pub fn replace(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_then_rename(&dir.join(format!("{name}.tmp")), &dir.join(name), bytes)
}

/// Writes `bytes` to a file a person chose at `path`, as [`replace`] does. A link at `path` keeps pointing at the file, which is what gets replaced. On Unix the file keeps its permissions, and a new one is its owner's alone. The file beside it is new, under a name of Wirecatch's own, since a person's folder may hold anything, and it goes again when a step fails. Blocks on the disk.
pub fn replace_chosen(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let (staged, file) = create_staged(&target)?;
    let written = fill(file, bytes);
    #[cfg(unix)]
    let written = written.and_then(|()| keep_access(&target, &staged));
    let written = written.and_then(|()| rename_into_place(&staged, &target));
    if written.is_err() {
        crate::discard::file(&staged);
    }
    written
}

/// After the name of a file being written beside the file a person chose, before a number.
const CHOSEN_STAGED_SUFFIX: &str = ".wirecatch-tmp-";

/// A new file beside `path`. Made new, so nothing there is written through, a link included.
fn create_staged(path: &Path) -> io::Result<(PathBuf, File)> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut n = 0u64;
    loop {
        let staged = crate::with_suffix(path, &format!("{CHOSEN_STAGED_SUFFIX}{n}"));
        match options.open(&staged) {
            Ok(file) => return Ok((staged, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Gives `staged` the permissions of the file it will replace, if there is one.
#[cfg(unix)]
fn keep_access(target: &Path, staged: &Path) -> io::Result<()> {
    match std::fs::metadata(target) {
        Ok(found) => std::fs::set_permissions(staged, found.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn fill(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn write_then_rename(staged: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    fill(File::create(staged)?, bytes)?;
    rename_into_place(staged, path)
}

fn rename_into_place(staged: &Path, path: &Path) -> io::Result<()> {
    std::fs::rename(staged, path)?;
    // The rename reaches the disk with its folder on Unix. Windows has no handle for a folder to flush, and records the rename with the file.
    #[cfg(unix)]
    {
        let dir = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// Windows' `ERROR_SHARING_VIOLATION`.
#[cfg(windows)]
const SHARING_VIOLATION: i32 = 32;

/// Puts what was written to the file at `path` on the disk, however it got there: a program that wrote it may have left it in memory. A file another program holds without sharing its writing (a scanner, on Windows) cannot be flushed from here, which is no failure. Blocks on the file system.
///
/// # Errors
///
/// When the file cannot be opened or flushed.
pub fn flush(path: &Path) -> io::Result<()> {
    // Write access: Windows refuses to flush a read-only handle.
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => file.sync_all(),
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(SHARING_VIOLATION) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::private_dir;

    #[test]
    fn a_missing_file_cannot_be_flushed() {
        let dir = private_dir("flush-missing").unwrap();
        assert!(flush(&dir.path().join("missing")).is_err());
        let there = dir.path().join("there");
        std::fs::write(&there, b"x").unwrap();
        flush(&there).unwrap();
    }

    #[test]
    fn a_chosen_file_is_replaced_and_nothing_stays_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chosen.json");
        let neighbour = dir.path().join("chosen.json.tmp");
        std::fs::write(&path, b"old").unwrap();
        std::fs::write(&neighbour, b"theirs").unwrap();
        replace_chosen(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(&neighbour).unwrap(), b"theirs");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_file_already_under_the_staged_name_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chosen.json");
        let taken = dir.path().join("chosen.json.wirecatch-tmp-0");
        std::fs::write(&taken, b"theirs").unwrap();
        replace_chosen(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(&taken).unwrap(), b"theirs");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_keeps_pointing_at_the_file_that_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        let link = dir.path().join("chosen.json");
        std::fs::write(&real, b"old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        replace_chosen(&link, b"new").unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_under_the_staged_name_is_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::write(&elsewhere, b"theirs").unwrap();
        let path = dir.path().join("chosen.json");
        std::os::unix::fs::symlink(&elsewhere, dir.path().join("chosen.json.wirecatch-tmp-0"))
            .unwrap();
        replace_chosen(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&elsewhere).unwrap(), b"theirs");
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn a_chosen_file_keeps_its_permissions_and_a_new_one_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chosen.json");
        replace_chosen(&path, b"first").unwrap();
        assert_eq!(mode(&path), 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        replace_chosen(&path, b"second").unwrap();
        assert_eq!(mode(&path), 0o640);
    }

    #[test]
    fn a_failed_replace_of_a_chosen_file_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a folder");
        std::fs::create_dir(&path).unwrap();
        replace_chosen(&path, b"new").unwrap_err();
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_replace_leaves_the_file_as_it_was() {
        let dir = private_dir("whole-file-fail").unwrap();
        replace(dir.path(), "kept", b"old").unwrap();
        // The file a new copy is written to first cannot be made.
        std::fs::create_dir(dir.path().join("kept.tmp")).unwrap();
        replace(dir.path(), "kept", b"new").unwrap_err();
        assert_eq!(std::fs::read(dir.path().join("kept")).unwrap(), b"old");
    }
}
