//! What yt-dlp says about a page: its formats, subtitles and chapters, and the format ids a selection names them by.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;
use wirecatch_engine::numeric::f64_to_u64;
use wirecatch_engine::units::{GIB, KIB};
use wirecatch_protocol::Secret;

/// What yt-dlp says about a page (the fields Wirecatch reads).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Info {
    #[serde(default)]
    pub title: Option<String>,
    /// The video's id on its site, so two links to one video are known to be the same.
    #[serde(default)]
    pub id: Option<String>,
    /// The extractor that read the page, for example `Youtube`; ids are unique per extractor.
    #[serde(default)]
    pub extractor_key: Option<String>,
    #[serde(default)]
    pub is_live: Option<bool>,
    #[serde(default)]
    pub live_status: Option<String>,
    /// Every format that reads; one that does not is left out alone.
    #[serde(default, deserialize_with = "readable_formats")]
    pub formats: Vec<Format>,
    /// For a page with one format, yt-dlp may describe it at the top level.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub format_id: Option<String>,
    /// Length in seconds.
    #[serde(default, deserialize_with = "lenient")]
    pub duration: Option<f64>,
    /// The picture yt-dlp names as the video's thumbnail.
    #[serde(default, deserialize_with = "lenient")]
    pub thumbnail: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub uploader: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub channel: Option<String>,
    /// For music, the performer.
    #[serde(default, deserialize_with = "lenient")]
    pub artist: Option<String>,
    /// For music, the track's title.
    #[serde(default, deserialize_with = "lenient")]
    pub track: Option<String>,
    /// Subtitles written by people, by language.
    #[serde(default, deserialize_with = "lenient")]
    pub subtitles: HashMap<String, Vec<SubtitleFile>>,
    /// Subtitles made by speech recognition (and machine translations of them), by language.
    #[serde(default, deserialize_with = "lenient")]
    pub automatic_captions: HashMap<String, Vec<SubtitleFile>>,
    #[serde(default, deserialize_with = "lenient")]
    pub chapters: Vec<Chapter>,
}

/// Reads a field that yt-dlp may leave out, set to `null` or fill with something
/// unexpected: anything that does not read as `T` becomes `T`'s default, so one odd
/// field never makes a whole page unreadable.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The formats of a page, each read on its own: a format that does not read (no id, say) is left out, and the others are kept.
fn readable_formats<'de, D>(deserializer: D) -> Result<Vec<Format>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Array(values) => values,
        _ => Vec::new(),
    };
    Ok(values
        .into_iter()
        .filter_map(|value| match serde_json::from_value(value) {
            Ok(format) => Some(format),
            Err(e) => {
                debug!(error = %e, "left out a format yt-dlp described oddly");
                None
            }
        })
        .collect())
}

/// Reads a count that yt-dlp may write as a whole number in floating point (`1080.0`, `1.5e6`); anything else, or a value out of range, is unknown.
fn whole_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: TryFrom<u64>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let whole = value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.fract() == 0.0)
            .and_then(f64_to_u64)
    });
    Ok(whole.and_then(|n| T::try_from(n).ok()))
}

/// Reads a map of text values, leaving out any entry whose value is not text.
fn text_map<'de, D>(deserializer: D) -> Result<HashMap<String, Secret>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let serde_json::Value::Object(map) = serde_json::Value::deserialize(deserializer)? else {
        return Ok(HashMap::new());
    };
    Ok(map
        .into_iter()
        .filter_map(|(name, value)| match value {
            serde_json::Value::String(text) => Some((name, Secret::from(text))),
            _ => None,
        })
        .collect())
}

/// One file of a subtitle language, in one of the formats the site offers.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SubtitleFile {
    #[serde(default, deserialize_with = "lenient")]
    pub ext: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    /// The language in words, for example "English" or "English (Original)".
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub http_headers: HashMap<String, Secret>,
}

/// One chapter of a video.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Chapter {
    #[serde(default, deserialize_with = "lenient")]
    pub start_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub end_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub title: Option<String>,
}

/// The id of a page's format, which a `-f` selection names it by. Sites choose these ids, so only letters, digits and `_ . - =` are accepted: a format with any other id is left out. A selection names an id in an exact, quoted filter ([`selector`](Self::selector)), since yt-dlp reads a bare `b`, `ba`, `best.2`, `mp4` and the like as a keyword or an extension before a format id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct FormatId(String);

/// The longest format id accepted; yt-dlp's are a few dozen characters at most.
const MAX_FORMAT_ID: usize = 100;

/// A format id with a character yt-dlp's selections give a meaning to, or none at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a format id Wirecatch passes to yt-dlp")]
pub(crate) struct InvalidFormatId(String);

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
    fn selector(&self) -> String {
        format!(r#"b*[format_id="{}"]"#, self.0)
    }
}

impl std::fmt::Display for FormatId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// yt-dlp's selection when no format is named: the best video with the best audio, else the best file with both.
const BEST_SELECTION: &str = "bv*+ba/b";

/// yt-dlp's `-f` selection for `video` and `audio`: both joined (`137` and `140` as `b*[format_id="137"]+b*[format_id="140"]`), either alone, or the best there is when neither is named.
pub(crate) fn selection(video: Option<&FormatId>, audio: Option<&FormatId>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => format!("{}+{}", v.selector(), a.selector()),
        (Some(one), None) | (None, Some(one)) => one.selector(),
        (None, None) => BEST_SELECTION.to_string(),
    }
}

/// The ids of `video` and `audio` as people and yt-dlp's own lists write them, `137+140`: a name for the pair, never a selection.
pub(crate) fn ids(video: Option<&FormatId>, audio: Option<&FormatId>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => format!("{v}+{a}"),
        (Some(one), None) | (None, Some(one)) => one.to_string(),
        (None, None) => String::new(),
    }
}

/// One format of a page.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Format {
    /// yt-dlp's `format_id`, which names the format in a `-f` selection.
    #[serde(rename = "format_id")]
    pub id: FormatId,
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub protocol: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub ext: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub vcodec: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub acodec: Option<String>,
    #[serde(default, deserialize_with = "whole_number")]
    pub height: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    pub fps: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub tbr: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub abr: Option<f64>,
    /// Video bit rate in kbps.
    #[serde(default, deserialize_with = "lenient")]
    pub vbr: Option<f64>,
    #[serde(default, deserialize_with = "whole_number")]
    pub filesize: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub filesize_approx: Option<f64>,
    #[serde(default, deserialize_with = "text_map")]
    pub http_headers: HashMap<String, Secret>,
    /// `true`, `false` or `"maybe"`.
    #[serde(default)]
    pub has_drm: Option<serde_json::Value>,
    /// How yt-dlp itself would download the format; see
    /// [`request_limit`](Self::request_limit).
    #[serde(default)]
    pub downloader_options: Option<DownloaderOptions>,
}

/// The part of yt-dlp's `downloader_options` Wirecatch reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DownloaderOptions {
    /// Kept as any JSON value, so an odd one is ignored instead of making the
    /// whole page unreadable.
    #[serde(default)]
    pub http_chunk_size: Option<serde_json::Value>,
}

/// The smallest `http_chunk_size` taken from yt-dlp. Anything smaller would
/// mean many thousands of requests for one video, so it is not a real limit.
const MIN_REQUEST_LIMIT: u64 = 64 * KIB;

/// The largest request limit used. A larger `http_chunk_size` is cut to this:
/// asking for less than a server allows is always fine.
const MAX_REQUEST_LIMIT: u64 = GIB;

impl Format {
    pub fn has_video(&self) -> bool {
        self.vcodec.as_deref().is_none_or(|c| c != "none")
            && !matches!(self.ext.as_deref(), Some("mhtml"))
    }

    pub fn has_audio(&self) -> bool {
        self.acodec.as_deref().is_none_or(|c| c != "none")
    }

    pub fn drm(&self) -> bool {
        matches!(self.has_drm, Some(serde_json::Value::Bool(true)))
    }

    /// The size in bytes, exact or estimated, when known.
    pub fn size(&self) -> Option<u64> {
        self.filesize.or_else(|| {
            self.filesize_approx
                .filter(|s| *s > 0.0)
                .and_then(f64_to_u64)
        })
    }

    /// The most bytes one request for this format should ask for, from yt-dlp's
    /// `http_chunk_size`: some servers (YouTube's among them) slow down any
    /// request for more, which is why yt-dlp itself downloads them in pieces.
    /// `None` when yt-dlp gave none, or a value that is not a whole number of
    /// at least 64 KiB; values above 1 GiB are cut to 1 GiB.
    pub fn request_limit(&self) -> Option<u64> {
        let value = self.downloader_options.as_ref()?.http_chunk_size.as_ref()?;
        let bytes = value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|f| f.is_finite() && f.fract() == 0.0 && *f >= 0.0)
                // Beyond `u64`: as large as can be, which the cut below brings down.
                .map(|f| f64_to_u64(f).unwrap_or(u64::MAX))
        })?;
        (bytes >= MIN_REQUEST_LIMIT).then(|| bytes.min(MAX_REQUEST_LIMIT))
    }

    /// Whether Wirecatch downloads this format itself: plain web addresses with
    /// the engine, HLS playlists with `wirecatch-streams`.
    pub fn transport(&self) -> Transport {
        match self.protocol.as_deref().unwrap_or("https") {
            "http" | "https" => Transport::File,
            "m3u8" | "m3u8_native" => Transport::Hls,
            _ => Transport::YtDlp,
        }
    }
}

/// Who downloads a format's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    File,
    Hls,
    YtDlp,
}

impl Info {
    pub fn is_live(&self) -> bool {
        self.is_live == Some(true)
            || matches!(self.live_status.as_deref(), Some("is_live" | "is_upcoming"))
    }

    /// The formats, including a single top-level one.
    pub fn all_formats(&self) -> Cow<'_, [Format]> {
        if !self.formats.is_empty() {
            return Cow::Borrowed(&self.formats);
        }
        match &self.url {
            Some(url) => Cow::Owned(vec![Format {
                id: self
                    .format_id
                    .clone()
                    .and_then(|id| FormatId::try_from(id).ok())
                    .unwrap_or_else(|| FormatId(TOP_LEVEL_FORMAT_ID.into())),
                url: Some(url.clone()),
                ext: self.ext.clone(),
                ..Format::default()
            }]),
            None => Cow::Borrowed(&[]),
        }
    }
}

/// The id given to a page's single top-level format when yt-dlp names none.
const TOP_LEVEL_FORMAT_ID: &str = "0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_read_from_yt_dlp_json() {
        let json = r#"{"title":"T","formats":[
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","protocol":"https","url":"https://a/140","filesize":10},
            {"format_id":"137","ext":"mp4","vcodec":"avc1","acodec":"none","height":1080,"protocol":"https","url":"https://a/137","has_drm":false},
            {"format_id":"hls-1","ext":"mp4","protocol":"m3u8_native","url":"https://a/x.m3u8","has_drm":"maybe"},
            {"format_id":"sb0","ext":"mhtml","vcodec":"none","acodec":"none","protocol":"mhtml"}
        ],"is_live":false}"#;
        let info: Info = serde_json::from_str(json).unwrap();
        assert!(!info.is_live());
        let f = &info.formats;
        assert!(f[0].has_audio() && !f[0].has_video());
        assert!(f[1].has_video() && !f[1].has_audio());
        assert_eq!(f[1].transport(), Transport::File);
        assert_eq!(f[2].transport(), Transport::Hls);
        assert!(!f[2].drm());
        assert_eq!(f[3].transport(), Transport::YtDlp);
        assert!(!f[3].has_video() && !f[3].has_audio());
        assert!(f.iter().all(|f| f.request_limit().is_none()));
    }

    // yt-dlp hands each format the page's cookies in its headers.
    #[test]
    fn a_format_never_prints_its_header_values() {
        let info: Info = serde_json::from_str(
            r#"{"title":"T","formats":[{"format_id":"a","http_headers":{"Cookie":"sid=cookie-that-must-not-print"}}]}"#,
        )
        .unwrap();
        let printed = format!("{:?}", info.all_formats());
        assert!(!printed.contains("must-not-print"), "{printed}");
        assert!(printed.contains("Cookie"), "{printed}");
    }

    /// One extractor writing a number where yt-dlp usually writes another kind never makes the whole page unreadable, and a format that cannot be read at all is left out alone.
    #[test]
    fn odd_format_fields_never_make_the_page_unreadable() {
        let info: Info = serde_json::from_str(
            r#"{"title":"T","formats":[
                {"format_id":"a","height":1080.0,"filesize":1.5e6,"fps":"30","ext":5,"http_headers":{"Referer":"r","X-Count":3}},
                {"format_id":7},
                {"format_id":"b","height":-1,"vcodec":null}
            ]}"#,
        )
        .unwrap();
        let formats = info.all_formats();
        let ids: Vec<String> = formats.iter().map(|f| f.id.to_string()).collect();
        assert_eq!(ids, ["a", "b"]);
        let a = &formats[0];
        assert_eq!(a.height, Some(1080));
        assert_eq!(a.filesize, Some(1_500_000));
        assert_eq!(a.fps, None);
        assert_eq!(a.ext, None);
        assert_eq!(a.http_headers.get("Referer").map(Secret::expose), Some("r"));
        assert!(!a.http_headers.contains_key("X-Count"));
        assert_eq!(formats[1].height, None);
    }

    #[test]
    fn the_request_limit_comes_from_http_chunk_size() {
        let limit = |value: serde_json::Value| {
            let format: Format = serde_json::from_value(serde_json::json!({
                "format_id": "137",
                "downloader_options": {"http_chunk_size": value},
            }))
            .unwrap();
            format.request_limit()
        };
        // What yt-dlp reports for every YouTube format.
        assert_eq!(limit(10_485_760.into()), Some(10_485_760));
        assert_eq!(limit(serde_json::json!(10_485_760.0)), Some(10_485_760));
        assert_eq!(limit((64 * KIB).into()), Some(64 * KIB));
        // Too large: cut to the most it takes.
        assert_eq!(limit((5 * GIB).into()), Some(MAX_REQUEST_LIMIT));
        assert_eq!(limit(serde_json::json!(1e300)), Some(MAX_REQUEST_LIMIT));
        // Zero, negative, tiny, fractional or not a number: no limit, and the
        // page is still readable.
        for odd in [
            serde_json::json!(0),
            serde_json::json!(-10_485_760),
            serde_json::json!(4096),
            serde_json::json!(10_485_760.5),
            serde_json::json!("10485760"),
            serde_json::json!(null),
            serde_json::json!({"x": 1}),
        ] {
            assert_eq!(limit(odd.clone()), None, "{odd}");
        }
        // No `downloader_options` at all, or no chunk size in it.
        let plain: Format = serde_json::from_str(r#"{"format_id":"1"}"#).unwrap();
        assert_eq!(plain.request_limit(), None);
        let other: Format =
            serde_json::from_str(r#"{"format_id":"1","downloader_options":{"x":1}}"#).unwrap();
        assert_eq!(other.request_limit(), None);
    }

    /// An estimated size is used only when it is a byte count: one too large
    /// for any file is unknown, not the largest number there is.
    #[test]
    fn an_estimated_size_that_is_not_a_count_is_unknown() {
        let size = |approx: serde_json::Value| {
            let format: Format = serde_json::from_value(serde_json::json!({
                "format_id": "18",
                "filesize_approx": approx,
            }))
            .unwrap();
            format.size()
        };
        assert_eq!(size(serde_json::json!(1_048_576.7)), Some(1_048_576));
        assert_eq!(size(serde_json::json!(1e30)), None);
        assert_eq!(size(serde_json::json!(-5.0)), None);
        assert_eq!(size(serde_json::json!(0.0)), None);
    }

    /// A page description as yt-dlp prints it for YouTube: every format has
    /// `downloader_options.http_chunk_size`, and the whole page still reads
    /// when one format has an odd value there.
    #[test]
    fn a_youtube_page_gives_every_format_its_chunk_size() {
        let json = r#"{"title":"T","formats":[
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","protocol":"https",
             "url":"https://rr1.googlevideo.example/videoplayback?itag=140","downloader_options":{"http_chunk_size":10485760}},
            {"format_id":"137","ext":"mp4","vcodec":"avc1.640028","acodec":"none","protocol":"https",
             "url":"https://rr1.googlevideo.example/videoplayback?itag=137","downloader_options":{"http_chunk_size":10485760}},
            {"format_id":"18","ext":"mp4","protocol":"https","url":"https://a/18","downloader_options":{"http_chunk_size":"big"}}
        ]}"#;
        let info: Info = serde_json::from_str(json).unwrap();
        let limits: Vec<Option<u64>> = info.formats.iter().map(Format::request_limit).collect();
        assert_eq!(limits, [Some(10_485_760), Some(10_485_760), None]);
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
        // A page whose format ids a site made hostile keeps the formats it can name.
        let info: Info = serde_json::from_str(
            r#"{"formats":[{"format_id":"137"},{"format_id":"137+bestaudio"},{"format_id":"best/b"}]}"#,
        )
        .unwrap();
        let ids: Vec<String> = info.formats.iter().map(|f| f.id.to_string()).collect();
        assert_eq!(ids, ["137"]);
    }

    /// yt-dlp reads a bare `b`, `ba`, `best.2`, `mp4` or `mhtml` as a keyword or an extension before a format id, so every id is selected by an exact filter instead.
    #[test]
    fn a_selection_names_formats_by_their_exact_id() {
        let id = |s: &str| FormatId::try_from(s.to_string()).unwrap();
        assert_eq!(
            selection(Some(&id("137")), Some(&id("140"))),
            r#"b*[format_id="137"]+b*[format_id="140"]"#
        );
        for keyword in [
            "b",
            "w",
            "best",
            "worst",
            "bv",
            "ba",
            "wa",
            "b.2",
            "all",
            "mergeall",
            "bestvideo",
            "worstaudio",
            "mp4",
            "webm",
            "m4a",
            "mp3",
            "flv",
            "3gp",
            "ogg",
            "aac",
            "mhtml",
            "dash-video=4000000",
        ] {
            assert_eq!(
                selection(None, Some(&id(keyword))),
                format!(r#"b*[format_id="{keyword}"]"#)
            );
        }
        assert_eq!(selection(None, None), BEST_SELECTION);
    }
}
