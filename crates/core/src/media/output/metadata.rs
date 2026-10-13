//! The ffmpeg metadata file (`;FFMETADATA1`) with a video's title, artist and chapters, escaped so a site's text never reaches ffmpeg's options.

#![forbid(unsafe_code)]

use wirecatch_engine::numeric::f64_to_u64;

use crate::media::ytdlp::Chapter;

/// The latest a chapter can start or end, in seconds; later is a mistake in the page.
const MAX_CHAPTER_SECONDS: f64 = 1e9;

/// Most chapters written.
const MAX_CHAPTERS: usize = 1_000;
/// Longest text written into a tag or a chapter title.
const MAX_TAG_CHARS: usize = 300;

/// An ffmpeg metadata file (`;FFMETADATA1`) with the title and artist, when
/// given, and the chapters that make sense (in order, inside the video, not
/// empty).
pub(crate) fn ffmetadata(
    title: Option<&str>,
    artist: Option<&str>,
    chapters: &[Chapter],
    duration_ms: Option<u64>,
) -> String {
    // Line by line, joined at the end.
    let mut lines = vec![";FFMETADATA1".to_string()];
    for (key, value) in [("title", title), ("artist", artist)] {
        if let Some(value) = value.map(escape).filter(|v| !v.is_empty()) {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut last_end = 0u64;
    for chapter in chapters.iter().take(MAX_CHAPTERS) {
        let ms = |s: Option<f64>| {
            s.filter(|s| (0.0..MAX_CHAPTER_SECONDS).contains(s))
                .and_then(|s| f64_to_u64((s * 1000.0).round()))
        };
        let (Some(start), Some(end)) = (ms(chapter.start_time), ms(chapter.end_time)) else {
            continue;
        };
        let end = duration_ms.map_or(end, |d| end.min(d));
        if end <= start || start < last_end {
            continue;
        }
        last_end = end;
        lines.push(format!(
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}"
        ));
        if let Some(title) = chapter
            .title
            .as_deref()
            .map(escape)
            .filter(|t| !t.is_empty())
        {
            lines.push(format!("title={title}"));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Whether `chapters` holds any chapter [`ffmetadata`] would write.
pub(crate) fn has_chapters(chapters: &[Chapter]) -> bool {
    ffmetadata(None, None, chapters, None).contains("[CHAPTER]")
}

/// A value for an ffmpeg metadata file: control characters dropped, the
/// characters that file format gives a meaning (`=`, `;`, `#`, `\`) escaped.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TAG_CHARS)
    {
        if matches!(c, '=' | ';' | '#' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_files_escape_what_the_site_wrote() {
        let chapters: Vec<Chapter> = serde_json::from_value(serde_json::json!([
            {"start_time": 0, "end_time": 61.5, "title": "Intro; part=1 #a\\b"},
            {"start_time": 61.5, "end_time": 120, "title": "Line\nbreak"},
            {"start_time": 100, "end_time": 90, "title": "Backwards"},
            {"start_time": 130, "end_time": 500, "title": "Past the end"},
        ]))
        .unwrap();
        let text = ffmetadata(Some("A = B"), Some("Me\r\n"), &chapters, Some(150_000));
        assert_eq!(
            text,
            ";FFMETADATA1\ntitle=A \\= B\nartist=Me\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=61500\ntitle=Intro\\; part\\=1 \\#a\\\\b\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=61500\nEND=120000\ntitle=Linebreak\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=130000\nEND=150000\ntitle=Past the end\n"
        );
        assert!(has_chapters(&chapters));
        assert!(!has_chapters(&[]));
    }
}
