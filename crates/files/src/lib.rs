use std::path::{Path, PathBuf};

pub mod discard;
mod error;
pub mod owner_only;
pub mod whole_file;
pub mod zone;

pub use error::FileError;

/// Deletes the folder at `path` with everything in it. Only for a folder the app made: a path from a record, a setting or a person may be their own folder. Blocks on the disk.
///
/// # Errors
///
/// The folder or something in it cannot be deleted, or there is no such folder.
#[expect(
    clippy::disallowed_methods,
    reason = "this is the one call the app's other code goes through"
)]
pub fn remove_own_folder(path: &Path) -> std::io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `path` with `suffix` added to its file name, past any extension: `a.mp4` and `.part` give `a.mp4.part`.
#[must_use]
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
