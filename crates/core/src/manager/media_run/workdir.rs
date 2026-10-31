//! A media download's work folder: the names of the files kept there, and the list of what was
//! downloaded, which a later run reads back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wirecatch_media::output::{ImageFormat, SubtitleFormat, SubtitlePick};

pub(super) const COVER: &str = "cover.image";
pub(super) const METADATA: &str = "metadata.txt";
/// Kept for an answer about a taken name, so the next run places the file without downloading it again:
/// the link's digest (a token in a link stays out of the folder), the file, then each subtitle and its language.
pub(super) const PLACING: &str = "placing.done";

/// A subtitle as the site sent it.
pub(super) fn subtitle(n: usize, ext: &str) -> String {
    format!("sub-{n}.{ext}")
}

/// A subtitle converted for a file saved next to the video.
pub(super) fn converted_subtitle(n: usize) -> String {
    format!("subtitle-{n}.srt")
}

/// The file the last ffmpeg pass writes.
pub(super) fn output(ext: &str) -> String {
    format!("output.{ext}")
}

/// The video and audio joined into one file.
pub(super) fn joined(ext: &str) -> String {
    format!("joined.{ext}")
}

/// Names the file a finished part (`role`) was saved as.
pub(super) fn marker(role: &str) -> String {
    format!("{role}.done")
}

/// The list of what a page's video downloaded into its work folder, written
/// once everything is there, so a conversion that is paused or fails starts
/// again from these files without reading the page or downloading anything.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Downloaded {
    /// The video and the separate audio, or the one file.
    pub(super) media: Vec<String>,
    #[serde(default)]
    pub(super) subtitles: Vec<DownloadedSubtitle>,
    #[serde(default)]
    pub(super) cover: Option<(String, ImageFormat)>,
    /// The ffmpeg metadata file (tags, chapters).
    #[serde(default)]
    pub(super) metadata: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct DownloadedSubtitle {
    pub(super) file: String,
    pub(super) format: SubtitleFormat,
    pub(super) pick: SubtitlePick,
}

/// The list's name in the work folder.
pub(super) const DOWNLOADED: &str = "downloaded.json";

impl Downloaded {
    pub(super) fn of_media(files: &[PathBuf]) -> Self {
        Self {
            media: files
                .iter()
                .filter_map(|f| f.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect(),
            ..Self::default()
        }
    }

    /// Every file name in the list.
    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.media
            .iter()
            .map(String::as_str)
            .chain(self.subtitles.iter().map(|s| s.file.as_str()))
            .chain(self.cover.iter().map(|c| c.0.as_str()))
            .chain(self.metadata.iter().map(String::as_str))
    }

    /// The list in `work`, with the size of the media files, when it is there
    /// and every file it names is a file straight inside `work`.
    pub(super) async fn read(work: &Path) -> Option<(Downloaded, u64)> {
        let text = tokio::fs::read(work.join(DOWNLOADED)).await.ok()?;
        let list: Downloaded = serde_json::from_slice(&text).ok()?;
        if list.media.is_empty() || !(1..=2).contains(&list.media.len()) {
            return None;
        }
        for name in list.names() {
            if !plain_name(name) {
                return None;
            }
            let meta = tokio::fs::symlink_metadata(work.join(name)).await.ok()?;
            if !meta.is_file() {
                return None;
            }
        }
        let mut bytes = 0;
        for name in &list.media {
            bytes += tokio::fs::metadata(work.join(name)).await.ok()?.len();
        }
        Some((list, bytes))
    }

    pub(super) async fn write(&self, work: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        tokio::fs::write(work.join(DOWNLOADED), json).await
    }
}

/// A file name with no folder in it, nor anything Windows reads as a stream or
/// a drive.
pub(super) fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}
