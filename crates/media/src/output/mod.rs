//! Output formats for media read by yt-dlp (D24): which formats a quality can be
//! saved as, whether that copies the streams or re-encodes them, the encoders to
//! re-encode with, the subtitles and chapters a video has, and ffmpeg's
//! arguments for all of it. Nothing here runs a program; see
//! [`convert`](super::convert) for that.

#![forbid(unsafe_code)]

mod arguments;
mod codec;
mod encoders;
mod metadata;
mod subtitles;
mod variants;

pub use wirecatch_records::media::output::*;

pub use arguments::*;
pub(crate) use codec::*;
pub(crate) use encoders::*;
pub use metadata::*;
pub use subtitles::*;
pub(crate) use variants::*;

#[cfg(test)]
pub(super) mod test_support {
    //! What the tests of these modules share.

    use std::ffi::OsString;

    use super::*;
    use crate::ytdlp::{Format, Info};

    pub(crate) fn format(json: serde_json::Value) -> Format {
        serde_json::from_value(json).unwrap()
    }

    /// A format as yt-dlp describes one of a YouTube page's.
    pub(crate) fn listed(
        id: &str,
        ext: &str,
        codecs: (&str, &str),
        height: Option<u32>,
        fps: f64,
        tbr: f64,
    ) -> Format {
        format(serde_json::json!({
            "format_id": id, "ext": ext, "vcodec": codecs.0, "acodec": codecs.1,
            "height": height, "fps": fps, "tbr": tbr, "url": "u", "filesize": 1000
        }))
    }

    pub(crate) fn info(json: serde_json::Value) -> Info {
        serde_json::from_value(json).unwrap()
    }

    pub(crate) fn job(container: Container, video: VideoAction, audio: AudioAction) -> OutputJob {
        OutputJob {
            container,
            video,
            audio,
            subtitles: Vec::new(),
            subtitle_files: false,
            extras: Extras::default(),
            duration_ms: None,
            original_ext: "webm".into(),
            original: false,
        }
    }

    pub(crate) fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    pub(crate) fn pick(language: &str, auto: bool) -> SubtitlePick {
        SubtitlePick {
            key: language.into(),
            auto,
            language: language.into(),
            name: "English".into(),
        }
    }
}
