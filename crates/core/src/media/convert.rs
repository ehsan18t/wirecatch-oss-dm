//! Running the media add-on's ffmpeg for output formats (D24): finding a video
//! encoder that works on this computer, converting with progress, and turning
//! subtitles into `SubRip` files.
//!
//! Which hardware encoder works is found once per ffmpeg by encoding a short
//! generated clip, and remembered while the app runs. A hardware encoder that
//! then fails on a real video (too large for it, say) is skipped from then on,
//! and the conversion goes on with the next one, ending with the software
//! encoder.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use tokio::sync::OnceCell;
use tracing::{info, warn};
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::ProcessingStep;

use super::MediaProgress;
use super::output::{self, Encoder, Inputs, OutputJob, SubtitleFormat, VideoAction, VideoCodec};
use super::process::{self, Output, RunError};
use crate::addons::MediaTools;

/// How long asking ffmpeg for its encoders, or one encoder test, may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// Identifies one ffmpeg program: its path and when it last changed, so an
/// updated add-on is asked again.
type ProgramKey = (PathBuf, Option<SystemTime>);

/// The encoders each ffmpeg lists (`None` when it could not be asked).
type Listed = Arc<OnceCell<Option<Arc<HashSet<String>>>>>;

/// What is known about the ffmpeg programs used so far.
#[derive(Default)]
struct Known {
    listed: HashMap<ProgramKey, Listed>,
    /// The video encoders to use for each codec, in order.
    video: HashMap<(ProgramKey, VideoCodec), Arc<OnceCell<Vec<&'static Encoder>>>>,
    /// Hardware encoders that failed on a real video.
    failed: HashSet<(ProgramKey, &'static str)>,
}

fn known() -> std::sync::MutexGuard<'static, Known> {
    static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Mutex::default);
    // A lock poisoned by a panic in another holder is taken over as it is, for
    // the reason `Manager::lock` gives: the data is plain and still sound.
    KNOWN.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn program_key(ffmpeg: &Path) -> ProgramKey {
    let modified = tokio::fs::metadata(ffmpeg)
        .await
        .ok()
        .and_then(|m| m.modified().ok());
    (ffmpeg.to_path_buf(), modified)
}

/// The encoders ffmpeg lists, asked once per ffmpeg.
async fn listed(tools: &MediaTools, key: &ProgramKey) -> Option<Arc<HashSet<String>>> {
    let cell = known().listed.entry(key.clone()).or_default().clone();
    cell.get_or_init(|| async {
        let args: Vec<OsString> = ["-hide_banner", "-nostdin", "-encoders"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let dir = tools.ffmpeg.parent().unwrap_or(Path::new("."));
        let ran = process::run(
            &tools.ffmpeg,
            &args,
            dir,
            true,
            Some(PROBE_TIMEOUT),
            &CancellationToken::new(),
            |_, _| {},
        )
        .await;
        match ran {
            Ok(ran) if ran.success => {
                let found = output::listed_encoders(&String::from_utf8_lossy(&ran.stdout));
                (!found.is_empty()).then(|| Arc::new(found))
            }
            _ => None,
        }
    })
    .await
    .clone()
}

/// The video encoders for `codec`, in the order to use them: the first
/// hardware encoder that encodes a test clip on this computer, then the
/// software ones. Found once per ffmpeg; `None` when cancelled.
async fn video_encoders(
    tools: &MediaTools,
    key: &ProgramKey,
    codec: VideoCodec,
    cancel: &CancellationToken,
) -> Option<Vec<&'static Encoder>> {
    let listed = listed(tools, key).await;
    let cell = known()
        .video
        .entry((key.clone(), codec))
        .or_default()
        .clone();
    let found = cell
        .get_or_try_init(|| async {
            let candidates = output::encoders_for(codec, std::env::consts::OS);
            let order = output::encoder_order(&candidates, listed.as_deref(), &HashSet::new());
            let mut chosen = Vec::new();
            for encoder in order.iter().filter(|e| e.hardware) {
                match probe(tools, encoder, cancel).await {
                    Err(()) => return Err(()),
                    Ok(true) => {
                        info!(
                            encoder = encoder.name,
                            "a hardware video encoder works here"
                        );
                        chosen.push(*encoder);
                        break;
                    }
                    Ok(false) => {}
                }
            }
            chosen.extend(order.iter().filter(|e| !e.hardware));
            Ok(chosen)
        })
        .await
        .ok()?;
    let known = known();
    Some(
        found
            .iter()
            .filter(|e| !known.failed.contains(&(key.clone(), e.name)))
            .copied()
            .collect(),
    )
}

/// Whether `encoder` encodes a short generated clip; `Err` when cancelled.
async fn probe(
    tools: &MediaTools,
    encoder: &Encoder,
    cancel: &CancellationToken,
) -> Result<bool, ()> {
    let dir = tools.ffmpeg.parent().unwrap_or(Path::new("."));
    match process::run(
        &tools.ffmpeg,
        &output::probe_arguments(encoder),
        dir,
        false,
        Some(PROBE_TIMEOUT),
        cancel,
        |_, _| {},
    )
    .await
    {
        Ok(ran) => Ok(ran.success),
        Err(RunError::Cancelled) => Err(()),
        Err(_) => Ok(false),
    }
}

/// How a conversion ended without failing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Converted {
    Done,
    Cancelled,
}

/// Why a conversion failed.
#[derive(Debug)]
pub(crate) enum ConvertError {
    /// ffmpeg ran and refused or failed, for this reason.
    Failed(String),
    /// ffmpeg could not be started.
    Start(std::io::Error),
}

/// The step a conversion is, as the person sees it.
pub(crate) fn step(job: &OutputJob, inputs: &Inputs) -> ProcessingStep {
    if job.converts() {
        ProcessingStep::Converting
    } else if inputs.video.is_some() && inputs.audio.is_some() {
        ProcessingStep::Joining
    } else {
        ProcessingStep::Finishing
    }
}

/// Saves `inputs` into `output` as `job` says, reporting into `progress`. A
/// video to re-encode goes through the encoders that work here in order; when
/// a hardware encoder fails on this video, the next one is tried. Whatever
/// ends, a partial `output` is removed.
pub(crate) async fn convert(
    tools: &MediaTools,
    job: &OutputJob,
    inputs: &Inputs,
    output: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Converted, ConvertError> {
    progress.set_step(Some(step(job, inputs)));
    let key = program_key(&tools.ffmpeg).await;
    let listed = if job.converts() {
        listed(tools, &key).await
    } else {
        None
    };
    let audio_encoder = match job.audio {
        output::AudioAction::Encode { codec, .. } => {
            output::audio_encoder(codec, listed.as_deref())
        }
        _ => ("copy", &[][..]),
    };
    let encoders: Vec<Option<&'static Encoder>> = match job.video {
        VideoAction::Encode { codec, .. } => {
            match video_encoders(tools, &key, codec, cancel).await {
                None => return Ok(Converted::Cancelled),
                Some(list) if list.is_empty() => {
                    return Err(ConvertError::Failed(format!(
                        "this ffmpeg has no {} encoder",
                        codec.name()
                    )));
                }
                Some(list) => list.into_iter().map(Some).collect(),
            }
        }
        _ => vec![None],
    };
    let mut last = None;
    for encoder in encoders {
        let args = output::arguments(job, inputs, encoder, audio_encoder, output);
        progress.set_per_mille(job.duration_ms.map(|_| 0));
        let result = run_ffmpeg(tools, &args, output, job.duration_ms, progress, cancel).await;
        match result {
            Ok(Converted::Done) => {
                if let Some(e) = encoder {
                    info!(encoder = e.name, "converted a video");
                }
                return Ok(Converted::Done);
            }
            Ok(Converted::Cancelled) => {
                remove(output).await;
                return Ok(Converted::Cancelled);
            }
            Err(ConvertError::Failed(reason)) => {
                remove(output).await;
                match encoder {
                    Some(e) if e.hardware => {
                        warn!(encoder = e.name, %reason, "a hardware encoder failed; trying the next one");
                        known().failed.insert((key.clone(), e.name));
                        last = Some(reason);
                    }
                    _ => return Err(ConvertError::Failed(reason)),
                }
            }
            Err(e) => {
                remove(output).await;
                return Err(e);
            }
        }
    }
    Err(ConvertError::Failed(
        last.unwrap_or_else(|| "no encoder worked".into()),
    ))
}

/// Runs one ffmpeg conversion into `output`, turning its `-progress` reports
/// into tenths of a percent of `duration_ms`.
async fn run_ffmpeg(
    tools: &MediaTools,
    args: &[OsString],
    output: &Path,
    duration_ms: Option<u64>,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Converted, ConvertError> {
    let dir = output.parent().unwrap_or(Path::new("."));
    let ran = process::run(
        &tools.ffmpeg,
        args,
        dir,
        false,
        None,
        cancel,
        |from, line| {
            if from == Output::Stdout
                && let Some(ms) = output::progress_ms(line)
            {
                progress.set_per_mille(output::per_mille(ms, duration_ms));
            }
        },
    )
    .await;
    match ran {
        Ok(ran) if ran.success && output.is_file() => Ok(Converted::Done),
        Ok(ran) => Err(ConvertError::Failed(
            ran.error_line()
                .unwrap_or("it gave no reason")
                .trim_end_matches('.')
                .to_string(),
        )),
        Err(RunError::Cancelled) => Ok(Converted::Cancelled),
        Err(RunError::TimedOut) => Err(ConvertError::Failed("it took too long".into())),
        Err(RunError::Start(e)) => Err(ConvertError::Start(e)),
    }
}

/// Turns subtitle file `input` (in `format`) into a `SubRip` file at `output`.
pub(crate) async fn to_srt(
    tools: &MediaTools,
    format: SubtitleFormat,
    input: &Path,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Converted, ConvertError> {
    let args = output::srt_arguments(format, input, output);
    let dir = output.parent().unwrap_or(Path::new("."));
    match process::run(&tools.ffmpeg, &args, dir, false, None, cancel, |_, _| {}).await {
        Ok(ran) if ran.success && output.is_file() => Ok(Converted::Done),
        Ok(ran) => {
            remove(output).await;
            Err(ConvertError::Failed(
                ran.error_line()
                    .unwrap_or("it gave no reason")
                    .trim_end_matches('.')
                    .to_string(),
            ))
        }
        Err(RunError::Cancelled) => {
            remove(output).await;
            Ok(Converted::Cancelled)
        }
        Err(RunError::TimedOut) => Err(ConvertError::Failed("it took too long".into())),
        Err(RunError::Start(e)) => Err(ConvertError::Start(e)),
    }
}

async fn remove(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(path = %path.display(), error = %e, "could not remove an unfinished file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::output::Extras;
    use crate::media::output::{AudioAction, Container, ImageFormat, SubtitlePick};
    use wirecatch_streams::InputFormat;

    /// A real ffmpeg from `WIRECATCH_TEST_FFMPEG`. Without one the test is
    /// skipped with a note, unless `WIRECATCH_REQUIRE_FFMPEG=1`.
    fn real_ffmpeg() -> Option<PathBuf> {
        let found = std::env::var_os("WIRECATCH_TEST_FFMPEG")
            .map(PathBuf::from)
            .filter(|p| p.is_file());
        if found.is_none() {
            let required = std::env::var("WIRECATCH_REQUIRE_FFMPEG").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "WIRECATCH_REQUIRE_FFMPEG=1 but WIRECATCH_TEST_FFMPEG is not a file"
            );
            eprintln!("skipped: set WIRECATCH_TEST_FFMPEG to a real ffmpeg to run this test");
        }
        found
    }

    fn make(ffmpeg: &Path, args: &[&str], path: &Path) {
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "could not make {}", path.display());
    }

    /// What ffmpeg says about `path` (its streams, chapters and tags).
    fn describe(ffmpeg: &Path, path: &Path) -> String {
        let out = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn job(container: Container, video: VideoAction, audio: AudioAction) -> OutputJob {
        OutputJob {
            container,
            video,
            audio,
            subtitles: Vec::new(),
            subtitle_files: false,
            extras: Extras::default(),
            duration_ms: Some(2_000),
            original_ext: "webm".into(),
            original: false,
        }
    }

    fn pick(language: &str, name: &str, auto: bool) -> SubtitlePick {
        SubtitlePick {
            key: language.into(),
            auto,
            language: language.into(),
            name: name.into(),
        }
    }

    /// A VP9 video and an Opus audio of two seconds, as WebM files, plus a
    /// `WebVTT` and a `SubRip` subtitle and a metadata file with two chapters.
    struct Fixture {
        dir: tempfile::TempDir,
        video: PathBuf,
        audio: PathBuf,
        vtt: PathBuf,
        srt: PathBuf,
        metadata: PathBuf,
    }

    fn fixture(ffmpeg: &Path) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("video.webm");
        make(
            ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=15:duration=2",
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-b:v",
                "200k",
                "-an",
            ],
            &video,
        );
        let audio = dir.path().join("audio.webm");
        make(
            ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-c:a",
                "libopus",
                "-vn",
            ],
            &audio,
        );
        let vtt = dir.path().join("sub-0.vtt");
        std::fs::write(
            &vtt,
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello\n\n00:00:01.000 --> 00:00:02.000\nWorld\n",
        )
        .unwrap();
        let srt = dir.path().join("sub-1.srt");
        std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,500\nBonjour\n").unwrap();
        let metadata = dir.path().join("metadata.txt");
        let chapters: Vec<crate::media::ytdlp::Chapter> =
            serde_json::from_value(serde_json::json!([
                {"start_time": 0, "end_time": 1, "title": "One"},
                {"start_time": 1, "end_time": 2, "title": "Two; and = more"},
            ]))
            .unwrap();
        std::fs::write(
            &metadata,
            output::ffmetadata(
                Some("Cats: the movie"),
                Some("Someone"),
                &chapters,
                Some(2_000),
            ),
        )
        .unwrap();
        Fixture {
            dir,
            video,
            audio,
            vtt,
            srt,
            metadata,
        }
    }

    fn tools(ffmpeg: &Path) -> MediaTools {
        MediaTools::for_test(ffmpeg)
    }

    fn inputs(f: &Fixture, subtitles: bool) -> Inputs {
        Inputs {
            video: Some((InputFormat::WebM, f.video.clone())),
            audio: Some((InputFormat::WebM, f.audio.clone())),
            subtitles: if subtitles {
                vec![
                    (
                        SubtitleFormat::WebVtt,
                        f.vtt.clone(),
                        pick("en", "English", false),
                    ),
                    (
                        SubtitleFormat::Srt,
                        f.srt.clone(),
                        pick("fr", "French", true),
                    ),
                ]
            } else {
                Vec::new()
            },
            metadata: Some(f.metadata.clone()),
            cover: None,
        }
    }

    /// MKV: the streams are copied, both subtitles go in as switchable `SubRip`
    /// tracks with their languages, and the chapters are there.
    #[tokio::test]
    async fn real_ffmpeg_copies_into_mkv_with_subtitles_and_chapters() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let mut j = job(Container::Mkv, VideoAction::Copy, AudioAction::Copy);
        j.subtitles = vec![pick("en", "English", false), pick("fr", "French", true)];
        j.extras.chapters = true;
        let out = f.dir.path().join("output.mkv");
        let progress = MediaProgress::default();
        let done = convert(
            &tools(&ffmpeg),
            &j,
            &inputs(&f, true),
            &out,
            &progress,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        let text = describe(&ffmpeg, &out);
        assert!(text.contains("Video: vp9"), "{text}");
        assert!(text.contains("Audio: opus"), "{text}");
        assert_eq!(text.matches("Subtitle: subrip").count(), 2, "{text}");
        assert!(text.contains("(eng)") && text.contains("(fre)"), "{text}");
        assert!(text.contains("French (auto-generated)"), "{text}");
        assert!(text.contains("Chapter #0:1"), "{text}");
        assert!(text.contains("Two; and = more"), "{text}");
        assert_eq!(
            progress.read().processing.map(|p| p.step),
            Some(ProcessingStep::Joining)
        );
    }

    /// MP4: the VP9 video is re-encoded to H.264 with the software encoder
    /// (`OpenH264` in the add-on's build), the Opus audio to AAC, the subtitles
    /// become `mov_text` and the chapters stay; the progress reaches the end.
    #[tokio::test]
    async fn real_ffmpeg_re_encodes_into_mp4_with_mov_text_and_chapters() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let mut j = job(
            Container::Mp4,
            VideoAction::Encode {
                codec: VideoCodec::H264,
                kbps: 300,
            },
            AudioAction::Encode {
                codec: output::AudioCodec::Aac,
                kbps: Some(128),
            },
        );
        j.subtitles = vec![pick("en", "English", false), pick("fr", "French", true)];
        j.extras.chapters = true;
        let out = f.dir.path().join("output.mp4");
        let tools = tools(&ffmpeg);
        let key = program_key(&tools.ffmpeg).await;
        let listed = listed(&tools, &key).await;
        let software = output::encoder_order(
            &output::encoders_for(VideoCodec::H264, std::env::consts::OS),
            listed.as_deref(),
            &HashSet::new(),
        )
        .into_iter()
        .find(|e| !e.hardware)
        .expect("a software H.264 encoder");
        let args = output::arguments(
            &j,
            &inputs(&f, true),
            Some(software),
            output::audio_encoder(output::AudioCodec::Aac, listed.as_deref()),
            &out,
        );
        let progress = MediaProgress::default();
        progress.set_step(Some(ProcessingStep::Converting));
        let done = run_ffmpeg(
            &tools,
            &args,
            &out,
            j.duration_ms,
            &progress,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        let text = describe(&ffmpeg, &out);
        assert!(text.contains("Video: h264"), "{text}");
        assert!(text.contains("Audio: aac"), "{text}");
        assert_eq!(text.matches("Subtitle: mov_text").count(), 2, "{text}");
        assert!(text.contains("(eng)") && text.contains("(fre)"), "{text}");
        assert!(text.contains("Chapter #0:1"), "{text}");
        let read = progress.read().processing.unwrap();
        assert_eq!(read.step, ProcessingStep::Converting);
        assert!(read.per_mille.is_some_and(|p| p >= 900), "{read:?}");
    }

    /// The whole conversion path with encoder choice: whatever encoder this
    /// computer has (a hardware one if its test passes), the result is H.264.
    #[tokio::test]
    async fn real_ffmpeg_converts_with_the_best_encoder_it_has() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let j = job(
            Container::Mp4,
            VideoAction::Encode {
                codec: VideoCodec::H264,
                kbps: 300,
            },
            AudioAction::Encode {
                codec: output::AudioCodec::Aac,
                kbps: Some(128),
            },
        );
        let out = f.dir.path().join("output.mp4");
        let progress = MediaProgress::default();
        let done = convert(
            &tools(&ffmpeg),
            &j,
            &inputs(&f, false),
            &out,
            &progress,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        assert!(describe(&ffmpeg, &out).contains("Video: h264"));
        assert_eq!(
            progress.read().processing.map(|p| p.step),
            Some(ProcessingStep::Converting)
        );
    }

    /// MP3 at 320 kbps with title, artist and a cover picture made from a WebP
    /// thumbnail.
    #[tokio::test]
    async fn real_ffmpeg_makes_an_mp3_with_cover_and_tags() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let cover = f.dir.path().join("cover.webp");
        make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x36:d=1",
                "-frames:v",
                "1",
                "-c:v",
                "libwebp",
            ],
            &cover,
        );
        let head = std::fs::read(&cover).unwrap();
        assert_eq!(ImageFormat::detect(&head), Some(ImageFormat::Webp));
        let mut j = job(
            Container::Mp3,
            VideoAction::None,
            AudioAction::Encode {
                codec: output::AudioCodec::Mp3,
                kbps: Some(320),
            },
        );
        j.extras.tags = true;
        let inputs = Inputs {
            video: None,
            audio: Some((InputFormat::WebM, f.audio.clone())),
            subtitles: Vec::new(),
            metadata: Some(f.metadata.clone()),
            cover: Some((ImageFormat::Webp, cover)),
        };
        let out = f.dir.path().join("output.mp3");
        let done = convert(
            &tools(&ffmpeg),
            &j,
            &inputs,
            &out,
            &MediaProgress::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        let text = describe(&ffmpeg, &out);
        assert!(text.contains("Audio: mp3"), "{text}");
        assert!(text.contains("320 kb/s"), "{text}");
        assert!(text.contains("Cats: the movie"), "{text}");
        assert!(text.contains("Someone"), "{text}");
        assert!(text.contains("Video: mjpeg"), "{text}");
        assert!(text.contains("(attached pic)"), "{text}");
        assert!(!text.contains("Chapter"), "{text}");
    }

    /// WebM keeps VP9 and Opus as they are and takes `WebVTT` subtitles.
    #[tokio::test]
    async fn real_ffmpeg_copies_into_webm_with_webvtt() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let mut j = job(Container::Webm, VideoAction::Copy, AudioAction::Copy);
        j.subtitles = vec![pick("en", "English", false), pick("fr", "French", true)];
        j.extras.chapters = true;
        let out = f.dir.path().join("output.webm");
        let done = convert(
            &tools(&ffmpeg),
            &j,
            &inputs(&f, true),
            &out,
            &MediaProgress::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        let text = describe(&ffmpeg, &out);
        assert_eq!(text.matches("Subtitle: webvtt").count(), 2, "{text}");
        assert!(text.contains("Chapter #0:1"), "{text}");
    }

    /// Subtitles saved next to an audio file become `SubRip`.
    #[tokio::test]
    async fn real_ffmpeg_turns_webvtt_into_srt() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        let out = f.dir.path().join("sub-0.srt");
        let done = to_srt(
            &tools(&ffmpeg),
            SubtitleFormat::WebVtt,
            &f.vtt,
            &out,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(done, Converted::Done);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.starts_with("1\r\n00:00:00,000 --> 00:00:01,000")
                || text.starts_with("1\n00:00:00,000 --> 00:00:01,000"),
            "{text}"
        );
        assert!(text.contains("World"));
    }

    /// A conversion ffmpeg cannot do fails with its reason, leaves no partial
    /// output and leaves the inputs alone.
    #[tokio::test]
    async fn real_ffmpeg_failing_keeps_the_inputs() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let f = fixture(&ffmpeg);
        // Opus copied into an MP3 file: ffmpeg refuses.
        let j = job(Container::Mp3, VideoAction::None, AudioAction::Copy);
        let inputs = Inputs {
            video: None,
            audio: Some((InputFormat::WebM, f.audio.clone())),
            subtitles: Vec::new(),
            metadata: None,
            cover: None,
        };
        let out = f.dir.path().join("output.mp3");
        let error = convert(
            &tools(&ffmpeg),
            &j,
            &inputs,
            &out,
            &MediaProgress::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let ConvertError::Failed(reason) = error else {
            panic!("not a failure: {error:?}");
        };
        assert_ne!(reason, "");
        assert!(!out.exists(), "a partial output was left");
        assert!(f.audio.is_file(), "the download is gone");
    }

    /// Cancelling stops ffmpeg at once, kills it and removes the partial file.
    #[tokio::test]
    async fn real_ffmpeg_is_killed_when_cancelled() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        // A long video, so the conversion is still running when cancelled.
        let video = dir.path().join("long.mkv");
        make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30:duration=60",
                "-c:v",
                "mpeg4",
            ],
            &video,
        );
        let j = OutputJob {
            duration_ms: Some(60_000),
            ..job(
                Container::Webm,
                VideoAction::Encode {
                    codec: VideoCodec::Vp9,
                    kbps: 2000,
                },
                AudioAction::None,
            )
        };
        let inputs = Inputs {
            video: Some((InputFormat::Matroska, video)),
            audio: None,
            subtitles: Vec::new(),
            metadata: None,
            cover: None,
        };
        let out = dir.path().join("output.webm");
        let encoder = output::VP9_ENCODERS
            .iter()
            .find(|e| e.name == "libvpx-vp9")
            .unwrap();
        let args = output::arguments(&j, &inputs, Some(encoder), ("copy", &[]), &out);
        let cancel = CancellationToken::new();
        let progress = MediaProgress::default();
        let tools = tools(&ffmpeg);
        let started = std::time::Instant::now();
        let stopper = {
            let cancel = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                cancel.cancel();
            })
        };
        let result = run_ffmpeg(&tools, &args, &out, j.duration_ms, &progress, &cancel).await;
        stopper.await.unwrap();
        assert!(matches!(result, Ok(Converted::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(15), "ffmpeg ran on");
        remove(&out).await;
        // The file can be removed (no ffmpeg holds it), and nothing writes it again.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!out.exists(), "ffmpeg kept writing after it was cancelled");
    }
}
