//! ffmpeg's arguments for a conversion: every input a named local file, the streams mapped, copied or re-encoded, subtitles, tags, chapters and a cover where the format holds them, and progress reports.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wirecatch_protocol::app::PER_MILLE_DONE;
use wirecatch_streams::InputFormat;
use wirecatch_streams::ffmpeg::{
    FileArgumentError, ffmpeg_input_args, file_argument, input_args_for, preamble, progress_args,
};

use super::Container;
use super::encoders::{AudioEncoder, Encoder, encode_filter};
use super::subtitles::{ImageFormat, MAX_NAME_CHARS, SubtitleFormat, iso639_2};
use super::{AudioAction, OutputJob, SubtitlePick, VideoAction};

/// A text for an ffmpeg option value (a track's name or language): no control characters, a sensible length.
fn plain(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

/// ffmpeg's demuxer for its own metadata file format (`;FFMETADATA1`).
const FFMETADATA: &str = "ffmetadata";

/// The files a conversion reads, all in the download's work folder.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// The video (or the one file holding video and audio) and its format.
    pub video: Option<(InputFormat, PathBuf)>,
    /// Separate audio, if any.
    pub audio: Option<(InputFormat, PathBuf)>,
    /// Subtitles to put inside the file, each with its format and pick.
    pub subtitles: Vec<(SubtitleFormat, PathBuf, SubtitlePick)>,
    /// The metadata file with tags and chapters.
    pub metadata: Option<PathBuf>,
    /// The cover picture.
    pub cover: Option<(ImageFormat, PathBuf)>,
}

/// ffmpeg's arguments to save `inputs` into `output` as `job` says: streams that fit are copied, the rest re-encoded (video with `encoder`, audio with `audio_encoder`), with subtitles, chapters, tags and a cover where the format holds them, and progress reports on standard output.
pub(crate) fn arguments(
    job: &OutputJob,
    inputs: &Inputs,
    video: VideoPlan<'_>,
    audio_encoder: AudioEncoder,
    output: &Path,
) -> Result<Vec<OsString>, FileArgumentError> {
    let mut args = preamble();
    if let VideoPlan::Encode { encoder, .. } = video {
        push(&mut args, encoder.global);
    }
    let numbers = add_inputs(&mut args, job, inputs)?;
    add_maps(&mut args, job, &numbers);
    add_video_codec(&mut args, video);
    add_audio_codec(&mut args, job, audio_encoder);
    if let Some(codec) = job
        .container
        .subtitle_codec()
        .filter(|_| job.embedded_subtitles())
    {
        add_subtitle_tracks(&mut args, codec, &numbers.subtitles);
    }
    if numbers.cover.is_some() {
        add_cover_disposition(&mut args, job);
    }
    // Tags and chapters come from the metadata file, or not at all.
    match numbers.metadata {
        Some(m) => {
            args.push("-map_metadata".into());
            args.push(m.to_string().into());
            args.push("-map_chapters".into());
            let chapters = job.extras.chapters && job.container.chapters();
            args.push(if chapters { m.to_string() } else { "-1".into() }.into());
        }
        None => push(&mut args, &["-map_chapters", "-1"]),
    }
    if job.container.faststart() {
        push(&mut args, &["-movflags", "+faststart"]);
    }
    if job.container == Container::Mp3 {
        push(&mut args, &["-id3v2_version", "3"]);
    }
    args.extend(progress_args());
    push(&mut args, &["-f", job.container.muxer()]);
    args.push(file_argument(output)?);
    Ok(args)
}

fn push(args: &mut Vec<OsString>, words: &[&str]) {
    args.extend(words.iter().map(OsString::from));
}

/// The number ffmpeg gives each input that was added, counting from 0 in the order they were added.
struct InputNumbers<'a> {
    video: Option<usize>,
    audio: Option<usize>,
    /// Each embedded subtitle's input, with the subtitle it is.
    subtitles: Vec<(usize, &'a SubtitlePick)>,
    metadata: Option<usize>,
    cover: Option<usize>,
}

/// Adds the inputs `job` uses to `args`, in a fixed order, and returns their numbers for the mapping.
fn add_inputs<'a>(
    args: &mut Vec<OsString>,
    job: &OutputJob,
    inputs: &'a Inputs,
) -> Result<InputNumbers<'a>, FileArgumentError> {
    let mut next = 0usize;
    let mut number = |args: &mut Vec<OsString>, input: Result<Vec<OsString>, FileArgumentError>| {
        args.extend(input?);
        next += 1;
        Ok::<_, FileArgumentError>(next - 1)
    };
    let video = inputs
        .video
        .as_ref()
        .map(|(f, p)| number(args, ffmpeg_input_args(*f, p)))
        .transpose()?;
    let audio = inputs
        .audio
        .as_ref()
        .map(|(f, p)| number(args, ffmpeg_input_args(*f, p)))
        .transpose()?;
    let subtitles = if job.embedded_subtitles() {
        inputs
            .subtitles
            .iter()
            .map(|(f, p, pick)| Ok((number(args, input_args_for(f.demuxer(), p))?, pick)))
            .collect::<Result<_, FileArgumentError>>()?
    } else {
        Vec::new()
    };
    let metadata = inputs
        .metadata
        .as_ref()
        .filter(|_| job.wants_metadata())
        .map(|p| number(args, input_args_for(FFMETADATA, p)))
        .transpose()?;
    let cover = inputs
        .cover
        .as_ref()
        .filter(|_| job.wants_cover())
        .map(|(f, p)| number(args, input_args_for(f.demuxer(), p)))
        .transpose()?;
    Ok(InputNumbers {
        video,
        audio,
        subtitles,
        metadata,
        cover,
    })
}

/// Picks the streams that go into the file: the video, the audio (from the video's input when it came joined), each subtitle and the cover.
fn add_maps(args: &mut Vec<OsString>, job: &OutputJob, numbers: &InputNumbers<'_>) {
    let mut map = |spec: String| {
        args.push("-map".into());
        args.push(spec.into());
    };
    if job.video != VideoAction::None
        && let Some(v) = numbers.video
    {
        map(format!("{v}:v:0"));
    }
    if job.audio != AudioAction::None {
        match (numbers.audio, numbers.video) {
            (Some(a), _) => map(format!("{a}:a:0")),
            (None, Some(v)) => map(format!("{v}:a:0")),
            (None, None) => {}
        }
    }
    for (i, _) in &numbers.subtitles {
        map(format!("{i}:s:0"));
    }
    if let Some(c) = numbers.cover {
        map(format!("{c}:v:0"));
    }
}

/// How the video is written: not at all, re-encoded with `encoder` at the job's rate, or copied.
fn add_video_codec(args: &mut Vec<OsString>, video: VideoPlan<'_>) {
    match video {
        VideoPlan::None => {}
        VideoPlan::Encode { encoder: e, kbps } => {
            push(args, &["-filter:v:0", &encode_filter(e), "-c:v:0", e.name]);
            push(args, e.options);
            let (rate, max, buf) = (
                kbps,
                kbps.saturating_mul(MAXRATE_FACTOR.0) / MAXRATE_FACTOR.1,
                kbps.saturating_mul(BUFSIZE_FACTOR),
            );
            for (flag, value) in [
                ("-b:v:0", rate),
                ("-maxrate:v:0", max),
                ("-bufsize:v:0", buf),
            ] {
                args.push(flag.into());
                args.push(format!("{value}k").into());
            }
        }
        VideoPlan::Copy => push(args, &["-c:v:0", "copy"]),
    }
}

/// The highest video bit rate a re-encode may reach for a moment, as a fraction of its average (D24): one and a half.
const MAXRATE_FACTOR: (u32, u32) = (3, 2);
/// The rate control buffer of a re-encode, in seconds of its average bit rate.
const BUFSIZE_FACTOR: u32 = 2;

/// What happens to the video of a conversion, with its encoder: a re-encode cannot be planned without one.
#[derive(Debug, Clone, Copy)]
pub(crate) enum VideoPlan<'a> {
    /// There is no video.
    None,
    /// Copied as it is.
    Copy,
    /// Re-encoded with `encoder` at about `kbps`.
    Encode { encoder: &'a Encoder, kbps: u32 },
}

impl<'a> VideoPlan<'a> {
    /// The plan for `action`; `None` when it re-encodes and there is no `encoder`.
    pub fn new(action: VideoAction, encoder: Option<&'a Encoder>) -> Option<Self> {
        match (action, encoder) {
            (VideoAction::None, _) => Some(VideoPlan::None),
            (VideoAction::Copy, _) => Some(VideoPlan::Copy),
            (VideoAction::Encode { kbps, .. }, Some(encoder)) => {
                Some(VideoPlan::Encode { encoder, kbps })
            }
            (VideoAction::Encode { .. }, None) => None,
        }
    }
}

/// How the audio is written: not at all, copied, or re-encoded with `encoder` (its name and options) at the job's rate, if it has one.
fn add_audio_codec(args: &mut Vec<OsString>, job: &OutputJob, encoder: AudioEncoder) {
    match job.audio {
        AudioAction::None => {}
        AudioAction::Copy => push(args, &["-c:a", "copy"]),
        AudioAction::Encode { kbps, .. } => {
            push(args, &["-c:a", encoder.name]);
            push(args, encoder.options);
            if let Some(kbps) = kbps {
                args.push("-b:a".into());
                args.push(format!("{kbps}k").into());
            }
        }
    }
}

/// Writes the embedded subtitles with `codec`, each named and labelled with its language.
fn add_subtitle_tracks(
    args: &mut Vec<OsString>,
    codec: &str,
    subtitles: &[(usize, &SubtitlePick)],
) {
    push(args, &["-c:s", codec]);
    for (n, (_, pick)) in subtitles.iter().enumerate() {
        let name = if pick.auto {
            format!("{} (auto-generated)", plain(&pick.name, MAX_NAME_CHARS))
        } else {
            plain(&pick.name, MAX_NAME_CHARS)
        };
        args.push(format!("-metadata:s:s:{n}").into());
        args.push(format!("language={}", iso639_2(&pick.language)).into());
        args.push(format!("-metadata:s:s:{n}").into());
        args.push(format!("title={name}").into());
    }
}

/// Writes the cover picture as an attached JPEG picture after the video, if there is one.
fn add_cover_disposition(args: &mut Vec<OsString>, job: &OutputJob) {
    let n = usize::from(job.video != VideoAction::None);
    args.push(format!("-c:v:{n}").into());
    args.push("mjpeg".into());
    args.push(format!("-disposition:v:{n}").into());
    args.push("attached_pic".into());
    args.push(format!("-metadata:s:v:{n}").into());
    args.push("comment=Cover (front)".into());
}

/// ffmpeg's arguments to turn subtitle file `input` into `SubRip` at `output`.
pub(crate) fn srt_arguments(
    format: SubtitleFormat,
    input: &Path,
    output: &Path,
) -> Result<Vec<OsString>, FileArgumentError> {
    let mut args = preamble();
    args.extend(input_args_for(format.demuxer(), input)?);
    args.extend(progress_args());
    push(&mut args, &["-map", "0:s:0", "-c:s", "srt", "-f", "srt"]);
    args.push(file_argument(output)?);
    Ok(args)
}

/// How far a conversion is, in milliseconds of media, from one line of ffmpeg's `-progress` report (`out_time_us=1234567`).
pub(crate) fn progress_ms(line: &str) -> Option<u64> {
    let (key, value) = line.trim().split_once('=')?;
    match key {
        // Both are microseconds (`out_time_ms` is misnamed).
        "out_time_us" | "out_time_ms" => value.parse::<u64>().ok().map(|us| us / 1000),
        _ => None,
    }
}

/// Tenths of a percent done after `done_ms` of `total_ms`.
pub(crate) fn per_mille(done_ms: u64, total_ms: Option<u64>) -> Option<u16> {
    let total = total_ms.filter(|t| *t > 0)?;
    let per_mille = done_ms.saturating_mul(u64::from(PER_MILLE_DONE)) / total;
    // Past the end (a duration reported short) counts as done.
    Some(u16::try_from(per_mille).map_or(PER_MILLE_DONE, |p| p.min(PER_MILLE_DONE)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::test_support::*;
    use crate::output::*;

    #[test]
    fn progress_is_read_from_ffmpeg_reports() {
        assert_eq!(progress_ms("out_time_us=1500000"), Some(1500));
        assert_eq!(progress_ms("out_time_ms=2000000"), Some(2000));
        assert_eq!(progress_ms("out_time_us=N/A"), None);
        assert_eq!(progress_ms("progress=continue"), None);
        assert_eq!(per_mille(500, Some(1000)), Some(500));
        assert_eq!(per_mille(5000, Some(1000)), Some(1000));
        assert_eq!(per_mille(5, None), None);
    }

    /// Every input, subtitles and the metadata file included, is read as its own format from a local file only, and the output is one `file:` path.
    #[test]
    fn every_input_of_a_conversion_is_a_named_local_file() {
        let mut j = job(Container::Mkv, VideoAction::Copy, AudioAction::Copy);
        j.subtitles = vec![pick("en", false), pick("en", true)];
        j.extras.chapters = true;
        let inputs = Inputs {
            video: Some((InputFormat::WebM, PathBuf::from("/w/-video.webm"))),
            audio: Some((InputFormat::WebM, PathBuf::from("/w/audio.webm"))),
            subtitles: vec![
                (
                    SubtitleFormat::WebVtt,
                    PathBuf::from("/w/sub-0.vtt"),
                    pick("en", false),
                ),
                (
                    SubtitleFormat::Srt,
                    PathBuf::from("/w/sub-1.srt"),
                    pick("en", true),
                ),
            ],
            metadata: Some(PathBuf::from("/w/metadata.txt")),
            cover: Some((ImageFormat::Webp, PathBuf::from("/w/cover.webp"))),
        };
        let args = text(
            &arguments(
                &j,
                &inputs,
                VideoPlan::new(j.video, None).unwrap(),
                audio_encoder(AudioCodec::Aac, None),
                Path::new("/w/output.mkv"),
            )
            .unwrap(),
        );
        let inputs_seen: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-i")
            .map(|(i, _)| i)
            .collect();
        // Video, audio, two subtitles and the metadata; no cover for a video.
        assert_eq!(inputs_seen.len(), 5, "{args:?}");
        for i in inputs_seen {
            assert_eq!(args[i - 6], "-protocol_whitelist");
            assert_eq!(args[i - 5], "file");
            assert_eq!(args[i - 4], "-format_whitelist");
            assert_eq!(args[i - 2], "-f");
            assert_eq!(args[i - 3], args[i - 1]);
            assert!(args[i + 1].starts_with("file:"), "{}", args[i + 1]);
        }
        let joined = args.join(" ");
        assert!(
            joined.contains("-map 0:v:0 -map 1:a:0 -map 2:s:0 -map 3:s:0"),
            "{joined}"
        );
        assert!(joined.contains("-c:v:0 copy"));
        assert!(joined.contains("-c:a copy"));
        assert!(joined.contains("-c:s srt"));
        assert!(joined.contains("-metadata:s:s:0 language=eng -metadata:s:s:0 title=English"));
        assert!(joined.contains("-metadata:s:s:1 title=English (auto-generated)"));
        assert!(joined.contains("-map_metadata 4 -map_chapters 4"));
        assert!(joined.contains("-progress pipe:1"));
        assert!(joined.contains("-f matroska file:"), "{joined}");
        assert!(args.last().unwrap().starts_with("file:"));
        assert!(!joined.contains("cover.webp"));
    }

    #[test]
    fn a_conversion_names_its_encoders_and_rates() {
        let encoder = encoders_for(VideoCodec::H264, "windows")
            .into_iter()
            .find(|e| e.name == "libopenh264")
            .unwrap();
        let j = job(
            Container::Mp4,
            VideoAction::Encode {
                codec: VideoCodec::H264,
                kbps: 4000,
            },
            AudioAction::Encode {
                codec: AudioCodec::Aac,
                kbps: Some(192),
            },
        );
        let inputs = Inputs {
            video: Some((InputFormat::WebM, PathBuf::from("/w/video.webm"))),
            audio: Some((InputFormat::WebM, PathBuf::from("/w/audio.webm"))),
            subtitles: Vec::new(),
            metadata: None,
            cover: None,
        };
        let joined = text(
            &arguments(
                &j,
                &inputs,
                VideoPlan::new(j.video, Some(encoder)).unwrap(),
                audio_encoder(AudioCodec::Aac, None),
                Path::new("/w/out.mp4"),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(
            joined.contains(
                "-filter:v:0 scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p -c:v:0 libopenh264"
            ),
            "{joined}"
        );
        assert!(joined.contains("-b:v:0 4000k -maxrate:v:0 6000k -bufsize:v:0 8000k"));
        assert!(joined.contains("-c:a aac -b:a 192k"));
        assert!(joined.contains("-map_chapters -1"));
        assert!(joined.contains("-movflags +faststart"));
        assert!(joined.contains("-f mp4 file:"));
    }

    #[test]
    fn an_mp3_gets_its_cover_and_tags() {
        let mut j = job(
            Container::Mp3,
            VideoAction::None,
            AudioAction::Encode {
                codec: AudioCodec::Mp3,
                kbps: Some(320),
            },
        );
        j.extras.tags = true;
        j.subtitles = vec![pick("en", false)];
        assert!(j.sidecar_subtitles() && !j.embedded_subtitles());
        let inputs = Inputs {
            video: None,
            audio: Some((InputFormat::WebM, PathBuf::from("/w/audio.webm"))),
            subtitles: vec![(
                SubtitleFormat::WebVtt,
                PathBuf::from("/w/sub-0.vtt"),
                pick("en", false),
            )],
            metadata: Some(PathBuf::from("/w/metadata.txt")),
            cover: Some((ImageFormat::Webp, PathBuf::from("/w/cover.webp"))),
        };
        let joined = text(
            &arguments(
                &j,
                &inputs,
                VideoPlan::new(j.video, None).unwrap(),
                audio_encoder(AudioCodec::Mp3, None),
                Path::new("/w/out.mp3"),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(joined.contains("-map 0:a:0 -map 2:v:0"), "{joined}");
        assert!(joined.contains("-f webp_pipe -i file:"));
        assert!(joined.contains("-c:a libmp3lame -b:a 320k"));
        assert!(joined.contains("-c:v:0 mjpeg -disposition:v:0 attached_pic"));
        assert!(joined.contains("-map_metadata 1 -map_chapters -1"));
        assert!(joined.contains("-id3v2_version 3"));
        assert!(!joined.contains("sub-0.vtt"), "subtitles go next to an MP3");
    }

    #[test]
    fn subtitles_become_srt_files_with_their_format_named() {
        let joined = text(
            &srt_arguments(
                SubtitleFormat::WebVtt,
                Path::new("/w/sub.vtt"),
                Path::new("/w/sub.srt"),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(
            joined.contains("-protocol_whitelist file -format_whitelist webvtt -f webvtt -i file:"),
            "{joined}"
        );
        assert!(joined.contains("-c:s srt -f srt file:"));
    }

    #[test]
    fn a_re_encode_cannot_be_planned_without_an_encoder() {
        let encode = VideoAction::Encode {
            codec: VideoCodec::H264,
            kbps: 1000,
        };
        assert!(VideoPlan::new(encode, None).is_none());
        assert!(matches!(
            VideoPlan::new(VideoAction::Copy, None),
            Some(VideoPlan::Copy)
        ));
    }
}
