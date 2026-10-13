//! How a downloaded video or audio is saved, stored with the download so a conversion that was paused or failed runs again by itself (D24).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use wirecatch_protocol::app::StreamAction;

use super::{AudioCodec, Container, VideoCodec};

/// What happens to the video of a download being saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum VideoAction {
    /// There is no video.
    None,
    Copy,
    /// Re-encoded to `codec` at about `kbps`.
    Encode {
        codec: VideoCodec,
        kbps: u32,
    },
}

impl VideoAction {
    /// What the quality window is told about it, without the codec and bit rate.
    pub fn stream(self) -> StreamAction {
        match self {
            VideoAction::None => StreamAction::None,
            VideoAction::Copy => StreamAction::Copy,
            VideoAction::Encode { .. } => StreamAction::Encode,
        }
    }
}

/// What happens to the audio of a download being saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AudioAction {
    /// There is no audio.
    None,
    Copy,
    /// Re-encoded to `codec`, at `kbps` for a lossy codec.
    Encode {
        codec: AudioCodec,
        kbps: Option<u32>,
    },
}

impl AudioAction {
    /// What the quality window is told about it, without the codec and bit rate.
    pub fn stream(self) -> StreamAction {
        match self {
            AudioAction::None => StreamAction::None,
            AudioAction::Copy => StreamAction::Copy,
            AudioAction::Encode { .. } => StreamAction::Encode,
        }
    }
}

/// One subtitle language chosen for a download, found again by its key when the
/// page is read again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SubtitlePick {
    /// yt-dlp's key for it, for example `en` or `en-orig`.
    pub key: String,
    /// From the auto-generated list.
    pub auto: bool,
    /// The language code, for example `en`.
    pub language: String,
    /// The language in words, for the track's name.
    pub name: String,
}

/// How a downloaded video or audio is saved, stored with the download so a
/// conversion that was paused or failed runs again by itself (D24). A download
/// without one is saved as it was downloaded, as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OutputJob {
    pub container: Container,
    pub video: VideoAction,
    pub audio: AudioAction,
    #[serde(default)]
    pub subtitles: Vec<SubtitlePick>,
    /// Subtitles go next to the file as `.srt` files instead of inside it.
    #[serde(default)]
    pub subtitle_files: bool,
    /// What else goes into the file. Stored as fields of the job itself, as
    /// jobs saved before were.
    #[serde(flatten)]
    pub extras: Extras,
    /// The media's length, for the conversion's progress.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// The extension of the file as downloaded, for "Keep the original".
    pub original_ext: String,
    /// Saved in the format it was downloaded in ("Original"): ffmpeg only runs
    /// when something goes inside the file.
    #[serde(default)]
    pub original: bool,
}

/// What goes into a saved file besides its streams, where its format holds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Extras {
    /// The video's chapters.
    #[serde(default)]
    pub chapters: bool,
    /// Title, artist and cover picture.
    #[serde(default)]
    pub tags: bool,
}

impl OutputJob {
    /// Whether ffmpeg writes the file. Otherwise the download is kept as it is
    /// (joined, if it came as separate video and audio), and only subtitle
    /// files are added next to it.
    pub fn needs_pass(&self) -> bool {
        !self.original
            || self.converts()
            || self.embedded_subtitles()
            || self.wants_metadata()
            || self.wants_cover()
    }

    /// Whether anything is re-encoded.
    pub fn converts(&self) -> bool {
        matches!(self.video, VideoAction::Encode { .. })
            || matches!(self.audio, AudioAction::Encode { .. })
    }

    /// Whether the subtitles are saved as files next to the download.
    pub fn sidecar_subtitles(&self) -> bool {
        !self.subtitles.is_empty()
            && (self.subtitle_files || self.container.subtitle_codec().is_none())
    }

    /// Whether the subtitles go inside the file.
    pub fn embedded_subtitles(&self) -> bool {
        !self.subtitles.is_empty() && !self.sidecar_subtitles()
    }

    /// Whether a metadata file (tags, chapters) is written for ffmpeg.
    pub fn wants_metadata(&self) -> bool {
        (self.extras.chapters && self.container.chapters())
            || (self.extras.tags && self.container.tags())
    }

    /// Whether a cover picture is downloaded for the file.
    pub fn wants_cover(&self) -> bool {
        self.extras.tags && self.container.cover()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::output::test_support::*;
    use crate::media::output::*;

    #[test]
    fn the_original_format_runs_ffmpeg_only_when_something_goes_inside() {
        let mut j = job(Container::Webm, VideoAction::Copy, AudioAction::Copy);
        j.original = true;
        assert!(!j.needs_pass());
        j.subtitles = vec![pick("en", false)];
        assert!(j.needs_pass(), "subtitles go inside a WebM");
        j.subtitle_files = true;
        assert!(!j.needs_pass(), "subtitles go next to it");
        j.extras.chapters = true;
        assert!(j.needs_pass());
        j.original = false;
        j.extras.chapters = false;
        assert!(j.needs_pass(), "another format always runs ffmpeg");
    }
}
