use std::path::{Path, PathBuf};

pub mod discard;
pub mod owner_only;
pub mod whole_file;
pub mod zone;

/// `path` with `suffix` added to its file name, past any extension: `a.mp4` and `.part` give `a.mp4.part`.
#[must_use]
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
