//! Writing a file so that a crash or a power cut leaves the old one or the new
//! one, never a part of either.

use std::io::{self, Write};
use std::path::Path;

/// Writes `bytes` to `name` in `dir`, which is made if missing: first to a file
/// beside it, flushed to the disk, which then replaces `name` in one step. When
/// any step fails, `name` is left as it was. Blocks on the disk.
pub fn replace(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let staged = dir.join(format!("{name}.tmp"));
    let mut file = std::fs::File::create(&staged)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&staged, dir.join(name))?;
    // The rename reaches the disk with its folder on Unix. Windows has no handle for a folder to flush, and records the rename with the file.
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::private_dir;

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
