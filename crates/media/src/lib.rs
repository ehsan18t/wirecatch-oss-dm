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
use wirecatch_protocol::UnixMillis;
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
    own: Option<OwnProgress>,
    /// When the next try with fresh addresses from the page starts, in Unix milliseconds.
    retry_at: Option<UnixMillis>,
}

/// What is known of yt-dlp's own download, format by format: it counts each from nothing, and a second run reports again the formats it kept.
#[derive(Debug, Default)]
struct OwnProgress {
    formats: Vec<OwnFormat>,
    /// The format last reported on.
    current: Option<usize>,
    /// The bytes and the total last read, whose ratio the bar shows.
    shown: Option<(u64, u64)>,
}

#[derive(Debug)]
struct OwnFormat {
    id: String,
    done: u64,
    total: Option<u64>,
}

impl OwnProgress {
    fn place_of(&mut self, id: &str) -> usize {
        self.formats
            .iter()
            .position(|f| f.id == id)
            .unwrap_or_else(|| {
                self.shown = None;
                self.formats.push(OwnFormat {
                    id: id.to_owned(),
                    done: 0,
                    total: None,
                });
                self.formats.len() - 1
            })
    }

    fn report(&mut self, id: &str, done: u64, total: Option<u64>) {
        let place = self.place_of(id);
        if let Some(format) = self.formats.get_mut(place) {
            format.done = done;
            format.total = total.or(format.total);
        }
        self.current = Some(place);
    }

    /// Bytes so far, and the total once every format's is known. A format left behind without one is as long as what it got.
    fn sum(&self) -> (u64, Option<u64>) {
        let mut done = 0_u64;
        let mut total = Some(0_u64);
        for (place, format) in self.formats.iter().enumerate() {
            done = done.saturating_add(format.done);
            let left_behind = format.done > 0 && self.current != Some(place);
            let size = format.total.or(left_behind.then_some(format.done));
            total = total.zip(size).map(|(all, size)| all.saturating_add(size));
        }
        (done, total)
    }

    /// `total` as read now: while one above what was shown would move the bar back, it follows `done` at the ratio shown, until the real one is reached (D173).
    fn steady_total(&mut self, done: u64, total: u64) -> u64 {
        let steady = match self.shown {
            Some((shown_done, shown_total))
                if total > shown_total && done >= shown_done && shown_done > 0 =>
            {
                let held = u128::from(done) * u128::from(shown_total) / u128::from(shown_done);
                u64::try_from(held).map_or(total, |held| held.min(total))
            }
            _ => total,
        };
        if steady >= done {
            self.shown = Some((done, steady));
        }
        steady
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
    pub retry_at: Option<UnixMillis>,
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

    /// Counts `file`, finished, among the parts a later run keeps, whatever its server allows.
    pub fn finish_file(&self, file: &Arc<Progress>) {
        for part in &mut self.lock().parts {
            if let Part::File(progress, _) = part
                && Arc::ptr_eq(progress, file)
            {
                *part = Part::Done(file.done());
            }
        }
    }

    /// Whether a later run continues from anything this one has (D153).
    pub fn keeps_data(&self) -> bool {
        let state = self.lock();
        let own = state.own.as_ref().is_some_and(|own| own.sum().0 > 0);
        own || state.parts.iter().any(|part| match part {
            Part::File(file, _) => file.done() > 0 && file.can_continue(),
            Part::Stream(stream) => stream.segments_done() > 0,
            Part::Done(bytes) => *bytes > 0,
        })
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
    pub fn set_retry_at(&self, at: Option<UnixMillis>) {
        self.lock().retry_at = at.filter(|t| t.0 > 0);
    }

    /// Forgets the parts of an attempt that did not finish, before the next
    /// attempt adds its own (finished parts come back as such), and ends the
    /// wait. Until then the earlier attempt's progress stays visible.
    pub fn start_over(&self) {
        *self.lock() = State::default();
    }

    /// The formats yt-dlp is about to download and their sizes where the page says, so the total is whole from the start.
    pub fn expect_own(&self, formats: impl Iterator<Item = (String, Option<u64>)>) {
        let mut state = self.lock();
        let own = state.own.get_or_insert_default();
        for (id, size) in formats {
            let place = own.place_of(&id);
            if let Some(format) = own.formats.get_mut(place) {
                format.total = format.total.or(size);
            }
        }
    }

    /// yt-dlp's own report on `format`: bytes so far and the total, if known. What it reported of other formats stays counted.
    pub fn set_own(&self, format: &str, done: u64, total: Option<u64>) {
        self.lock()
            .own
            .get_or_insert_default()
            .report(format, done, total);
    }

    /// Locks the state. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason `Manager::lock` gives; each field is
    /// replaced whole, so it is never half written either.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn read(&self) -> Reading {
        let mut guard = self.lock();
        let state = &mut *guard;
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
            reading.done = reading.done.saturating_add(done);
            reading.connections += connections;
            reading.answered |= answered;
            stream_joining |= joining;
            total = total.zip(part_total).map(|(a, b)| a.saturating_add(b));
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
        if let Some(own) = &mut state.own {
            let (own_done, own_total) = own.sum();
            let own_total = own_total.map(|total| own.steady_total(own_done, total));
            reading.answered |= own.current.is_some();
            reading.done = reading.done.saturating_add(own_done);
            total = match (parts.is_empty(), own_total) {
                (_, None) => None,
                (true, own) => own,
                (false, Some(own)) => total.map(|t| t.saturating_add(own)),
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
    // A title is no path: `sanitize` keeps only what follows the last slash of a server's name.
    let mut stem: String = title
        .chars()
        .take(MAX_TITLE_CHARS)
        .map(|c| if matches!(c, '/' | '\\') { '-' } else { c })
        .collect();
    stem = wirecatch_engine::naming::sanitize(&stem);
    let stem = stem.trim_end_matches('.');
    format!("{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_keeps_what_is_before_its_slash() {
        assert_eq!(
            file_name("AC/DC - Thunderstruck", "mp4"),
            "AC-DC - Thunderstruck.mp4"
        );
        assert_eq!(file_name("Episode 1\\10", "mp4"), "Episode 1-10.mp4");
        assert_ne!(
            file_name("Episode 1/10", "mp4"),
            file_name("Episode 2/10", "mp4")
        );
    }

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
        progress.set_retry_at(Some(UnixMillis(1_000)));
        let waiting = progress.read();
        assert_eq!(waiting.retry_at, Some(UnixMillis(1_000)));
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
        progress.set_own("", 10, Some(20));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (10, Some(20)));
        progress.start_over();
        assert_eq!(progress.read(), Reading::default());
    }

    /// What the page says of each format makes the total whole before the audio starts, so the bar never falls back.
    #[test]
    fn yt_dlps_total_is_whole_from_the_page_sizes() {
        let progress = MediaProgress::default();
        progress
            .expect_own([("137".to_owned(), Some(100)), ("140".to_owned(), Some(20))].into_iter());
        assert!(!progress.read().answered);
        progress.set_own("137", 100, Some(100));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (100, Some(120)));
        progress.set_own("140", 20, Some(20));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (120, Some(120)));
    }

    /// The page's size was an estimate: the real one above it holds the bar still until the bytes catch up, and the total ends at the real size.
    #[test]
    fn a_real_size_above_the_estimate_never_moves_the_bar_back() {
        let progress = MediaProgress::default();
        progress.expect_own([("137".to_owned(), Some(100))].into_iter());
        let shown = |done, total| {
            progress.set_own("137", done, Some(total));
            let reading = progress.read();
            (reading.done, reading.total)
        };
        assert_eq!(shown(50, 100), (50, Some(100)));
        assert_eq!(shown(50, 200), (50, Some(100)));
        assert_eq!(
            progress.read().total,
            Some(100),
            "a second look changes nothing"
        );
        assert_eq!(shown(60, 200), (60, Some(120)));
        assert_eq!(shown(100, 200), (100, Some(200)));
        assert_eq!(shown(150, 200), (150, Some(200)));
    }

    /// yt-dlp's estimate of a fragmented format grows as it goes.
    #[test]
    fn a_growing_estimate_never_moves_the_bar_back() {
        let progress = MediaProgress::default();
        let (mut last_done, mut last_total) = (0_u64, 1_u64);
        for (done, estimate) in [
            (10, 40),
            (20, 90),
            (30, 95),
            (40, 300),
            (50, 310),
            (299, 310),
        ] {
            progress.set_own("hls", done, Some(estimate));
            let reading = progress.read();
            let total = reading.total.unwrap();
            assert!(
                reading.done * last_total >= last_done * total,
                "{}/{total} fell below {last_done}/{last_total}",
                reading.done
            );
            (last_done, last_total) = (reading.done, total);
        }
        assert_eq!(progress.read().total, Some(310));
    }

    /// A format begun again has nothing to hold: the bar shows how far it really is.
    #[test]
    fn a_format_begun_again_reads_its_real_total() {
        let progress = MediaProgress::default();
        progress.set_own("137", 50, Some(100));
        assert_eq!(progress.read().total, Some(100));
        progress.set_own("137", 10, Some(200));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (10, Some(200)));
    }

    /// A format no size was expected for is more to get, not a better estimate: the bar shows the real share.
    #[test]
    fn a_format_not_expected_reads_the_real_total() {
        let progress = MediaProgress::default();
        progress.set_own("video", 100, Some(100));
        assert_eq!(progress.read().total, Some(100));
        progress.set_own("audio", 10, Some(200));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (110, Some(300)));
    }

    /// Bytes past the estimate show no total until a larger one is known, which then holds the bar where it was.
    #[test]
    fn bytes_past_the_estimate_keep_the_ratio_shown_before() {
        let progress = MediaProgress::default();
        progress.set_own("137", 90, Some(100));
        assert_eq!(progress.read().total, Some(100));
        progress.set_own("137", 110, Some(100));
        assert_eq!(progress.read().total, None);
        progress.set_own("137", 180, Some(400));
        assert_eq!(progress.read().total, Some(200));
    }

    /// A format the page does not size leaves the total unknown rather than short.
    #[test]
    fn yt_dlps_total_waits_for_a_format_without_a_size() {
        let progress = MediaProgress::default();
        progress.expect_own([("137".to_owned(), Some(100)), ("140".to_owned(), None)].into_iter());
        progress.set_own("137", 100, Some(100));
        assert_eq!(progress.read().total, None);
    }

    /// A row with no saved formats runs yt-dlp's best selection, whose pick the page names as `137+140`.
    #[test]
    fn the_best_selections_total_waits_for_every_format_it_takes() {
        let info: ytdlp::Info = serde_json::from_str(
            r#"{"format_id":"137+140","formats":[
                {"format_id":"137","filesize":100},{"format_id":"140"},{"format_id":"18","filesize":7}
            ]}"#,
        )
        .unwrap();
        let progress = MediaProgress::default();
        progress.expect_own(info.best_formats().into_iter());
        progress.set_own("137", 100, Some(100));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (100, None));
        progress.set_own("140", 1, Some(20));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (101, Some(120)));
    }

    /// A second run, with the browser's sign-in, reports again the format it kept.
    #[test]
    fn a_format_reported_again_counts_once() {
        let progress = MediaProgress::default();
        for (id, done, total) in [("137", 100, 100), ("140", 20, 20), ("137", 100, 100)] {
            progress.set_own(id, done, Some(total));
        }
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (120, Some(120)));
    }

    /// yt-dlp counts each format from nothing, so the audio after a video must add to it.
    #[test]
    fn yt_dlps_counts_add_up_over_its_formats() {
        let progress = MediaProgress::default();
        progress.set_own("137", 90, Some(100));
        progress.set_own("137", 100, Some(100));
        progress.set_own("140", 1, Some(20));
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (101, Some(120)));
        // A format begun again replaces its own count.
        progress.set_own("140", 0, Some(20));
        assert_eq!(progress.read().done, 100);
        // A format whose total is unknown leaves the sum unknown, but its bytes count.
        progress.set_own("141", 5, None);
        let reading = progress.read();
        assert_eq!((reading.done, reading.total), (105, None));
    }
}
