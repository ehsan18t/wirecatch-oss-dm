//! The one list of media file extensions, for streams (this crate) and for pages read by yt-dlp (`wirecatch-core`).

#![forbid(unsafe_code)]

/// Extensions a saved video or audio file may have. A site names the extension of what it serves (through yt-dlp, or in a link); taking that as it is would let a page save `Invoice.exe` (yt-dlp's CVE-2024-38519), so a name with any other extension gets one of these instead.
const MEDIA: [&str; 17] = [
    "mp4", "m4v", "mkv", "mka", "webm", "mov", "ts", "m2ts", "mp3", "m4a", "aac", "opus", "ogg",
    "flac", "wav", "3gp", "flv",
];

/// Extensions of playlists, manifests and segments. A name made from a stream's link may end in one, and it is replaced by the extension of the file written, but no file is ever saved with one.
const STREAM_PARTS: [&str; 3] = ["m3u8", "mpd", "m4s"];

/// Whether `ext` (without the dot, any case) is a media extension a file may be saved with.
#[must_use]
pub fn is_media(ext: &str) -> bool {
    MEDIA.contains(&ext.to_ascii_lowercase().as_str())
}

/// Whether `ext` (without the dot, any case) is replaced by the extension of the file actually written: a media extension, or that of a playlist, manifest or segment.
#[must_use]
pub fn is_replaceable(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    MEDIA.contains(&ext.as_str()) || STREAM_PARTS.contains(&ext.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_extensions_are_known_in_any_case_and_programs_are_not() {
        for ext in ["mp4", "MKV", "flac", "Opus", "m2ts", "3gp"] {
            assert!(is_media(ext), "{ext}");
            assert!(is_replaceable(ext), "{ext}");
        }
        for ext in ["m3u8", "MPD", "m4s"] {
            assert!(!is_media(ext), "{ext}");
            assert!(is_replaceable(ext), "{ext}");
        }
        for ext in ["exe", "bat", "lnk", "", "mp4.exe", "../mp4"] {
            assert!(!is_media(ext), "{ext}");
            assert!(!is_replaceable(ext), "{ext}");
        }
    }
}
