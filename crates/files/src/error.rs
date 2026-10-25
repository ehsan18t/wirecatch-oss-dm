use std::io;
use std::path::Path;

/// A file or folder that could not be read, written or created.
#[derive(Debug, thiserror::Error)]
#[error(
    "Could not use {path} ({source}). Check that the folder exists and that you are allowed to write there."
)]
pub struct FileError {
    pub path: String,
    pub source: io::Error,
}

impl FileError {
    #[must_use]
    pub fn new(path: &Path, source: io::Error) -> Self {
        Self {
            path: path.display().to_string(),
            source,
        }
    }
}
