//! The subtitles a page offers, the subtitle and picture files a conversion reads (checked to be what they claim), and the languages remembered for next time.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use wirecatch_protocol::app::MediaSubtitle;

use super::SubtitlePick;
use crate::ytdlp::{Info, SubtitleFile};

/// The longest subtitle key read as a language (`en`, `pt-BR`, `zh-Hans-orig`).
const MAX_KEY_LEN: usize = 35;

/// Most subtitle languages offered for one video.
const MAX_SUBTITLES: usize = 100;
/// Longest language name kept.
pub(super) const MAX_NAME_CHARS: usize = 60;
/// Subtitle file formats Wirecatch reads, best first.
const SUBTITLE_EXTS: [&str; 4] = ["vtt", "srt", "ass", "ssa"];

/// The subtitles a page offers: human-written ones first (by name), then auto-generated ones in the video's own language only (a site may list a machine translation into every language; those are left out). Lists without a file Wirecatch can read (live chat, for example) are left out too.
#[must_use]
pub fn subtitles(info: &Info) -> Vec<(MediaSubtitle, SubtitlePick)> {
    let mut found = Vec::new();
    for (auto, list) in [(false, &info.subtitles), (true, &info.automatic_captions)] {
        let mut part: Vec<(MediaSubtitle, SubtitlePick)> = list
            .iter()
            .filter(|(key, files)| plain_key(key) && pick_file(files).is_some())
            .filter(|(_, files)| !auto || pick_file(files).is_some_and(|f| !translated(f)))
            .map(|(key, files)| {
                let language = language_of(key);
                let name = files
                    .iter()
                    .find_map(|f| f.name.as_deref())
                    .map(clean_name)
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| language.clone());
                let id = format!("{}:{key}", if auto { "auto" } else { "sub" });
                let pick = SubtitlePick {
                    key: key.clone(),
                    auto,
                    language: language.clone(),
                    name: name.clone(),
                };
                (
                    MediaSubtitle {
                        id,
                        language,
                        name,
                        auto,
                    },
                    pick,
                )
            })
            .collect();
        part.sort_by(|a, b| a.0.name.cmp(&b.0.name).then(a.0.id.cmp(&b.0.id)));
        found.extend(part);
    }
    found.truncate(MAX_SUBTITLES);
    found
}

/// A key that names a language (`en`, `pt-BR`, `en-orig`), not live chat or the like.
fn plain_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(key, "live_chat" | "rechat" | "danmaku" | "comments")
}

/// The language code of a subtitle key: `en-orig` (YouTube's name for the language a video is spoken in) is `en`.
pub(crate) fn language_of(key: &str) -> String {
    key.strip_suffix("-orig").unwrap_or(key).to_string()
}

/// A language name from the site, cut to a sensible length, without control characters, and without YouTube's "(Original)".
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    name.trim()
        .trim_end_matches("(Original)")
        .trim()
        .to_string()
}

/// A machine translation of a subtitle (YouTube's `tlang`).
fn translated(file: &SubtitleFile) -> bool {
    file.url
        .as_deref()
        .and_then(|u| wirecatch_engine::Url::parse(u).ok())
        .is_some_and(|u| u.query_pairs().any(|(k, _)| k == "tlang"))
}

/// The file of a subtitle language Wirecatch downloads: `WebVTT`, else `SubRip`, else `SubStation`.
pub(crate) fn pick_file(files: &[SubtitleFile]) -> Option<&SubtitleFile> {
    SUBTITLE_EXTS.iter().find_map(|ext| {
        files
            .iter()
            .find(|f| f.ext.as_deref() == Some(*ext) && f.url.is_some())
    })
}

/// The subtitle files of a page for `pick`, found again after reading it.
#[must_use]
pub fn subtitle_files<'a>(info: &'a Info, pick: &SubtitlePick) -> Option<&'a SubtitleFile> {
    let list = if pick.auto {
        &info.automatic_captions
    } else {
        &info.subtitles
    };
    list.get(&pick.key).and_then(|files| pick_file(files))
}

/// A subtitle file format ffmpeg reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleFormat {
    WebVtt,
    Srt,
    Ass,
}

impl SubtitleFormat {
    /// ffmpeg's demuxer for it.
    #[must_use]
    pub fn demuxer(self) -> &'static str {
        match self {
            SubtitleFormat::WebVtt => "webvtt",
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::Ass => "ass",
        }
    }

    /// Recognizes a downloaded subtitle file from its first bytes; `None` when it is not text in the format its extension `ext` promises, so a site cannot hand ffmpeg anything else.
    pub fn detect(head: &[u8], ext: &str) -> Option<SubtitleFormat> {
        let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
        let text = std::str::from_utf8(head)
            .or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
            .ok()?;
        let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
        match ext {
            "vtt" if first.starts_with("WEBVTT") => Some(SubtitleFormat::WebVtt),
            "ass" | "ssa" if first.eq_ignore_ascii_case("[Script Info]") => {
                Some(SubtitleFormat::Ass)
            }
            "srt" if first.bytes().all(|b| b.is_ascii_digit()) => Some(SubtitleFormat::Srt),
            _ => None,
        }
    }
}

/// A picture format ffmpeg reads for a cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Jpeg,
    Png,
    Webp,
}

impl ImageFormat {
    /// ffmpeg's demuxer for it.
    #[must_use]
    pub fn demuxer(self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg_pipe",
            ImageFormat::Png => "png_pipe",
            ImageFormat::Webp => "webp_pipe",
        }
    }

    /// Recognizes a picture from its first bytes.
    #[must_use]
    pub fn detect(head: &[u8]) -> Option<ImageFormat> {
        if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(ImageFormat::Jpeg)
        } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ImageFormat::Png)
        } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            Some(ImageFormat::Webp)
        } else {
            None
        }
    }
}

/// The three-letter (ISO 639-2) code for a two-letter one, which MP4 needs for a track's language; other codes are returned as they are.
pub(crate) fn iso639_2(language: &str) -> String {
    let primary = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase();
    let three = match primary.as_str() {
        "ar" => "ara",
        "bg" => "bul",
        "bn" => "ben",
        "ca" => "cat",
        "cs" => "cze",
        "da" => "dan",
        "de" => "ger",
        "el" => "gre",
        "en" => "eng",
        "es" => "spa",
        "et" => "est",
        "fa" => "per",
        "fi" => "fin",
        "fil" | "tl" => "fil",
        "fr" => "fre",
        "he" | "iw" => "heb",
        "hi" => "hin",
        "hr" => "hrv",
        "hu" => "hun",
        "id" => "ind",
        "it" => "ita",
        "ja" => "jpn",
        "ko" => "kor",
        "lt" => "lit",
        "lv" => "lav",
        "ms" => "may",
        "nl" => "dut",
        "no" | "nb" => "nor",
        "pa" => "pan",
        "pl" => "pol",
        "pt" => "por",
        "ro" => "rum",
        "ru" => "rus",
        "sk" => "slo",
        "sl" => "slv",
        "sr" => "srp",
        "sv" => "swe",
        "sw" => "swa",
        "ta" => "tam",
        "te" => "tel",
        "th" => "tha",
        "tr" => "tur",
        "uk" => "ukr",
        "ur" => "urd",
        "vi" => "vie",
        "zh" => "chi",
        other if other.len() == 3 && other.bytes().all(|b| b.is_ascii_lowercase()) => {
            return other.to_string();
        }
        _ => "und",
    };
    three.to_string()
}

/// Subtitle languages remembered for next time from the subtitles picked, and whether an auto-generated one was ticked, which makes them wanted. Leaving them unticked never makes them unwanted (D50): a person who ticks the subtitles written by people still wants a language's auto-generated ones when it has none. `None` when the video had no subtitles to pick from, so the last choice stays.
#[must_use]
pub fn remembered_subtitles(
    offered: &[MediaSubtitle],
    picked: &[String],
) -> Option<(Vec<String>, bool)> {
    if offered.is_empty() {
        return None;
    }
    let mut languages: Vec<String> = Vec::new();
    let mut any_auto = false;
    for subtitle in offered.iter().filter(|s| picked.contains(&s.id)) {
        let language = subtitle.language.to_ascii_lowercase();
        if !languages.contains(&language) {
            languages.push(language);
        }
        any_auto |= subtitle.auto;
    }
    Some((languages, any_auto))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::test_support::*;

    /// Human-written subtitles first, then the auto-generated ones in the video's own language only; live chat and unreadable formats are left out.
    #[test]
    fn subtitles_list_human_ones_first_and_skip_translations() {
        let info = info(serde_json::json!({
            "subtitles": {
                "fr": [{"ext": "vtt", "url": "https://s/fr.vtt", "name": "French"}],
                "en-GB": [{"ext": "json3", "url": "https://s/x"}, {"ext": "vtt", "url": "https://s/en.vtt", "name": "English (United Kingdom)"}],
                "live_chat": [{"ext": "json", "url": "https://s/chat"}],
                "de": [{"ext": "json3", "url": "https://s/de"}],
            },
            "automatic_captions": {
                "en-orig": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt", "name": "English (Original)"}],
                "en": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt&tlang=en", "name": "English"}],
                "es": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt&tlang=es", "name": "Spanish"}],
            }
        }));
        let list = subtitles(&info);
        let shown: Vec<(&str, &str, bool)> = list
            .iter()
            .map(|(s, _)| (s.id.as_str(), s.name.as_str(), s.auto))
            .collect();
        assert_eq!(
            shown,
            [
                ("sub:en-GB", "English (United Kingdom)", false),
                ("sub:fr", "French", false),
                ("auto:en-orig", "English", true),
            ]
        );
        assert_eq!(list[2].0.language, "en");
        assert_eq!(list[2].1.key, "en-orig");
        assert_eq!(
            subtitle_files(&info, &list[0].1).unwrap().url.as_deref(),
            Some("https://s/en.vtt")
        );
    }

    /// yt-dlp sometimes says `null` or something odd: the page is still read.
    #[test]
    fn odd_subtitle_and_chapter_values_are_ignored() {
        let info = info(serde_json::json!({
            "title": "T", "subtitles": null, "automatic_captions": "nope",
            "chapters": [{"start_time": "x"}, 7], "duration": "long", "thumbnail": 5,
        }));
        assert!(info.subtitles.is_empty() && info.automatic_captions.is_empty());
        assert!(info.chapters.is_empty());
        assert_eq!(info.duration, None);
        assert_eq!(info.thumbnail, None);
        assert_eq!(info.title.as_deref(), Some("T"));
    }

    #[test]
    fn last_subtitle_choices_are_remembered() {
        let s = |id: &str, language: &str, auto: bool| MediaSubtitle {
            id: id.into(),
            language: language.into(),
            name: language.into(),
            auto,
        };
        let offered = [
            s("sub:en-US", "en-US", false),
            s("sub:fr", "fr", false),
            s("auto:de-orig", "de", true),
        ];
        // English and the German auto-generated ones were ticked.
        let (languages, auto) =
            remembered_subtitles(&offered, &["sub:en-US".into(), "auto:de-orig".into()]).unwrap();
        assert_eq!(languages, ["en-us", "de"]);
        assert!(auto);
        // Nothing offered: the last choice stays.
        assert_eq!(remembered_subtitles(&[], &[]), None);
        // Auto-generated ones offered and none ticked: whether they are wanted stays as it was.
        assert_eq!(
            remembered_subtitles(&offered, &["sub:fr".into()]),
            Some((vec!["fr".to_string()], false))
        );
    }

    #[test]
    fn languages_get_three_letter_codes_for_mp4() {
        assert_eq!(iso639_2("en"), "eng");
        assert_eq!(iso639_2("pt-BR"), "por");
        assert_eq!(iso639_2("fil"), "fil");
        assert_eq!(iso639_2("zz-top"), "und");
    }

    #[test]
    fn subtitle_and_picture_files_are_recognised_by_their_content() {
        assert_eq!(
            SubtitleFormat::detect(b"\xEF\xBB\xBFWEBVTT\n\n00:00", "vtt"),
            Some(SubtitleFormat::WebVtt)
        );
        assert_eq!(
            SubtitleFormat::detect(b"\n1\n00:00:01,000 --> 00:00:02,000\nHi", "srt"),
            Some(SubtitleFormat::Srt)
        );
        assert_eq!(
            SubtitleFormat::detect(b"[Script Info]\nTitle: x", "ass"),
            Some(SubtitleFormat::Ass)
        );
        // A playlist or anything else named like subtitles is refused.
        assert_eq!(SubtitleFormat::detect(b"#EXTM3U\n", "vtt"), None);
        assert_eq!(SubtitleFormat::detect(b"WEBVTT", "srt"), None);
        assert_eq!(SubtitleFormat::detect(b"", "vtt"), None);
        assert_eq!(
            ImageFormat::detect(b"\xFF\xD8\xFF\xE0"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::detect(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(
            ImageFormat::detect(b"\x89PNG\r\n\x1a\n"),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::detect(b"<svg"), None);
    }
}
