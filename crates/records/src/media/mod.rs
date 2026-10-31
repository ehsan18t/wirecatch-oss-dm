//! What a media download downloads, as it is saved with the download.

pub mod output;

use serde::{Deserialize, Serialize};
use wirecatch_protocol::app::MediaSummary;

use output::OutputJob;

/// Which track of a stream to take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Pick {
    /// The best the stream offers.
    Auto,
    Track(String),
    Skip,
}

/// What a media download downloads, stored with it so it can continue after a
/// restart. The link itself is the download's URL: the playlist or manifest for
/// a stream, the page for yt-dlp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaJob {
    /// An HLS or DASH stream, downloaded with `wirecatch-streams`.
    Stream { video: Pick, audio: Pick },
    /// A page read by yt-dlp. Its format addresses expire, so the page is read
    /// again on every start; the chosen formats are found again by id. yt-dlp's
    /// selection is made from the ids ([`FormatId::selector`]); a `format` field
    /// stored by earlier builds is ignored when read.
    Page {
        /// The video format id, when the choice names one.
        video: Option<FormatId>,
        /// The audio format id, when separate from the video.
        audio: Option<FormatId>,
        /// How the downloaded video or audio is saved (D24); `None` keeps it
        /// as downloaded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Box<OutputJob>>,
        /// The extension the joined video gets when it is kept as downloaded
        /// after a conversion failed ("Keep the original"); `None` takes the
        /// planned file's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_ext: Option<String>,
        /// The last run downloaded the parts but could not convert them, so
        /// "Keep the original" may save them as they came (D24).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        conversion_failed: bool,
    },
    /// What to download was stored but cannot be read back (a damaged
    /// database). It never runs: downloading the link as a plain file would
    /// save a web page, or worse.
    Unreadable,
}

impl MediaJob {
    /// Records whether the last run of a page job ended with its parts downloaded but not converted.
    pub fn set_conversion_failed(&mut self, failed: bool) {
        if let MediaJob::Page {
            conversion_failed, ..
        } = self
        {
            *conversion_failed = failed;
        }
    }
}

/// The message for a media download whose stored details cannot be read.
#[must_use]
pub fn unreadable() -> wirecatch_protocol::app::UserError {
    wirecatch_protocol::app::UserError {
        message: "The saved details of this video could not be read, so it cannot continue. Remove it and download the video again from its page.".into(),
        fix: None,
    }
}

/// The media part of a stored download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMedia {
    pub summary: MediaSummary,
    /// `None` for a direct media file, which downloads like any other file.
    pub job: Option<MediaJob>,
}

/// The id of a page's format, which a `-f` selection names it by. Sites choose these ids, so only letters, digits and `_ . - =` are accepted: a format with any other id is left out. A selection names an id in an exact, quoted filter ([`selector`](Self::selector)), since yt-dlp reads a bare `b`, `ba`, `best.2`, `mp4` and the like as a keyword or an extension before a format id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FormatId(String);

/// The longest format id accepted; yt-dlp's are a few dozen characters at most.
const MAX_FORMAT_ID: usize = 100;

/// A format id with a character yt-dlp's selections give a meaning to, or none at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a format id Wirecatch passes to yt-dlp")]
pub struct InvalidFormatId(String);

impl TryFrom<String> for FormatId {
    type Error = InvalidFormatId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        let plain = !id.is_empty()
            && id.len() <= MAX_FORMAT_ID
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'='));
        if plain {
            Ok(FormatId(id))
        } else {
            Err(InvalidFormatId(id))
        }
    }
}

impl From<FormatId> for String {
    fn from(id: FormatId) -> Self {
        id.0
    }
}

impl FormatId {
    /// The selection of exactly this format: `b*` picks the one format the filter leaves, video, audio or both. The id holds no quote or backslash, so it cannot end the quoted value.
    #[must_use]
    pub fn selector(&self) -> String {
        format!(r#"b*[format_id="{}"]"#, self.0)
    }
}

impl std::fmt::Display for FormatId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_is_stored_as_plain_json() {
        let id = |id: &str| FormatId::try_from(id.to_string()).unwrap();
        let job = MediaJob::Page {
            video: Some(id("137")),
            audio: Some(id("140")),
            output: None,
            keep_ext: None,
            conversion_failed: false,
        };
        let json = serde_json::to_string(&job).unwrap();
        // The selection is made from the ids, never stored.
        assert_eq!(json, r#"{"type":"page","video":"137","audio":"140"}"#);
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), job);
        let converting = MediaJob::Page {
            video: Some(id("313")),
            audio: Some(id("140")),
            output: Some(Box::new(OutputJob {
                container: output::Container::Mp4,
                video: output::VideoAction::Encode {
                    codec: output::VideoCodec::H264,
                    kbps: 8000,
                },
                audio: output::AudioAction::Copy,
                subtitles: vec![output::SubtitlePick {
                    key: "en-orig".into(),
                    auto: true,
                    language: "en".into(),
                    name: "English".into(),
                }],
                subtitle_files: false,
                extras: output::Extras {
                    chapters: true,
                    tags: false,
                },
                duration_ms: Some(1000),
                original_ext: "webm".into(),
                original: false,
            })),
            keep_ext: None,
            conversion_failed: false,
        };
        let json = serde_json::to_string(&converting).unwrap();
        // The extras are stored as fields of the job, as jobs saved before were.
        assert!(
            json.contains(r#""subtitle_files":false,"chapters":true,"tags":false,"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), converting);
        let unconverted = MediaJob::Page {
            video: Some(id("137")),
            audio: None,
            output: None,
            keep_ext: None,
            conversion_failed: true,
        };
        let json = serde_json::to_string(&unconverted).unwrap();
        assert!(json.ends_with(r#""conversion_failed":true}"#), "{json}");
        assert_eq!(
            serde_json::from_str::<MediaJob>(&json).unwrap(),
            unconverted
        );
        let job = MediaJob::Stream {
            video: Pick::Track("v1".into()),
            audio: Pick::Skip,
        };
        let json = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), job);
    }

    #[test]
    fn format_ids_that_mean_something_in_a_selection_are_refused() {
        for id in ["137", "hls-1080p", "dash-video=4000000", "en_US.0"] {
            assert!(FormatId::try_from(id.to_string()).is_ok(), "{id}");
        }
        for id in [
            "",
            "137+140",
            "best/worst",
            "a,b",
            "bv[height<=720]",
            "x y",
            "(a)",
            "a*",
        ] {
            assert!(FormatId::try_from(id.to_string()).is_err(), "{id}");
        }
    }
}
