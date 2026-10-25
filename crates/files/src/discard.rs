//! Deleting files and folders the app is done with. A failure is logged and
//! stops nothing: what is left behind takes disk space but the app never reads
//! it again.

use std::io;
use std::path::Path;

use tracing::warn;

/// Deletes the file at `path`, if there is one. Blocks on the disk.
pub fn file(path: &Path) {
    logged(path, std::fs::remove_file(path));
}

/// Deletes the folder at `path` with everything in it, if there is one. Blocks
/// on the disk.
pub fn dir(path: &Path) {
    logged(path, std::fs::remove_dir_all(path));
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
