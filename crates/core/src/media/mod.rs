//! Video and audio from streams (HLS, DASH) and from pages that need the media
//! add-on's yt-dlp: building the quality choice, and what a chosen download
//! needs to run and to resume after a restart.

pub(crate) mod choice;
pub(crate) mod convert;
pub(crate) mod merge;
pub(crate) mod output;
pub(crate) mod process;
pub(crate) mod ytdlp;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use wirecatch_engine::Progress;
use wirecatch_protocol::app::{MediaSummary, PER_MILLE_DONE, ProcessingProgress, ProcessingStep};
use wirecatch_streams::{Stage, StreamProgress};

use output::OutputJob;

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
        /// How the downloaded video or audio is saved (D24); `None` keeps it
        /// as downloaded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Box<OutputJob>>,
        /// The extension the joined video gets when it is kept as downloaded
        /// after a conversion failed ("Keep the original"); `None` takes the
        /// planned file's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_ext: Option<String>,
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
#[derive(Debug)]
pub(crate) struct MediaProgress {
    parts: Mutex<Vec<Part>>,
    /// What ffmpeg does now: one of the `STEP_` values.
    step: AtomicU8,
    /// Tenths of a percent of that step done; [`UNKNOWN`] when not known.
    per_mille: AtomicU16,
    /// Progress reported by yt-dlp when it downloads by itself.
    own_done: AtomicU64,
    own_total: AtomicU64,
    own: AtomicBool,
    /// When the next try with fresh addresses from the page starts, in Unix
    /// milliseconds; 0 while not waiting.
    retry_at: AtomicI64,
}

const STEP_NONE: u8 = 0;
const STEP_JOINING: u8 = 1;
const STEP_FINISHING: u8 = 2;
const STEP_CONVERTING: u8 = 3;
/// How far a step is, when that is not known.
const UNKNOWN: u16 = u16::MAX;

impl Default for MediaProgress {
    fn default() -> Self {
        Self {
            parts: Mutex::default(),
            step: AtomicU8::new(STEP_NONE),
            per_mille: AtomicU16::new(UNKNOWN),
            own_done: AtomicU64::default(),
            own_total: AtomicU64::default(),
            own: AtomicBool::default(),
            retry_at: AtomicI64::default(),
        }
    }
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
    /// What ffmpeg does now, if anything, and how far it is.
    pub processing: Option<ProcessingProgress>,
    /// Waiting to try again (a server refused, and the page is read again
    /// for fresh addresses) at this Unix time in milliseconds; nothing is being
    /// received meanwhile.
    pub retry_at: Option<i64>,
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

    /// Marks ffmpeg as doing `step` now (with nothing of it known done yet), or,
    /// with `None`, as done.
    pub fn set_step(&self, step: Option<ProcessingStep>) {
        let value = match step {
            None => STEP_NONE,
            Some(ProcessingStep::Joining) => STEP_JOINING,
            Some(ProcessingStep::Finishing) => STEP_FINISHING,
            Some(ProcessingStep::Converting) => STEP_CONVERTING,
        };
        self.per_mille.store(UNKNOWN, Ordering::Relaxed);
        self.step.store(value, Ordering::Relaxed);
    }

    /// How far the current step is, in tenths of a percent, when known.
    pub fn set_per_mille(&self, done: Option<u16>) {
        self.per_mille.store(
            done.map_or(UNKNOWN, |d| d.min(PER_MILLE_DONE)),
            Ordering::Relaxed,
        );
    }

    fn processing(&self) -> Option<ProcessingProgress> {
        let step = match self.step.load(Ordering::Relaxed) {
            STEP_JOINING => ProcessingStep::Joining,
            STEP_FINISHING => ProcessingStep::Finishing,
            STEP_CONVERTING => ProcessingStep::Converting,
            _ => return None,
        };
        let per_mille = Some(self.per_mille.load(Ordering::Relaxed)).filter(|p| *p != UNKNOWN);
        Some(ProcessingProgress { step, per_mille })
    }

    /// Marks the download as waiting to try again at `at` (Unix milliseconds),
    /// or, with `None`, no longer waiting.
    pub fn set_retry_at(&self, at: Option<i64>) {
        self.retry_at
            .store(at.filter(|t| *t > 0).unwrap_or(0), Ordering::Relaxed);
    }

    /// Forgets the parts of an attempt that did not finish, before the next
    /// attempt adds its own (finished parts come back as such), and ends the
    /// wait. Until then the earlier attempt's progress stays visible.
    pub fn start_over(&self) {
        self.lock().clear();
        self.own.store(false, Ordering::Relaxed);
        self.own_done.store(0, Ordering::Relaxed);
        self.own_total.store(0, Ordering::Relaxed);
        self.set_step(None);
        self.retry_at.store(0, Ordering::Relaxed);
    }

    /// yt-dlp's own report: bytes so far and the total, if known.
    pub fn set_own(&self, done: u64, total: Option<u64>) {
        self.own.store(true, Ordering::Relaxed);
        self.own_done.store(done, Ordering::Relaxed);
        self.own_total.store(total.unwrap_or(0), Ordering::Relaxed);
    }

    /// Locks the parts. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason `Manager::lock` gives; each part is
    /// replaced whole, so it is never half written either.
    fn lock(&self) -> MutexGuard<'_, Vec<Part>> {
        self.parts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn read(&self) -> Reading {
        let mut reading = Reading {
            processing: self.processing(),
            retry_at: Some(self.retry_at.load(Ordering::Relaxed)).filter(|t| *t > 0),
            ..Reading::default()
        };
        let mut stream_joining = false;
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
        reading.total = total.filter(|t| *t >= reading.done || reading.processing.is_some());
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
    fn a_conversion_stored_by_the_previous_build_is_read_as_it_was() {
        let read = |json: &str| -> output::OutputJob {
            let job: MediaJob = serde_json::from_str(json).unwrap();
            let MediaJob::Page {
                output: Some(output),
                ..
            } = job
            else {
                panic!("not a page with an output: {job:?}");
            };
            *output
        };
        // As the build before `Extras` wrote it: chapters and tags are fields of
        // the job itself.
        let stored = read(
            r#"{"type":"page","format":"313+140","video":"313","audio":"140","output":{"container":"mp4","video":{"type":"encode","codec":"h264","kbps":8000},"audio":{"type":"copy"},"subtitles":[],"subtitle_files":false,"chapters":true,"tags":true,"duration_ms":1000,"original_ext":"webm","original":false}}"#,
        );
        assert_eq!(
            stored.extras,
            output::Extras {
                chapters: true,
                tags: true,
            }
        );
        assert_eq!(stored.container, output::Container::Mp4);
        assert_eq!(stored.duration_ms, Some(1000));
        // Without them (both are optional when read): neither is added.
        let older = read(
            r#"{"type":"page","format":"313+140","video":"313","audio":"140","output":{"container":"mp4","video":{"type":"copy"},"audio":{"type":"copy"},"original_ext":"webm"}}"#,
        );
        assert_eq!(older.extras, output::Extras::default());
        assert!(!older.subtitle_files && !older.original);
    }

    #[test]
    fn the_job_is_stored_as_plain_json() {
        let job = MediaJob::Page {
            format: "137+140".into(),
            video: Some("137".into()),
            audio: Some("140".into()),
            output: None,
            keep_ext: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        // Stored as before output formats existed, and read back.
        assert_eq!(
            json,
            r#"{"type":"page","format":"137+140","video":"137","audio":"140"}"#
        );
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), job);
        let converting = MediaJob::Page {
            format: "313+140".into(),
            video: Some("313".into()),
            audio: Some("140".into()),
            output: Some(Box::new(OutputJob {
                container: output::Container::Mp4,
                video: output::VideoAction::Encode {
                    codec: output::VideoCodec::H264,
                    kbps: 8000,
                },
                audio: output::AudioAction::Copy,
                subtitles: vec![output::SubtitlePick {
                    key: "en-orig".into(),
                    auto: true,
                    language: "en".into(),
                    name: "English".into(),
                }],
                subtitle_files: false,
                extras: output::Extras {
                    chapters: true,
                    tags: false,
                },
                duration_ms: Some(1000),
                original_ext: "webm".into(),
                original: false,
            })),
            keep_ext: None,
        };
        let json = serde_json::to_string(&converting).unwrap();
        // The extras are stored as fields of the job, as jobs saved before were.
        assert!(
            json.contains(r#""subtitle_files":false,"chapters":true,"tags":false,"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<MediaJob>(&json).unwrap(), converting);
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
}
