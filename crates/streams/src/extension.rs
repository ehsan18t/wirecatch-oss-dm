//! The one list of media file extensions, for streams (this crate) and for pages read by yt-dlp (`wirecatch-core`).

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::ffmpeg::InputFormat;

/// Extensions a saved video or audio file may have, and the format ffmpeg reads each as: what a page offers to convert is only what ffmpeg may read. A site names the extension of what it serves (through yt-dlp, or in a link); taking that as it is would let a page save `Invoice.exe` (yt-dlp's CVE-2024-38519), so a name with any other extension gets one of these instead.
const MEDIA: [(&str, InputFormat); 18] = [
    ("mp4", InputFormat::Mp4),
    ("m4v", InputFormat::Mp4),
    ("mkv", InputFormat::Matroska),
    ("mka", InputFormat::Matroska),
    ("webm", InputFormat::WebM),
    ("mov", InputFormat::Mp4),
    ("ts", InputFormat::MpegTs),
    ("m2ts", InputFormat::MpegTs),
    ("mp3", InputFormat::Mp3),
    ("m4a", InputFormat::Mp4),
    ("aac", InputFormat::Aac),
    ("opus", InputFormat::Ogg),
    ("ogg", InputFormat::Ogg),
    ("flac", InputFormat::Flac),
    ("wav", InputFormat::Wav),
    ("3gp", InputFormat::Mp4),
    ("flv", InputFormat::Flv),
    ("avi", InputFormat::Avi),
];

/// Extensions of playlists, manifests and segments. A name made from a stream's link may end in one, and it is replaced by the extension of the file written, but no file is ever saved with one.
const STREAM_PARTS: [&str; 3] = ["m3u8", "mpd", "m4s"];

/// Whether `ext` (without the dot, any case) is a media extension a file may be saved with.
#[must_use]
pub fn is_media(ext: &str) -> bool {
    read_as(ext).is_some()
}

/// Every extension a saved video or audio file may have.
pub fn media() -> impl Iterator<Item = &'static str> {
    MEDIA.iter().map(|(ext, _)| *ext)
}

/// The format ffmpeg reads a file with the media extension `ext` (without the dot, any case) as.
#[must_use]
pub fn read_as(ext: &str) -> Option<InputFormat> {
    let ext = ext.to_ascii_lowercase();
    MEDIA
        .iter()
        .find(|(media, _)| *media == ext)
        .map(|(_, format)| *format)
}

/// Whether `ext` (without the dot, any case) is replaced by the extension of the file actually written: a media extension, or that of a playlist, manifest or segment.
#[must_use]
pub fn is_replaceable(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    is_media(&ext) || STREAM_PARTS.contains(&ext.as_str())
}

/// `path` with its extension set to `ext`. Only a media, playlist or segment extension is replaced: what follows any other dot is the name's own words (`Dr. Who`).
#[must_use]
pub fn with_extension(path: &Path, ext: &str) -> PathBuf {
    let replaced = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_replaceable);
    let stem = if replaced {
        path.file_stem()
    } else {
        path.file_name()
    };
    let mut name = stem.map_or_default(OsString::from);
    name.push(".");
    name.push(ext);
    path.with_file_name(name)
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
