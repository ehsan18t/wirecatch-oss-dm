//! Writing a file so that a crash or a power cut leaves the old one or the new
//! one, never a part of either.

use std::io::{self, Write};
use std::path::Path;

/// Writes `bytes` to `name` in `dir`, which is made if missing: first to a file
/// beside it, flushed to the disk, which then replaces `name` in one step. When
/// any step fails, `name` is left as it was. Blocks on the disk.
pub fn replace(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_then_rename(&dir.join(format!("{name}.tmp")), &dir.join(name), bytes)
}

/// Writes `bytes` to a file a person chose at `path`, as [`replace`] does. The file beside it has a name of Wirecatch's own, since a person's folder may hold anything, and it goes again when a step fails. Blocks on the disk.
pub fn replace_chosen(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let staged = crate::with_suffix(path, CHOSEN_STAGED_SUFFIX);
    let written = write_then_rename(&staged, path, bytes);
    if written.is_err() {
        crate::discard::file(&staged);
    }
    written
}

/// After the name of a file being written beside the file a person chose.
const CHOSEN_STAGED_SUFFIX: &str = ".wirecatch-tmp";

fn write_then_rename(staged: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::create(staged)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
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
