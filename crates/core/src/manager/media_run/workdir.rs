//! A media download's work folder: the names of the files kept there, and the list of what was
//! downloaded, which a later run reads back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wirecatch_engine::Url;
use wirecatch_media::output::{ImageFormat, SubtitleFormat, SubtitlePick};
use wirecatch_sites::cookie_scope::site_of;
use wirecatch_sites::public_suffix::SuffixList;

use super::placement::link_digest;

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

/// The digest of the link the folder's contents came from (a token in a link stays out of the folder).
const LINK: &str = "link.digest";

/// Makes `work` the folder of `url`: what another link left there, or a folder that names no link, is cleared, so a name shared by two downloads never gives one the other's video.
pub(super) async fn claim_for_link(work: &Path, url: &Url) -> std::io::Result<()> {
    let digest = link_digest(url.as_str());
    let named = work.join(LINK);
    if tokio::fs::read_to_string(&named)
        .await
        .is_ok_and(|text| text == digest)
    {
        return Ok(());
    }
    let folder = work.to_owned();
    let removed = tokio::task::spawn_blocking(move || wirecatch_files::remove_own_folder(&folder))
        .await
        .map_err(std::io::Error::other)?;
    match removed {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    tokio::fs::create_dir_all(work).await?;
    tokio::fs::write(named, digest).await
}

/// Whose a stream's segments are: the link's site and port. A refreshed stream keeps them on any host of the site, while another site serving the same paths never takes them.
pub(super) fn stream_owner(url: &Url, suffixes: &SuffixList) -> String {
    let site = site_of(url.as_str(), suffixes).unwrap_or_default();
    format!("{site}:{}", url.port_or_known_default().unwrap_or_default())
}

/// The formats a download took when its saved ones were gone, written here so its next attempt keeps what this one finished.
const CHOSEN_AGAIN: &str = "chosen-again.txt";

/// Makes `work` the folder of formats `ids`, chosen again because the page stopped listing the saved ones: parts of any others are cleared, since a part's marker does not say which format it holds.
pub(super) async fn claim_for_formats(work: &Path, url: &Url, ids: &str) -> std::io::Result<()> {
    let named = work.join(CHOSEN_AGAIN);
    if tokio::fs::read_to_string(&named)
        .await
        .is_ok_and(|text| text == ids)
    {
        return Ok(());
    }
    tokio::fs::remove_dir_all(work).await?;
    claim_for_link(work, url).await?;
    tokio::fs::write(named, ids).await
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
