//! Video and audio from streams (HLS, DASH) and from pages that need the media
//! add-on's yt-dlp: building the quality choice, and what a chosen download
//! needs to run and to resume after a restart.

#![forbid(unsafe_code)]

pub mod choice;
pub mod convert;
pub mod describe;
pub mod ffmpeg;
pub mod merge;
pub mod output;
mod tools;
pub mod ytdlp;

pub use tools::MediaTools;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wirecatch_engine::Progress;
use wirecatch_files::with_suffix;
use wirecatch_protocol::app::{PER_MILLE_DONE, ProcessingProgress, ProcessingStep};
use wirecatch_streams::{Stage, StreamProgress};

pub use wirecatch_records::media::{MediaJob, Pick, StoredMedia, unreadable};

/// Next to a media download's file while it runs: the folder for its parts
/// (video, audio, yt-dlp's own files) until they are joined.
pub const WORK_SUFFIX: &str = ".wcmedia";

#[must_use]
pub fn work_dir(output: &Path) -> PathBuf {
    with_suffix(output, WORK_SUFFIX)
}

/// Where `wirecatch-streams` keeps segments while downloading `output`.
#[must_use]
pub fn stream_dir(output: &Path) -> PathBuf {
    with_suffix(output, wirecatch_streams::DIR_SUFFIX)
}

/// The progress of a media download, made of the progress of its parts. Updates come a few times a second at most, so one lock over all of it costs nothing and every reading is consistent.
#[derive(Debug, Default)]
pub struct MediaProgress {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    parts: Vec<Part>,
    /// What ffmpeg does now, and how far it is in tenths of a percent when known.
    step: Option<(ProcessingStep, Option<u16>)>,
    /// yt-dlp's own report when it downloads by itself: bytes so far and the total, when known.
    own: Option<(u64, Option<u64>)>,
    /// When the next try with fresh addresses from the page starts, in Unix milliseconds.
    retry_at: Option<i64>,
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
pub struct Reading {
    /// Data is arriving (or arrived): the source answered.
    pub answered: bool,
    pub done: u64,
    pub total: Option<u64>,
    pub connections: usize,
    /// What ffmpeg does now, if anything, and how far it is.
    pub processing: Option<ProcessingProgress>,
    /// Waiting to try again (a server refused, and the page is read again
    /// for fresh addresses) at this Unix time in milliseconds; nothing is being
    /// received meanwhile.
    pub retry_at: Option<i64>,
}

impl MediaProgress {
    pub fn add_file(&self, progress: Arc<Progress>, expected: Option<u64>) {
        self.lock().parts.push(Part::File(progress, expected));
    }

    pub fn add_stream(&self, progress: Arc<StreamProgress>) {
        self.lock().parts.push(Part::Stream(progress));
    }

    pub fn add_done(&self, bytes: u64) {
        self.lock().parts.push(Part::Done(bytes));
    }

    /// Marks ffmpeg as doing `step` now (with nothing of it known done yet), or,
    /// with `None`, as done.
    pub fn set_step(&self, step: Option<ProcessingStep>) {
        self.lock().step = step.map(|s| (s, None));
    }

    /// How far the current step is, in tenths of a percent, when known.
    pub fn set_per_mille(&self, done: Option<u16>) {
        if let Some((_, per_mille)) = &mut self.lock().step {
            *per_mille = done.map(|d| d.min(PER_MILLE_DONE));
        }
    }

    /// Marks the download as waiting to try again at `at` (Unix milliseconds),
    /// or, with `None`, no longer waiting.
    pub fn set_retry_at(&self, at: Option<i64>) {
        self.lock().retry_at = at.filter(|t| *t > 0);
    }

    /// Forgets the parts of an attempt that did not finish, before the next
    /// attempt adds its own (finished parts come back as such), and ends the
    /// wait. Until then the earlier attempt's progress stays visible.
    pub fn start_over(&self) {
        *self.lock() = State::default();
    }

    /// yt-dlp's own report: bytes so far and the total, if known.
    pub fn set_own(&self, done: u64, total: Option<u64>) {
        self.lock().own = Some((done, total));
    }

    /// Locks the state. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason `Manager::lock` gives; each field is
    /// replaced whole, so it is never half written either.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn read(&self) -> Reading {
        let state = self.lock();
        let mut reading = Reading {
            processing: state
                .step
                .map(|(step, per_mille)| ProcessingProgress { step, per_mille }),
            retry_at: state.retry_at,
            ..Reading::default()
        };
        let mut stream_joining = false;
        let mut total = Some(0u64);
        let parts = &state.parts;
        for part in parts {
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
            stream_joining |= joining;
            total = total.zip(part_total).map(|(a, b)| a + b);
        }
        if stream_joining && reading.processing.is_none() {
            reading.processing = Some(ProcessingProgress {
                step: ProcessingStep::Joining,
                per_mille: None,
            });
        }
        if parts.is_empty() {
            total = None;
        }
        if let Some((own_done, own_total)) = state.own {
            reading.answered = true;
            reading.done += own_done;
            total = match (parts.is_empty(), own_total) {
                (_, None) => None,
                (true, own) => own,
                (false, Some(own)) => total.map(|t| t + own),
            };
        }
        reading.total = total.filter(|t| *t >= reading.done || reading.processing.is_some());
        reading
    }
}

/// Whether `ext` (without the dot, any case) is a media extension a file may be saved with (see [`wirecatch_streams::extension`]).
#[must_use]
pub fn is_media_ext(ext: &str) -> bool {
    wirecatch_streams::extension::is_media(ext)
}

/// What a media file holds, which decides the extension used instead of one
/// that is not allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    Video,
    Audio,
    /// Not known, or video and audio joined from different containers.
    Unknown,
}

/// `ext` in lower case if it is a media extension, otherwise `mp4` for video,
/// `m4a` for audio and `mkv` when unknown.
#[must_use]
pub fn safe_ext(ext: Option<&str>, content: Content) -> String {
    let lower = ext
        .map(|e| e.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if is_media_ext(&lower) {
        return lower;
    }
    match content {
        Content::Video => "mp4",
        Content::Audio => "m4a",
        Content::Unknown => "mkv",
    }
    .to_string()
}

/// The most characters of a title used in a file name, leaving room for " (1)" and the extension within common name limits.
const MAX_TITLE_CHARS: usize = 150;

/// A file name for media called `title`, with extension `ext` (replaced by
/// `mkv` if it is not a media extension).
#[must_use]
pub fn file_name(title: &str, ext: &str) -> String {
    let ext = safe_ext(Some(ext), Content::Unknown);
    let title = title.trim();
    let title = if title.is_empty() { "video" } else { title };
    let mut stem: String = title.chars().take(MAX_TITLE_CHARS).collect();
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
    fn progress_sums_its_parts() {
        let progress = MediaProgress::default();
        assert_eq!(progress.read().total, None);
        progress.add_done(10);
        progress.add_file(Arc::new(Progress::default()), Some(30));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (10, Some(40)));
        assert!(reading.answered);
        assert_eq!(reading.retry_at, None);

        // Waiting to try again keeps what is shown until the next attempt
        // starts over with its own parts.
        progress.set_retry_at(Some(1_000));
        let waiting = progress.read();
        assert_eq!(waiting.retry_at, Some(1_000));
        assert_eq!((waiting.done, waiting.total), (10, Some(40)));
        progress.start_over();
        progress.add_done(10);
        let again = progress.read();
        assert_eq!(again.retry_at, None);
        assert_eq!((again.done, again.total), (10, Some(10)));
    }

    #[test]
    fn a_step_and_yt_dlp_progress_read_as_set_and_start_over_clears_them_together() {
        let progress = MediaProgress::default();
        progress.set_per_mille(Some(500));
        assert_eq!(
            progress.read().processing,
            None,
            "no step, nothing to report"
        );
        progress.set_step(Some(ProcessingStep::Converting));
        progress.set_per_mille(Some(5_000));
        assert_eq!(
            progress.read().processing,
            Some(ProcessingProgress {
                step: ProcessingStep::Converting,
                per_mille: Some(PER_MILLE_DONE),
            })
        );
        progress.set_own(10, Some(20));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (10, Some(20)));
        progress.start_over();
        assert_eq!(progress.read(), Reading::default());
    }
}
