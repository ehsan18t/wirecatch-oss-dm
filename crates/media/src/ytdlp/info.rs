//! What yt-dlp says about a page: its formats, subtitles and chapters, and the format ids a selection names them by.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashMap;

use serde::Deserialize;
use tracing::debug;
use wirecatch_engine::numeric::f64_to_u64;
use wirecatch_engine::units::{GIB, KIB};
use wirecatch_protocol::Secret;
pub use wirecatch_records::media::FormatId;

/// What yt-dlp says about a page (the fields Wirecatch reads).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Info {
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
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub ext: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
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
    /// For a page with several videos, each described on its own.
    #[serde(default, deserialize_with = "lenient")]
    entries: Vec<serde_json::Value>,
    /// What yt-dlp names `_type`, which says what `url` is.
    #[serde(default, rename = "_type", deserialize_with = "lenient")]
    kind: Option<String>,
    /// The fields not named here, which describe the one format of a page that has no list of them.
    #[serde(flatten)]
    top_level: TopLevel,
}

/// A page's top-level fields, which hold its headers and so are never printed.
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
struct TopLevel(serde_json::Map<String, serde_json::Value>);

impl std::fmt::Debug for TopLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} fields", self.0.len())
    }
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
        .filter_map(|value| match serde_json::from_value::<Format>(value) {
            Ok(format) if format.id.to_string().is_empty() => None,
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
pub struct SubtitleFile {
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
pub struct Chapter {
    #[serde(default, deserialize_with = "lenient")]
    pub start_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub end_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub title: Option<String>,
}

/// yt-dlp's selection when no format is named: the best video with the best audio, else the best file with both.
const BEST_SELECTION: &str = "bv*+ba/b";

/// yt-dlp's `-f` selection for `video` and `audio`: both joined (`137` and `140` as `b*[format_id="137"]+b*[format_id="140"]`), either alone, or the best there is when neither is named.
#[must_use]
pub fn selection(video: Option<&FormatId>, audio: Option<&FormatId>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => format!("{}+{}", v.selector(), a.selector()),
        (Some(one), None) | (None, Some(one)) => one.selector(),
        (None, None) => BEST_SELECTION.to_string(),
    }
}

/// The ids of `video` and `audio` as people and yt-dlp's own lists write them, `137+140`: a name for the pair, never a selection.
#[must_use]
pub fn ids(video: Option<&FormatId>, audio: Option<&FormatId>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => format!("{v}+{a}"),
        (Some(one), None) | (None, Some(one)) => one.to_string(),
        (None, None) => String::new(),
    }
}

/// One format of a page.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Format {
    /// yt-dlp's `format_id`, which names the format in a `-f` selection.
    #[serde(rename = "format_id", default)]
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
    /// How yt-dlp ranks the track's language: higher is nearer the video's own.
    #[serde(default, deserialize_with = "lenient")]
    pub language_preference: Option<i64>,
    #[serde(default, deserialize_with = "whole_number")]
    pub filesize: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub filesize_approx: Option<f64>,
    #[serde(default, deserialize_with = "text_map")]
    pub http_headers: HashMap<String, Secret>,
    /// The cookies the format's address needs, which yt-dlp lists here and not in `http_headers`. Never given to the engine: a page's cookies stay with the page.
    #[serde(default, deserialize_with = "lenient")]
    pub cookies: Option<Secret>,
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
pub struct DownloaderOptions {
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

/// Extensions that say a format holds no video when yt-dlp names no video codec.
const NOT_VIDEO_EXTENSIONS: [&str; 10] = [
    "mhtml", "mp3", "m4a", "aac", "opus", "ogg", "flac", "wav", "mka", "weba",
];

impl Format {
    #[must_use]
    pub fn has_video(&self) -> bool {
        let ext = self.ext.as_deref().map(str::to_ascii_lowercase);
        match self.vcodec.as_deref() {
            Some("none") => false,
            Some(_) => ext.as_deref() != Some("mhtml"),
            None => !ext.is_some_and(|e| NOT_VIDEO_EXTENSIONS.contains(&e.as_str())),
        }
    }

    #[must_use]
    pub fn has_audio(&self) -> bool {
        self.acodec.as_deref().is_none_or(|c| c != "none")
    }

    #[must_use]
    pub fn drm(&self) -> bool {
        matches!(self.has_drm, Some(serde_json::Value::Bool(true)))
    }

    /// The size in bytes, exact or estimated, when known.
    #[must_use]
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
    #[must_use]
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

    pub(crate) fn language_rank(&self) -> i64 {
        // yt-dlp's own default for a format that does not say.
        const UNRANKED: i64 = -1;
        self.language_preference.unwrap_or(UNRANKED)
    }

    /// Whether Wirecatch downloads this format itself: plain web addresses with
    /// the engine, HLS playlists with `wirecatch-streams`.
    #[must_use]
    pub fn transport(&self) -> Transport {
        if self.cookies.is_some() {
            return Transport::YtDlp;
        }
        match self.protocol.as_deref().unwrap_or("https") {
            "http" | "https" => Transport::File,
            "m3u8" | "m3u8_native" => Transport::Hls,
            _ => Transport::YtDlp,
        }
    }
}

/// Who downloads a format's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    File,
    Hls,
    YtDlp,
}

impl Info {
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.is_live == Some(true)
            || matches!(self.live_status.as_deref(), Some("is_live" | "is_upcoming"))
    }

    /// The formats, including a single top-level one.
    #[must_use]
    pub fn all_formats(&self) -> Cow<'_, [Format]> {
        if !self.formats.is_empty() {
            return Cow::Borrowed(&self.formats);
        }
        match &self.url {
            Some(url) => Cow::Owned(vec![self.top_level_format(url)]),
            None => Cow::Borrowed(&[]),
        }
    }

    /// The one format a page describes at the top level, from the same fields a listed format has.
    fn top_level_format(&self, url: &str) -> Format {
        let id = self
            .format_id
            .clone()
            .and_then(|id| FormatId::try_from(id).ok())
            .unwrap_or_else(|| {
                FormatId::try_from(TOP_LEVEL_FORMAT_ID.to_owned()).unwrap_or_default()
            });
        let fields = serde_json::Value::Object(self.top_level.0.clone());
        let read: Format = serde_json::from_value(fields).unwrap_or_default();
        Format {
            id,
            url: Some(url.to_owned()),
            ext: self.ext.clone(),
            ..read
        }
    }

    /// What a page with several videos is read as: the first entry that describes a video (D120), which `--playlist-items 1` leaves as the only one. Any other page is itself.
    #[must_use]
    pub fn single_video(self) -> Info {
        if self.entries.is_empty() {
            return self;
        }
        let first = self
            .entries
            .iter()
            .filter_map(|entry| serde_json::from_value::<Info>(entry.clone()).ok())
            .find(Info::has_something_to_download);
        match first {
            Some(video) => Info {
                title: video.title.or(self.title),
                ..video
            },
            None => Info {
                entries: Vec::new(),
                ..self
            },
        }
    }

    /// Whether this describes a video, rather than a link to one yt-dlp would read next.
    fn has_something_to_download(&self) -> bool {
        !self.formats.is_empty()
            || (self.url.is_some()
                && !matches!(self.kind.as_deref(), Some("url" | "url_transparent")))
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

    #[test]
    fn a_page_described_at_the_top_level_keeps_its_protocol_headers_and_codecs() {
        let info: Info = serde_json::from_str(
            r#"{"title":"T","url":"https://x.example/p.m3u8","ext":"mp4","protocol":"m3u8_native","format_id":"7","vcodec":"none","acodec":"aac","height":720,"http_headers":{"Referer":"https://x.example/"}}"#,
        )
        .unwrap();
        let all = info.all_formats();
        assert_eq!(all[0].transport(), Transport::Hls);
        assert_eq!(all[0].id.to_string(), "7");
        assert!(all[0].http_headers.contains_key("Referer"));
        assert!(!all[0].has_video());
        assert!(all[0].has_audio());
        assert_eq!(all[0].height, Some(720));
    }

    #[test]
    fn a_page_described_at_the_top_level_never_prints_its_headers() {
        let info: Info = serde_json::from_str(
            r#"{"url":"https://x.example/v","http_headers":{"Cookie":"sid=cookie-that-must-not-print"}}"#,
        )
        .unwrap();
        let printed = format!("{info:?}");
        assert!(!printed.contains("must-not-print"), "{printed}");
    }

    #[test]
    fn a_top_level_field_that_reads_oddly_leaves_the_format_alone() {
        let info: Info = serde_json::from_str(
            r#"{"url":"https://x.example/v.mp4","ext":"mp4","format_id":7,"height":"tall","http_headers":3}"#,
        )
        .unwrap();
        let all = info.all_formats();
        assert_eq!(all[0].id.to_string(), TOP_LEVEL_FORMAT_ID);
        assert_eq!(all[0].height, None);
    }

    #[test]
    fn a_page_with_several_videos_is_read_as_its_first() {
        let info: Info = serde_json::from_str(
            r#"{"_type":"playlist","title":"The page","entries":[
            null,
            {"_type":"url","url":"https://x.example/elsewhere"},
            {"id":"a","formats":[{"format_id":"0","ext":"mp4","url":"https://x.example/a.mp4"}]},
            {"title":"B","formats":[{"format_id":"0","ext":"mp4","url":"https://x.example/b.mp4"}]}]}"#,
        )
        .unwrap();
        let first = info.single_video();
        assert_eq!(first.title.as_deref(), Some("The page"));
        assert_eq!(first.id.as_deref(), Some("a"));
        assert_eq!(
            first.formats[0].url.as_deref(),
            Some("https://x.example/a.mp4")
        );
        let plain: Info =
            serde_json::from_str(r#"{"title":"T","url":"https://x.example/v"}"#).unwrap();
        assert_eq!(plain.single_video().all_formats().len(), 1);
        let none: Info = serde_json::from_str(r#"{"_type":"playlist","entries":[]}"#).unwrap();
        assert!(none.single_video().all_formats().is_empty());
    }

    #[test]
    fn an_audio_file_with_no_codec_named_is_not_a_video() {
        let format = |json: &str| serde_json::from_str::<Format>(json).unwrap();
        assert!(!format(r#"{"format_id":"a","ext":"mp3"}"#).has_video());
        assert!(!format(r#"{"format_id":"a","ext":"M4A"}"#).has_video());
        assert!(format(r#"{"format_id":"a","ext":"m4a","vcodec":"mjpeg"}"#).has_video());
        assert!(format(r#"{"format_id":"v","ext":"mp4"}"#).has_video());
        assert!(format(r#"{"format_id":"v"}"#).has_video());
    }

    #[test]
    fn a_format_never_prints_its_cookies_or_header_values() {
        let info: Info = serde_json::from_str(
            r#"{"title":"T","formats":[{"format_id":"a","cookies":"sid=cookie-that-must-not-print","http_headers":{"Cookie":"sid=header-that-must-not-print"}}]}"#,
        )
        .unwrap();
        let printed = format!("{:?}", info.all_formats());
        assert!(!printed.contains("must-not-print"), "{printed}");
        assert!(printed.contains("Cookie"), "{printed}");
    }

    #[test]
    fn a_format_that_needs_cookies_is_left_to_yt_dlp_whatever_its_protocol() {
        let info: Info = serde_json::from_str(
            r#"{"title":"T","formats":[
                {"format_id":"a","protocol":"https","cookies":"sid=1; Domain=example.com; Path=/"},
                {"format_id":"b","protocol":"m3u8","cookies":"sid=1; Domain=example.com; Path=/"},
                {"format_id":"c","protocol":"https","cookies":7}
            ]}"#,
        )
        .unwrap();
        let formats = info.all_formats();
        assert_eq!(formats[0].transport(), Transport::YtDlp);
        assert_eq!(formats[1].transport(), Transport::YtDlp);
        assert_eq!(formats[2].transport(), Transport::File);
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
    fn a_page_keeps_the_formats_it_can_name() {
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
