//! The names of the files a media download keeps in its work folder, which a later run reads back.

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

/// Names the file a finished part (`role`) was saved as.
pub(super) fn marker(role: &str) -> String {
    format!("{role}.done")
}
