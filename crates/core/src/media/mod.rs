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

/// A file name for media called `title`, with extension `ext`.
pub(crate) fn file_name(title: &str, ext: &str) -> String {
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
    }
}
