//! Running the media add-on's ffmpeg for output formats (D24): finding a video encoder that works on this computer, converting with progress, and turning subtitles into `SubRip` files.
//!
//! Which encoders an ffmpeg has, and which hardware encoder works, is found once per [`EncoderCatalog`], which lives with the add-on's programs ([`MediaTools`]), so looking at the add-on again (after an update, or a repair) asks again. A hardware encoder that fails on a real video is skipped from then on, but only once the same video converted with a software encoder: a failure the software encoder shares is the input's, not the encoder's.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::OnceCell;
use tracing::{info, warn};
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::ProcessingStep;
use wirecatch_streams::ffmpeg::preamble;
use wirecatch_streams::process::{self, RunError, RunOptions};

use super::MediaProgress;
use super::ffmpeg::{self, FfmpegError, Finished};
use super::output::{
    self, AudioAction, Encoder, Inputs, OutputJob, SubtitleFormat, VideoAction, VideoCodec,
};
use crate::addons::MediaTools;

/// How long asking ffmpeg for its encoders, or one encoder test, may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// What is known about the encoders of one ffmpeg.
#[derive(Debug, Default)]
pub(crate) struct EncoderCatalog {
    /// The encoders ffmpeg lists, once it answered.
    listed: OnceCell<Arc<HashSet<String>>>,
    /// The video encoders to use for each codec, in order.
    video: Mutex<HashMap<VideoCodec, Arc<OnceCell<Vec<&'static Encoder>>>>>,
    /// Hardware encoders that failed on a real video that a software encoder then converted.
    failed: Mutex<HashSet<&'static str>>,
}

/// Why ffmpeg could not be asked for its encoders.
#[derive(Debug, thiserror::Error)]
enum ListError {
    #[error(transparent)]
    Folder(FfmpegError),
    #[error("ffmpeg did not run: {0}")]
    Run(RunError),
    #[error("it listed no encoders")]
    NoneListed,
}

impl EncoderCatalog {
    /// The encoders ffmpeg lists. A failed or cancelled answer is not kept, so the next conversion asks again (a first run of a freshly installed ffmpeg may be slow while an antivirus scans it); `None` then.
    async fn listed(
        &self,
        tools: &MediaTools,
        cancel: &CancellationToken,
    ) -> Option<Arc<HashSet<String>>> {
        let listed = self
            .listed
            .get_or_try_init(|| async {
                let mut args = preamble();
                args.push("-encoders".into());
                let dir = ffmpeg::ffmpeg_dir(&tools.ffmpeg).map_err(ListError::Folder)?;
                let options = RunOptions::collect().with_timeout(PROBE_TIMEOUT);
                let ran = process::run(&tools.ffmpeg, &args, dir, options, cancel, |_, _| {})
                    .await
                    .map_err(ListError::Run)?;
                let found = output::listed_encoders(&String::from_utf8_lossy(&ran.stdout));
                if ran.success && !found.is_empty() {
                    Ok(Arc::new(found))
                } else {
                    Err(ListError::NoneListed)
                }
            })
            .await;
        match listed {
            Ok(listed) => Some(listed.clone()),
            Err(error) => {
                warn!(%error, "could not ask ffmpeg for its encoders; using the add-on's own");
                None
            }
        }
    }

    /// The video encoders for `codec`, in the order to use them: the first hardware encoder that encodes a test clip on this computer, then the software ones, without those known to fail. `None` when cancelled.
    async fn video_encoders(
        &self,
        tools: &MediaTools,
        codec: VideoCodec,
        cancel: &CancellationToken,
    ) -> Option<Vec<&'static Encoder>> {
        let Some(listed) = self.listed(tools, cancel).await else {
            // Not asked: only the add-on's own software encoder, without remembering it.
            return Some(output::encoder_order(
                &output::encoders_for(codec, std::env::consts::OS),
                None,
            ));
        };
        let cell = lock(&self.video).entry(codec).or_default().clone();
        let found = cell
            .get_or_try_init(|| async {
                let candidates = output::encoders_for(codec, std::env::consts::OS);
                let order = output::encoder_order(&candidates, Some(&listed));
                let mut chosen = Vec::new();
                for encoder in order.iter().filter(|e| e.hardware) {
                    match probe(tools, encoder, cancel).await {
                        Probe::Cancelled => return Err(()),
                        Probe::Works => {
                            info!(
                                encoder = encoder.name,
                                "a hardware video encoder works here"
                            );
                            chosen.push(*encoder);
                            break;
                        }
                        Probe::Fails => {}
                    }
                }
                chosen.extend(order.iter().filter(|e| !e.hardware));
                Ok(chosen)
            })
            .await
            .ok()?;
        let failed = lock(&self.failed);
        Some(
            found
                .iter()
                .filter(|e| !failed.contains(e.name))
                .copied()
                .collect(),
        )
    }

    /// Skips `encoders` from now on.
    fn mark_failed(&self, encoders: &[&'static Encoder]) {
        let mut failed = lock(&self.failed);
        for encoder in encoders {
            warn!(
                encoder = encoder.name,
                "a hardware encoder failed on a video a software encoder converted; it is skipped from now on"
            );
            failed.insert(encoder.name);
        }
    }
}

/// Locks part of the catalog. A lock poisoned by a panic in another holder is taken over as it is: each entry is replaced whole, so it is never half written.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What encoding a test clip with an encoder showed.
enum Probe {
    Works,
    Fails,
    Cancelled,
}

/// Whether `encoder` encodes a short generated clip.
async fn probe(tools: &MediaTools, encoder: &Encoder, cancel: &CancellationToken) -> Probe {
    let Ok(dir) = ffmpeg::ffmpeg_dir(&tools.ffmpeg) else {
        return Probe::Fails;
    };
    let options = RunOptions::lines().with_timeout(PROBE_TIMEOUT);
    match process::run(
        &tools.ffmpeg,
        &output::probe_arguments(encoder),
        dir,
        options,
        cancel,
        |_, _| {},
    )
    .await
    {
        Ok(ran) if ran.success => Probe::Works,
        Err(RunError::Cancelled) => Probe::Cancelled,
        Ok(_) | Err(_) => Probe::Fails,
    }
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

/// Saves `inputs` into `output` as `job` says, reporting into `progress`. A video to re-encode goes through the encoders that work here in order (see [`try_encoders`]). Whatever ends, a partial `output` is removed.
pub(crate) async fn convert(
    tools: &MediaTools,
    job: &OutputJob,
    inputs: &Inputs,
    output: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Finished, FfmpegError> {
    progress.set_step(Some(step(job, inputs)));
    let catalog = tools.encoders();
    let listed = if job.converts() {
        catalog.listed(tools, cancel).await
    } else {
        None
    };
    let audio_encoder = match job.audio {
        AudioAction::Encode { codec, .. } => output::audio_encoder(codec, listed.as_deref()),
        _ => output::AudioEncoder::COPY,
    };
    let encoders: Vec<Option<&'static Encoder>> = match job.video {
        VideoAction::Encode { codec, .. } => {
            match catalog.video_encoders(tools, codec, cancel).await {
                None => return Ok(Finished::Cancelled),
                Some(list) if list.is_empty() => return Err(FfmpegError::NoEncoder(codec)),
                Some(list) => list.into_iter().map(Some).collect(),
            }
        }
        _ => vec![None],
    };
    let target = match job.video {
        VideoAction::Encode { codec, .. } => codec,
        _ => VideoCodec::Other,
    };
    let (result, failed) = try_encoders(&encoders, |encoder| {
        let args = output::VideoPlan::new(job.video, encoder)
            .map(|video| output::arguments(job, inputs, video, audio_encoder, output));
        progress.set_per_mille(job.duration_ms.map(|_| 0));
        async move {
            let Some(args) = args else {
                return Err(FfmpegError::NoEncoder(target));
            };
            let args = args?;
            ffmpeg::run(tools, &args, output, None, cancel, |line| {
                if let Some(ms) = output::progress_ms(line) {
                    progress.set_per_mille(output::per_mille(ms, job.duration_ms));
                }
            })
            .await
        }
    })
    .await;
    catalog.mark_failed(&failed);
    result
}

/// Runs `attempt` with each of `encoders` in order until one converts. A hardware encoder that fails gives way to the next one; once a software encoder converts, the hardware encoders that failed before it are returned, to be skipped from then on. When a software encoder fails as well, the failure is the input's (a damaged subtitle, a full disk), so nothing is blamed on the hardware encoders and the error is returned.
async fn try_encoders<F, Fut>(
    encoders: &[Option<&'static Encoder>],
    mut attempt: F,
) -> (Result<Finished, FfmpegError>, Vec<&'static Encoder>)
where
    F: FnMut(Option<&'static Encoder>) -> Fut,
    Fut: Future<Output = Result<Finished, FfmpegError>>,
{
    let mut failed_hardware = Vec::new();
    let mut last = None;
    for &encoder in encoders {
        match attempt(encoder).await {
            Ok(Finished::Done) => {
                if let Some(e) = encoder {
                    info!(encoder = e.name, "converted a video");
                }
                let blamed = if encoder.is_some_and(|e| !e.hardware) {
                    failed_hardware
                } else {
                    Vec::new()
                };
                return (Ok(Finished::Done), blamed);
            }
            Ok(Finished::Cancelled) => return (Ok(Finished::Cancelled), Vec::new()),
            // A hardware encoder that fails, hangs or runs too long is passed over for the next.
            Err(
                error @ (FfmpegError::Failed { .. }
                | FfmpegError::Stalled(_)
                | FfmpegError::TimedOut(_)),
            ) if encoder.is_some_and(|e| e.hardware) => {
                if let Some(e) = encoder {
                    warn!(encoder = e.name, %error, "a hardware encoder failed; trying the next one");
                    failed_hardware.push(e);
                }
                last = Some(error);
            }
            Err(e) => return (Err(e), Vec::new()),
        }
    }
    let error = last.unwrap_or(FfmpegError::Failed {
        reason: "no encoder worked".into(),
    });
    (Err(error), Vec::new())
}

/// Turns subtitle file `input` (in `format`) into a `SubRip` file at `output`.
pub(crate) async fn to_srt(
    tools: &MediaTools,
    format: SubtitleFormat,
    input: &Path,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Finished, FfmpegError> {
    let args = output::srt_arguments(format, input, output)?;
    ffmpeg::run(tools, &args, output, None, cancel, |_| {}).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::media::ffmpeg::test_support::{make, real_ffmpeg};
    use crate::media::output::Extras;
    use crate::media::output::{AudioAction, Container, ImageFormat, SubtitlePick};
    use wirecatch_streams::InputFormat;

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

    fn failing(reason: &str) -> Result<Finished, FfmpegError> {
        Err(FfmpegError::Failed {
            reason: reason.into(),
        })
    }

    fn named(name: &str) -> Option<&'static Encoder> {
        output::H264_ENCODERS.iter().find(|e| e.name == name)
    }

    /// A damaged input fails with every encoder: the hardware encoder tried first is not blamed for it, and the software encoder's failure is the one reported.
    #[tokio::test]
    async fn an_input_failure_is_not_blamed_on_the_hardware_encoder() {
        let encoders = [named("h264_nvenc"), named("libopenh264")];
        let mut tried = Vec::new();
        let (result, blamed) = try_encoders(&encoders, |encoder| {
            tried.push(encoder.map(|e| e.name));
            async move {
                match encoder {
                    Some(e) if e.hardware => failing("hardware said no"),
                    _ => failing("subtitle.vtt: Invalid data found"),
                }
            }
        })
        .await;
        assert_eq!(blamed, Vec::<&Encoder>::new());
        assert!(
            matches!(result, Err(FfmpegError::Failed { reason }) if reason.contains("subtitle"))
        );
        assert_eq!(tried, [Some("h264_nvenc"), Some("libopenh264")]);
    }

    /// A hardware encoder that fails on a video the software encoder then converts is skipped from then on.
    #[tokio::test]
    async fn a_hardware_encoder_is_blamed_only_when_software_converts() {
        let encoders = [named("h264_nvenc"), named("h264_qsv"), named("libopenh264")];
        let (result, blamed) = try_encoders(&encoders, |encoder| async move {
            match encoder {
                Some(e) if e.hardware => failing("too large for it"),
                _ => Ok(Finished::Done),
            }
        })
        .await;
        assert!(matches!(result, Ok(Finished::Done)));
        let names: Vec<&str> = blamed.iter().map(|e| e.name).collect();
        assert_eq!(names, ["h264_nvenc", "h264_qsv"]);
        // Starting fails before any encoder is judged.
        let (result, blamed) = try_encoders(&encoders, |_| async {
            Err(FfmpegError::Start(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        })
        .await;
        assert!(matches!(result, Err(FfmpegError::Start(_))));
        assert_eq!(blamed, Vec::<&Encoder>::new());
    }

    /// A hardware encoder that hangs (a driver stuck on a frame) or runs past the limit is passed over like one that fails.
    #[tokio::test]
    async fn a_hardware_encoder_that_stalls_or_times_out_is_passed_over() {
        let encoders = [named("h264_nvenc"), named("h264_qsv"), named("libopenh264")];
        let limit = std::time::Duration::from_secs(300);
        let (result, blamed) = try_encoders(&encoders, |encoder| async move {
            match encoder.map(|e| e.name) {
                Some("h264_nvenc") => Err(FfmpegError::Stalled(limit)),
                Some("h264_qsv") => Err(FfmpegError::TimedOut(limit)),
                _ => Ok(Finished::Done),
            }
        })
        .await;
        assert!(matches!(result, Ok(Finished::Done)), "{result:?}");
        let names: Vec<&str> = blamed.iter().map(|e| e.name).collect();
        assert_eq!(names, ["h264_nvenc", "h264_qsv"]);
        // The software encoder stalling is reported as it is.
        let (result, _) = try_encoders(&[named("libopenh264")], |_| async move {
            Err(FfmpegError::Stalled(limit))
        })
        .await;
        assert!(matches!(result, Err(FfmpegError::Stalled(_))), "{result:?}");
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
        assert_eq!(done, Finished::Done);
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
        let cancel = CancellationToken::new();
        let listed = tools.encoders().listed(&tools, &cancel).await;
        let software = output::encoder_order(
            &output::encoders_for(VideoCodec::H264, std::env::consts::OS),
            listed.as_deref(),
        )
        .into_iter()
        .find(|e| !e.hardware)
        .expect("a software H.264 encoder");
        let args = output::arguments(
            &j,
            &inputs(&f, true),
            output::VideoPlan::new(j.video, Some(software)).unwrap(),
            output::audio_encoder(output::AudioCodec::Aac, listed.as_deref()),
            &out,
        )
        .unwrap();
        let progress = MediaProgress::default();
        progress.set_step(Some(ProcessingStep::Converting));
        let done = ffmpeg::run(&tools, &args, &out, None, &cancel, |line| {
            if let Some(ms) = output::progress_ms(line) {
                progress.set_per_mille(output::per_mille(ms, j.duration_ms));
            }
        })
        .await
        .unwrap();
        assert_eq!(done, Finished::Done);
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
        assert_eq!(done, Finished::Done);
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
        assert_eq!(done, Finished::Done);
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
        assert_eq!(done, Finished::Done);
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
        assert_eq!(done, Finished::Done);
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
        let FfmpegError::Failed { reason } = error else {
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
        let args = output::arguments(
            &j,
            &inputs,
            output::VideoPlan::new(j.video, Some(encoder)).unwrap(),
            output::AudioEncoder::COPY,
            &out,
        )
        .unwrap();
        let cancel = CancellationToken::new();
        let tools = tools(&ffmpeg);
        let started = std::time::Instant::now();
        let stopper = {
            let cancel = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                cancel.cancel();
            })
        };
        let result = ffmpeg::run(&tools, &args, &out, None, &cancel, |_| {}).await;
        stopper.await.unwrap();
        assert!(matches!(result, Ok(Finished::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(15), "ffmpeg ran on");
        ffmpeg::remove(&out).await;
        // The file can be removed (no ffmpeg holds it), and nothing writes it again.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!out.exists(), "ffmpeg kept writing after it was cancelled");
    }
}
