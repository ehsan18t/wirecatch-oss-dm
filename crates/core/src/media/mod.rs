//! Video and audio from streams (HLS, DASH) and from pages that need the media
//! add-on's yt-dlp: building the quality choice, and what a chosen download
//! needs to run and to resume after a restart.

pub(crate) mod choice;
pub(crate) mod merge;
pub(crate) mod process;
pub(crate) mod ytdlp;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use wirecatch_engine::Progress;
use wirecatch_protocol::app::MediaSummary;
use wirecatch_streams::{Stage, StreamProgress};

/// Next to a media download's file while it runs: the folder for its parts
/// (video, audio, yt-dlp's own files) until they are joined.
pub(crate) const WORK_SUFFIX: &str = ".wcmedia";

/// Which track of a stream to take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub(crate) enum Pick {
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
pub(crate) enum MediaJob {
    /// An HLS or DASH stream, downloaded with `wirecatch-streams`.
    Stream { video: Pick, audio: Pick },
    /// A page read by yt-dlp. Its format addresses expire, so the page is read
    /// again on every start; the chosen formats are found again by id.
    Page {
        /// yt-dlp's format selection, for example `137+140`.
        format: String,
        /// The video format id, when the choice names one.
        video: Option<String>,
        /// The audio format id, when separate from the video.
        audio: Option<String>,
    },
    /// What to download was stored but cannot be read back (a damaged
    /// database). It never runs: downloading the link as a plain file would
    /// save a web page, or worse.
    Unreadable,
}

/// The message for a media download whose stored details cannot be read.
pub(crate) fn unreadable() -> wirecatch_protocol::app::UserError {
    wirecatch_protocol::app::UserError {
        message: "The saved details of this video could not be read, so it cannot continue. Remove it and download the video again from its page.".into(),
        fix: None,
    }
}

/// The media part of a stored download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredMedia {
    pub summary: MediaSummary,
    /// `None` for a direct media file, which downloads like any other file.
    pub job: Option<MediaJob>,
}

/// Where a media download keeps its parts while it runs.
pub(crate) fn work_dir(output: &Path) -> PathBuf {
    with_suffix(output, WORK_SUFFIX)
}

/// The folder `wirecatch-streams` keeps segments in while downloading `output`.
pub(crate) fn stream_dir(output: &Path) -> PathBuf {
    with_suffix(output, wirecatch_streams::DIR_SUFFIX)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The progress of a media download, made of the progress of its parts.
#[derive(Debug, Default)]
pub(crate) struct MediaProgress {
    parts: Mutex<Vec<Part>>,
    /// Joining or remuxing with ffmpeg.
    processing: AtomicBool,
    /// Progress reported by yt-dlp when it downloads by itself.
    own_done: AtomicU64,
    own_total: AtomicU64,
    own: AtomicBool,
    /// Waiting to try again with fresh addresses from the page.
    waiting: AtomicBool,
}

#[derive(Debug)]
enum Part {
    /// A file the engine downloads; its expected size if the source said.
    File(Arc<Progress>, Option<u64>),
    Stream(Arc<StreamProgress>),
    /// A part finished in an earlier run.
    Done(u64),
}

/// One reading of a [`MediaProgress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Reading {
    /// Data is arriving (or arrived): the source answered.
    pub answered: bool,
    pub done: u64,
    pub total: Option<u64>,
    pub connections: usize,
    pub processing: bool,
    /// Waiting to try again (a server refused, and the page is read again
    /// for fresh addresses); nothing is being received.
    pub waiting: bool,
}

impl MediaProgress {
    pub fn add_file(&self, progress: Arc<Progress>, expected: Option<u64>) {
        self.lock().push(Part::File(progress, expected));
    }

    pub fn add_stream(&self, progress: Arc<StreamProgress>) {
        self.lock().push(Part::Stream(progress));
    }

    pub fn add_done(&self, bytes: u64) {
        self.lock().push(Part::Done(bytes));
    }

    pub fn set_processing(&self, on: bool) {
        self.processing.store(on, Ordering::Relaxed);
    }

    /// Marks the download as waiting to try again, or no longer waiting.
    pub fn set_waiting(&self, on: bool) {
        self.waiting.store(on, Ordering::Relaxed);
    }

    /// Forgets the parts of an attempt that did not finish, before the next
    /// attempt adds its own (finished parts come back as such), and ends the
    /// wait. Until then the earlier attempt's progress stays visible.
    pub fn start_over(&self) {
        self.lock().clear();
        self.own.store(false, Ordering::Relaxed);
        self.own_done.store(0, Ordering::Relaxed);
        self.own_total.store(0, Ordering::Relaxed);
        self.processing.store(false, Ordering::Relaxed);
        self.waiting.store(false, Ordering::Relaxed);
    }

    /// yt-dlp's own report: bytes so far and the total, if known.
    pub fn set_own(&self, done: u64, total: Option<u64>) {
        self.own.store(true, Ordering::Relaxed);
        self.own_done.store(done, Ordering::Relaxed);
        self.own_total.store(total.unwrap_or(0), Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Part>> {
        self.parts.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn read(&self) -> Reading {
        let mut reading = Reading {
            processing: self.processing.load(Ordering::Relaxed),
            waiting: self.waiting.load(Ordering::Relaxed),
            ..Reading::default()
        };
        let mut total = Some(0u64);
        let parts = self.lock();
        for part in parts.iter() {
            let (done, part_total, connections, answered, joining) = match part {
                Part::File(p, expected) => (
                    p.done(),
                    p.total().or(*expected),
                    p.connections(),
                    p.file_name().is_some(),
                    false,
                ),
                Part::Stream(p) => {
                    let (bytes, done, all) = (p.bytes(), p.segments_done(), p.segments_total());
                    // Segments are about the same size, so the total is estimated
                    // from those done so far.
                    let estimate = (done > 0 && all > 0)
                        .then(|| bytes.saturating_mul(all as u64) / done as u64);
                    let running = p.stage() == Stage::Downloading;
                    (
                        bytes,
                        estimate,
                        usize::from(running),
                        bytes > 0,
                        p.stage() == Stage::Joining,
                    )
                }
                Part::Done(bytes) => (*bytes, Some(*bytes), 0, true, false),
            };
            reading.done += done;
            reading.connections += connections;
            reading.answered |= answered;
            reading.processing |= joining;
            total = total.zip(part_total).map(|(a, b)| a + b);
        }
        if parts.is_empty() {
            total = None;
        }
        if self.own.load(Ordering::Relaxed) {
            reading.answered = true;
            reading.done += self.own_done.load(Ordering::Relaxed);
            let own = self.own_total.load(Ordering::Relaxed);
            total = match (parts.is_empty(), own) {
                (_, 0) => None,
                (true, own) => Some(own),
                (false, own) => total.map(|t| t + own),
            };
        }
        reading.total = total.filter(|t| *t >= reading.done || reading.processing);
        reading
    }
}

/// Extensions a media file may get. A site, through yt-dlp, names the extension
/// of what it serves; taking that as is would let a page save `Invoice.exe`
/// (yt-dlp's CVE-2024-38519), so anything else is replaced.
const MEDIA_EXTENSIONS: [&str; 16] = [
    "mp4", "m4a", "webm", "mkv", "mka", "mp3", "opus", "ogg", "aac", "flac", "wav", "mov", "ts",
    "3gp", "m4v", "flv",
];

/// Whether `ext` (without the dot, any case) is a media extension.
pub(crate) fn is_media_ext(ext: &str) -> bool {
    MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// What a media file holds, which decides the extension used instead of one
/// that is not allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Content {
    Video,
    Audio,
    /// Not known, or video and audio joined from different containers.
    Unknown,
}

/// `ext` in lower case if it is a media extension, otherwise `mp4` for video,
/// `m4a` for audio and `mkv` when unknown.
pub(crate) fn safe_ext(ext: Option<&str>, content: Content) -> String {
    let lower = ext
        .map(|e| e.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if MEDIA_EXTENSIONS.contains(&lower.as_str()) {
        return lower;
    }
    match content {
        Content::Video => "mp4",
        Content::Audio => "m4a",
        Content::Unknown => "mkv",
    }
    .to_string()
}

/// A file name for media called `title`, with extension `ext` (replaced by
/// `mkv` if it is not a media extension).
pub(crate) fn file_name(title: &str, ext: &str) -> String {
    let ext = safe_ext(Some(ext), Content::Unknown);
    let title = title.trim();
    let title = if title.is_empty() { "video" } else { title };
    // Leave room for " (1)" and the extension within common name limits.
    let mut stem: String = title.chars().take(150).collect();
    stem = wirecatch_engine::naming::sanitize(&stem);
    let stem = stem.trim_end_matches('.');
    format!("{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_media_extensions_are_kept() {
        for (given, content, kept) in [
            (Some("MP4"), Content::Video, "mp4"),
            (Some("webm"), Content::Unknown, "webm"),
            (Some("exe"), Content::Video, "mp4"),
            (Some("bat"), Content::Audio, "m4a"),
            (Some("lnk"), Content::Unknown, "mkv"),
            (Some(""), Content::Video, "mp4"),
            (None, Content::Audio, "m4a"),
            (Some("mp4.exe"), Content::Video, "mp4"),
            (Some("../mp4"), Content::Video, "mp4"),
            (Some("scr"), Content::Video, "mp4"),
        ] {
            assert_eq!(safe_ext(given, content), kept, "{given:?}");
        }
        assert_eq!(file_name("Invoice", "exe"), "Invoice.mkv");
    }

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(
            file_name("My: video?", "mp4"),
            wirecatch_engine::naming::sanitize("My: video?") + ".mp4"
        );
        assert_eq!(file_name("   ", "m4a"), "video.m4a");
        assert!(file_name(&"x".repeat(400), "mp4").len() < 200);
    }

    #[test]
    fn the_job_is_stored_as_plain_json() {
        let job = MediaJob::Page {
            format: "137+140".into(),
            video: Some("137".into()),
            audio: Some("140".into()),
        };
        let json = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), job);
        let job = MediaJob::Stream {
            video: Pick::Track("v1".into()),
            audio: Pick::Skip,
        };
        let json = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), job);
    }

    #[test]
    fn progress_sums_its_parts() {
        let progress = MediaProgress::default();
        assert_eq!(progress.read().total, None);
        progress.add_done(10);
        progress.add_file(Arc::new(Progress::default()), Some(30));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (10, Some(40)));
        assert!(reading.answered);
        assert!(!reading.waiting);

        // Waiting to try again keeps what is shown until the next attempt
        // starts over with its own parts.
        progress.set_waiting(true);
        let waiting = progress.read();
        assert!(waiting.waiting);
        assert_eq!((waiting.done, waiting.total), (10, Some(40)));
        progress.start_over();
        progress.add_done(10);
        let again = progress.read();
        assert!(!again.waiting);
        assert_eq!((again.done, again.total), (10, Some(10)));
    }
}
