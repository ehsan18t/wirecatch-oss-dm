//! The files a download has besides its own path, and the move of them under way.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One file of a move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedFile {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// A move of a download's files, on disk before its first file moves, so the next start can tell how far a kill let it get (D132).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveUnderWay {
    /// The download's path once moved.
    pub path: PathBuf,
    /// In the order they move: the largest first, so what a kill leaves to do is small.
    pub files: Vec<MovedFile>,
}

/// The partial files of a download being removed, on record until they are gone, so a kill during the removal cannot strand them with no row (D134).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDeletion {
    /// The download's path, which the partial files are beside.
    pub path: PathBuf,
    /// A video or audio download, which also has work folders.
    pub media: bool,
    /// The download's path once moved, while a move runs or a failed one left files there (D171, D179).
    #[serde(default)]
    pub moving_to: Option<PathBuf>,
    /// What moves, for the other finished rows of the file, which follow it (D179).
    #[serde(default)]
    pub moving_files: Vec<MovedFile>,
    /// A finished download has no partial files, so only what its move leaves goes.
    #[serde(default)]
    pub finished: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadFiles {
    /// Subtitle files beside a finished video.
    pub sidecars: Vec<PathBuf>,
    pub moving: Option<MoveUnderWay>,
}

impl DownloadFiles {
    /// The subtitle files among `moved` are at their new paths.
    pub fn follow(&mut self, moved: &[MovedFile]) {
        for sidecar in &mut self.sidecars {
            if let Some(file) = moved.iter().find(|file| file.from == *sidecar) {
                sidecar.clone_from(&file.to);
            }
        }
    }
}
