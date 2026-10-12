//! Output formats for media read by yt-dlp (D24): which formats a quality can be
//! saved as, whether that copies the streams or re-encodes them, the encoders to
//! re-encode with, the subtitles and chapters a video has, and ffmpeg's
//! arguments for all of it. Nothing here runs a program; see
//! [`convert`](super::convert) for that.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use wirecatch_engine::numeric::f64_to_u64;
use wirecatch_protocol::app::{
    MediaSubtitle, OutputFit, OutputFormat, PER_MILLE_DONE, StreamAction,
};
use wirecatch_streams::{InputFormat, ffmpeg_input_args};

use super::ytdlp::{Chapter, Format, Info, SubtitleFile};
use super::{Content, is_media_ext, safe_ext};

/// A video codec, as far as choosing a container goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
    Other,
}

impl VideoCodec {
    /// From yt-dlp's `vcodec` (for example `avc1.640028`), or, when it gave none,
    /// from the format's extension.
    pub fn of(format: &Format) -> Option<VideoCodec> {
        if !format.has_video() {
            return None;
        }
        let named = format
            .vcodec
            .as_deref()
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty());
        Some(match named.as_deref() {
            Some(c) if c.starts_with("avc") || c.starts_with("h264") => VideoCodec::H264,
            Some(c)
                if c.starts_with("hev")
                    || c.starts_with("hvc")
                    || c.starts_with("h265")
                    || c.starts_with("hevc") =>
            {
                VideoCodec::H265
            }
            Some(c) if c.starts_with("vp8") || c.starts_with("vp08") => VideoCodec::Vp8,
            Some(c) if c.starts_with("vp9") || c.starts_with("vp09") => VideoCodec::Vp9,
            Some(c) if c.starts_with("av01") || c.starts_with("av1") => VideoCodec::Av1,
            Some(_) => VideoCodec::Other,
            // yt-dlp did not say: the container is the best guess.
            None => match format.ext.as_deref() {
                Some("mp4" | "m4v") => VideoCodec::H264,
                Some("webm") => VideoCodec::Vp9,
                _ => VideoCodec::Other,
            },
        })
    }

    /// The codec in plain words.
    pub fn name(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265",
            VideoCodec::Vp8 => "VP8",
            VideoCodec::Vp9 => "VP9",
            VideoCodec::Av1 => "AV1",
            VideoCodec::Other => "another codec",
        }
    }
}

/// An audio codec, as far as choosing a container goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Alac,
    Ac3,
    Eac3,
    Pcm,
    Other,
}

impl AudioCodec {
    /// From yt-dlp's `acodec` (for example `mp4a.40.2`), or, when it gave none,
    /// from the format's extension.
    pub fn of(format: &Format) -> Option<AudioCodec> {
        if !format.has_audio() {
            return None;
        }
        let named = format
            .acodec
            .as_deref()
            .map(|c| c.trim().to_ascii_lowercase())
            .filter(|c| !c.is_empty());
        Some(match named.as_deref() {
            // MP3 inside MP4 is `mp4a.40.34` (or `mp4a.6b`).
            Some("mp4a.40.34" | "mp4a.6b" | "mp4a.69") => AudioCodec::Mp3,
            Some(c) if c.starts_with("mp4a") || c.starts_with("aac") => AudioCodec::Aac,
            Some(c) if c.starts_with("mp3") => AudioCodec::Mp3,
            Some(c) if c.starts_with("opus") => AudioCodec::Opus,
            Some(c) if c.starts_with("vorbis") => AudioCodec::Vorbis,
            Some(c) if c.starts_with("flac") => AudioCodec::Flac,
            Some(c) if c.starts_with("alac") => AudioCodec::Alac,
            Some(c) if c.starts_with("ac-3") || c.starts_with("ac3") => AudioCodec::Ac3,
            Some(c) if c.starts_with("ec-3") || c.starts_with("eac3") => AudioCodec::Eac3,
            Some(c) if c.starts_with("pcm") || c == "wav" => AudioCodec::Pcm,
            Some(_) => AudioCodec::Other,
            None => match format.ext.as_deref() {
                Some("m4a" | "mp4" | "aac") => AudioCodec::Aac,
                Some("webm" | "opus") => AudioCodec::Opus,
                Some("mp3") => AudioCodec::Mp3,
                Some("ogg") => AudioCodec::Vorbis,
                Some("flac") => AudioCodec::Flac,
                Some("wav") => AudioCodec::Pcm,
                _ => AudioCodec::Other,
            },
        })
    }

    /// The codec in plain words.
    pub fn name(self) -> &'static str {
        match self {
            AudioCodec::Aac => "AAC",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::Opus => "Opus",
            AudioCodec::Vorbis => "Vorbis",
            AudioCodec::Flac => "FLAC",
            AudioCodec::Alac => "ALAC",
            AudioCodec::Ac3 => "Dolby Digital",
            AudioCodec::Eac3 => "Dolby Digital Plus",
            AudioCodec::Pcm => "PCM",
            AudioCodec::Other => "another codec",
        }
    }

    /// Whether nothing of the sound was thrown away.
    pub fn lossless(self) -> bool {
        matches!(self, AudioCodec::Flac | AudioCodec::Alac | AudioCodec::Pcm)
    }

    /// The audio-only format this codec is encoded into. Its bit rates
    /// ([`OutputFormat::kbps_choices`]) are the codec's wherever it is encoded,
    /// inside a video too; `None` for codecs Wirecatch never encodes lossy.
    fn audio_format(self) -> Option<OutputFormat> {
        match self {
            AudioCodec::Mp3 => Some(OutputFormat::Mp3),
            AudioCodec::Aac => Some(OutputFormat::M4a),
            AudioCodec::Opus => Some(OutputFormat::Opus),
            _ => None,
        }
    }

    /// The audio qualities offered when encoding to this codec, in kbps; empty
    /// for lossless codecs.
    pub fn kbps_choices(self) -> &'static [u32] {
        self.audio_format().map_or(&[], OutputFormat::kbps_choices)
    }

    /// The usual quality when encoding to this codec, in kbps.
    pub fn default_kbps(self) -> Option<u32> {
        self.audio_format().and_then(OutputFormat::default_kbps)
    }
}

/// The file a media download is saved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Container {
    Mp4,
    Mkv,
    Webm,
    Mp3,
    M4a,
    Opus,
    Flac,
    Wav,
}

impl Container {
    /// The container a file with extension `ext` is, when it is one Wirecatch
    /// writes; a downloaded file kept in its own format is copied back into it.
    pub fn from_ext(ext: &str) -> Option<Container> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "mp4" => Container::Mp4,
            "mkv" => Container::Mkv,
            "webm" => Container::Webm,
            "mp3" => Container::Mp3,
            "m4a" => Container::M4a,
            "opus" => Container::Opus,
            "flac" => Container::Flac,
            "wav" => Container::Wav,
            _ => return None,
        })
    }

    pub fn ext(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Webm => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "m4a",
            Container::Opus => "opus",
            Container::Flac => "flac",
            Container::Wav => "wav",
        }
    }

    /// The format's name for a person, for example "MP4".
    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::Mkv => "MKV",
            Container::Webm => "WebM",
            Container::Mp3 => "MP3",
            Container::M4a => "M4A",
            Container::Opus => "Opus",
            Container::Flac => "FLAC",
            Container::Wav => "WAV",
        }
    }

    /// ffmpeg's muxer for it, always named so the extension never decides.
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "matroska",
            Container::Webm => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "ipod",
            Container::Opus => "opus",
            Container::Flac => "flac",
            Container::Wav => "wav",
        }
    }

    /// Whether it holds video at all.
    pub fn is_video(self) -> bool {
        matches!(self, Container::Mp4 | Container::Mkv | Container::Webm)
    }

    /// Whether a video in `codec` goes in as is. MP4 takes H.264 and H.265 only:
    /// VP9 and AV1 in MP4 are valid but many players and editors refuse them, and
    /// MP4 is chosen for those players.
    pub fn holds_video(self, codec: VideoCodec) -> bool {
        match self {
            Container::Mkv => true,
            Container::Mp4 => matches!(codec, VideoCodec::H264 | VideoCodec::H265),
            Container::Webm => matches!(codec, VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1),
            _ => false,
        }
    }

    /// Whether audio in `codec` goes in as is.
    pub fn holds_audio(self, codec: AudioCodec) -> bool {
        match self {
            Container::Mkv => true,
            Container::Mp4 => matches!(
                codec,
                AudioCodec::Aac | AudioCodec::Mp3 | AudioCodec::Ac3 | AudioCodec::Eac3
            ),
            Container::M4a => matches!(codec, AudioCodec::Aac | AudioCodec::Alac),
            Container::Webm => matches!(codec, AudioCodec::Opus | AudioCodec::Vorbis),
            Container::Mp3 => codec == AudioCodec::Mp3,
            Container::Opus => codec == AudioCodec::Opus,
            Container::Flac => codec == AudioCodec::Flac,
            // WAV is always written as 16-bit PCM.
            Container::Wav => false,
        }
    }

    /// The video codec a video is re-encoded to when it does not fit.
    pub fn video_target(self) -> VideoCodec {
        match self {
            Container::Webm => VideoCodec::Vp9,
            _ => VideoCodec::H264,
        }
    }

    /// The audio codec audio is re-encoded to when it does not fit.
    pub fn audio_target(self) -> AudioCodec {
        match self {
            Container::Mp4 | Container::Mkv | Container::M4a => AudioCodec::Aac,
            Container::Webm | Container::Opus => AudioCodec::Opus,
            Container::Mp3 => AudioCodec::Mp3,
            Container::Flac => AudioCodec::Flac,
            Container::Wav => AudioCodec::Pcm,
        }
    }

    /// ffmpeg's encoder for subtitles inside it; `None` when subtitles are saved
    /// as files next to it.
    pub fn subtitle_codec(self) -> Option<&'static str> {
        match self {
            Container::Mp4 => Some("mov_text"),
            Container::Mkv => Some("srt"),
            Container::Webm => Some("webvtt"),
            _ => None,
        }
    }

    /// Whether it can hold chapters.
    pub fn chapters(self) -> bool {
        matches!(
            self,
            Container::Mp4 | Container::Mkv | Container::Webm | Container::M4a
        )
    }

    /// Whether it can hold a cover picture.
    pub fn cover(self) -> bool {
        matches!(self, Container::Mp3 | Container::M4a | Container::Flac)
    }

    /// Whether title and artist tags are written into it.
    pub fn tags(self) -> bool {
        matches!(
            self,
            Container::Mp3 | Container::M4a | Container::Flac | Container::Opus
        )
    }
}

/// What happens to the video of a download being saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum VideoAction {
    /// There is no video.
    None,
    Copy,
    /// Re-encoded to `codec` at about `kbps`.
    Encode {
        codec: VideoCodec,
        kbps: u32,
    },
}

impl VideoAction {
    /// What the quality window is told about it, without the codec and bit rate.
    pub fn stream(self) -> StreamAction {
        match self {
            VideoAction::None => StreamAction::None,
            VideoAction::Copy => StreamAction::Copy,
            VideoAction::Encode { .. } => StreamAction::Encode,
        }
    }
}

/// What happens to the audio of a download being saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AudioAction {
    /// There is no audio.
    None,
    Copy,
    /// Re-encoded to `codec`, at `kbps` for a lossy codec.
    Encode {
        codec: AudioCodec,
        kbps: Option<u32>,
    },
}

impl AudioAction {
    /// What the quality window is told about it, without the codec and bit rate.
    pub fn stream(self) -> StreamAction {
        match self {
            AudioAction::None => StreamAction::None,
            AudioAction::Copy => StreamAction::Copy,
            AudioAction::Encode { .. } => StreamAction::Encode,
        }
    }
}

/// One subtitle language chosen for a download, found again by its key when the
/// page is read again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SubtitlePick {
    /// yt-dlp's key for it, for example `en` or `en-orig`.
    pub key: String,
    /// From the auto-generated list.
    pub auto: bool,
    /// The language code, for example `en`.
    pub language: String,
    /// The language in words, for the track's name.
    pub name: String,
}

/// How a downloaded video or audio is saved, stored with the download so a
/// conversion that was paused or failed runs again by itself (D24). A download
/// without one is saved as it was downloaded, as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OutputJob {
    pub container: Container,
    pub video: VideoAction,
    pub audio: AudioAction,
    #[serde(default)]
    pub subtitles: Vec<SubtitlePick>,
    /// Subtitles go next to the file as `.srt` files instead of inside it.
    #[serde(default)]
    pub subtitle_files: bool,
    /// What else goes into the file. Stored as fields of the job itself, as
    /// jobs saved before were.
    #[serde(flatten)]
    pub extras: Extras,
    /// The media's length, for the conversion's progress.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// The extension of the file as downloaded, for "Keep the original".
    pub original_ext: String,
    /// Saved in the format it was downloaded in ("Original"): ffmpeg only runs
    /// when something goes inside the file.
    #[serde(default)]
    pub original: bool,
}

/// What goes into a saved file besides its streams, where its format holds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Extras {
    /// The video's chapters.
    #[serde(default)]
    pub chapters: bool,
    /// Title, artist and cover picture.
    #[serde(default)]
    pub tags: bool,
}

impl OutputJob {
    /// Whether ffmpeg writes the file. Otherwise the download is kept as it is
    /// (joined, if it came as separate video and audio), and only subtitle
    /// files are added next to it.
    pub fn needs_pass(&self) -> bool {
        !self.original
            || self.converts()
            || self.embedded_subtitles()
            || self.wants_metadata()
            || self.wants_cover()
    }

    /// Whether anything is re-encoded.
    pub fn converts(&self) -> bool {
        matches!(self.video, VideoAction::Encode { .. })
            || matches!(self.audio, AudioAction::Encode { .. })
    }

    /// Whether the subtitles are saved as files next to the download.
    pub fn sidecar_subtitles(&self) -> bool {
        !self.subtitles.is_empty()
            && (self.subtitle_files || self.container.subtitle_codec().is_none())
    }

    /// Whether the subtitles go inside the file.
    pub fn embedded_subtitles(&self) -> bool {
        !self.subtitles.is_empty() && !self.sidecar_subtitles()
    }

    /// Whether a metadata file (tags, chapters) is written for ffmpeg.
    pub fn wants_metadata(&self) -> bool {
        (self.extras.chapters && self.container.chapters())
            || (self.extras.tags && self.container.tags())
    }

    /// Whether a cover picture is downloaded for the file.
    pub fn wants_cover(&self) -> bool {
        self.extras.tags && self.container.cover()
    }
}

/// The format and streams one "Save as" choice of one quality uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Variant {
    /// The format ids, as for [`MediaJob::Page`](super::MediaJob::Page).
    pub video: Option<String>,
    pub audio: Option<String>,
    /// yt-dlp's selection for the same, for example `137+140`.
    pub selection: String,
    pub container: Container,
    pub video_action: VideoAction,
    pub audio_action: AudioAction,
    /// The audio codec downloaded, which decides whether an audio quality can be
    /// kept as is.
    pub audio_codec: Option<AudioCodec>,
    /// The extension the downloaded streams get when kept as they came ("Keep
    /// the original" after a failed conversion).
    pub kept_ext: String,
    pub fit: OutputFit,
}

/// The extension downloaded streams get when kept as they came: MP4 for MP4
/// video with M4A audio, WebM for WebM with WebM, MKV for any other pair; a
/// single file keeps its own (a media extension, never one a site chose
/// otherwise).
pub(crate) fn kept_ext(video: Option<&Format>, audio: Option<&Format>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => {
            let (v_ext, a_ext) = (
                safe_ext(v.ext.as_deref(), Content::Video),
                safe_ext(a.ext.as_deref(), Content::Audio),
            );
            // Matroska holds anything, so it is the choice when either side
            // named something that is not a media extension.
            let named = [&v.ext, &a.ext]
                .iter()
                .all(|e| e.as_deref().is_some_and(is_media_ext));
            match (v_ext.as_str(), a_ext.as_str()) {
                ("mp4", "m4a" | "mp4") if named => "mp4",
                ("webm", "webm") if named => "webm",
                _ => "mkv",
            }
            .to_string()
        }
        (Some(f), None) => safe_ext(
            f.ext.as_deref(),
            if f.has_video() {
                Content::Video
            } else {
                Content::Audio
            },
        ),
        (None, Some(a)) => safe_ext(a.ext.as_deref(), Content::Audio),
        (None, None) => "mkv".into(),
    }
}

/// The lowest video bit rate a re-encode is given, in kbps.
const MIN_VIDEO_KBPS: u32 = 300;
/// The highest video bit rate a re-encode is given, in kbps.
const MAX_VIDEO_KBPS: u32 = 80_000;

/// Video bit rate in kbps re-encoded to `target` from a video of `source` at
/// `source_kbps` (and `height`): about the source's, more when the source codec is
/// more efficient than the target, or a usual rate for the height when the site
/// gave none.
pub(crate) fn video_kbps(
    source: VideoCodec,
    target: VideoCodec,
    source_kbps: Option<f64>,
    height: Option<u32>,
) -> u32 {
    let efficiency = |c: VideoCodec| match c {
        VideoCodec::Av1 => 1.8,
        VideoCodec::H265 | VideoCodec::Vp9 => 1.4,
        _ => 1.0,
    };
    let known = source_kbps
        .filter(|k| k.is_finite() && *k > 0.0)
        .map(|k| k * efficiency(source) / efficiency(target));
    let kbps = known.unwrap_or_else(|| {
        let h264 = match height.unwrap_or(720) {
            0..=240 => 500.0,
            241..=360 => 1_000.0,
            361..=480 => 2_000.0,
            481..=720 => 4_000.0,
            721..=1080 => 8_000.0,
            1081..=1440 => 16_000.0,
            _ => 35_000.0,
        };
        h264 / efficiency(target)
    });
    // `kbps` is positive here, so a rate that is no `u32` (or no `u64`, or
    // infinite after the efficiency factor) is one too large.
    let whole = f64_to_u64(kbps.round())
        .and_then(|k| u32::try_from(k).ok())
        .unwrap_or(u32::MAX);
    whole.clamp(MIN_VIDEO_KBPS, MAX_VIDEO_KBPS)
}

/// A format's video bit rate in kbps, when yt-dlp gave one.
fn video_rate(format: &Format) -> Option<f64> {
    format
        .vbr
        .or(format.tbr)
        .filter(|r| r.is_finite() && *r > 0.0)
}

/// A format's audio bit rate in kbps, when yt-dlp gave one.
pub(crate) fn audio_rate(format: &Format) -> Option<u32> {
    format
        .abr
        .or_else(|| (!format.has_video()).then_some(format.tbr).flatten())
        .filter(|r| *r > 0.0 && *r < 100_000.0)
        .and_then(|r| f64_to_u64(r.round()))
        .and_then(|r| u32::try_from(r).ok())
}

fn rate(format: &Format) -> f64 {
    format
        .tbr
        .or(format.abr)
        .filter(|r| r.is_finite())
        .unwrap_or(0.0)
}

/// Bytes for `kbps` over `duration_ms`.
fn bytes_for(kbps: u32, duration_ms: Option<u64>) -> Option<u64> {
    duration_ms
        .filter(|d| *d > 0)
        .map(|d| u64::from(kbps) * d / 8)
}

/// The best of `formats` for a row whose video runs at `fps`: the same frame
/// rate first, then the highest bit rate.
fn best_for_fps<'a>(formats: impl Iterator<Item = &'a Format>, fps: f64) -> Option<&'a Format> {
    formats.max_by(|a, b| {
        let near = |f: &Format| (f.fps.unwrap_or(fps) - fps).abs() < 1.0;
        near(a).cmp(&near(b)).then(rate(a).total_cmp(&rate(b)))
    })
}

fn best_audio<'a>(formats: impl Iterator<Item = &'a Format>) -> Option<&'a Format> {
    formats.max_by(|a, b| rate(a).total_cmp(&rate(b)))
}

/// The "Save as" choices of one video quality, Original first: the original
/// formats copied into their own container, then MP4, MKV and WebM. Each takes
/// the streams of this height that fit the container as they are, and re-encodes
/// only what has no such stream.
///
/// `at` holds the video formats of this height; `audios` every audio-only
/// format; `original` the streams of the Original choice (video, and separate
/// audio if any) in `original_ext`.
pub(crate) fn video_variants(
    at: &[&Format],
    audios: &[&Format],
    original: (&Format, Option<&Format>),
    original_ext: &str,
    height: Option<u32>,
    duration_ms: Option<u64>,
) -> Vec<Variant> {
    let fps = original.0.fps.unwrap_or(0.0);
    let mut variants = Vec::with_capacity(4);
    // Original: the streams as chosen today, in their own container.
    let own = Container::from_ext(original_ext);
    variants.push(variant(
        OutputFormat::Original,
        own.unwrap_or(Container::Mkv),
        original,
        (VideoAction::Copy, original_audio_action(original)),
        original_ext,
        own,
        duration_ms,
    ));
    for (format, container) in [
        (OutputFormat::Mp4, Container::Mp4),
        (OutputFormat::Mkv, Container::Mkv),
        (OutputFormat::Webm, Container::Webm),
    ] {
        let fits = |f: &&Format| VideoCodec::of(f).is_some_and(|c| container.holds_video(c));
        let audio_fits = |f: &&Format| AudioCodec::of(f).is_some_and(|c| container.holds_audio(c));
        // MKV holds anything, so it keeps the original streams.
        let video = if container == Container::Mkv {
            Some(original.0)
        } else {
            best_for_fps(at.iter().copied().filter(fits), fps)
        };
        let (video, video_action) = if let Some(v) = video {
            (v, VideoAction::Copy)
        } else {
            let source = original.0;
            let codec = container.video_target();
            let from = VideoCodec::of(source).unwrap_or(VideoCodec::Other);
            (
                source,
                VideoAction::Encode {
                    codec,
                    kbps: video_kbps(from, codec, video_rate(source), height),
                },
            )
        };
        // The video format's own audio, when it fits, else the best separate
        // audio that fits, else the original's audio re-encoded.
        let own_audio = AudioCodec::of(video).filter(|c| container.holds_audio(*c));
        let (audio, audio_action) = if container == Container::Mkv {
            (original.1, original_audio_action(original))
        } else if own_audio.is_some() && video_action == VideoAction::Copy {
            (None, AudioAction::Copy)
        } else if let Some(a) = best_audio(audios.iter().copied().filter(audio_fits)) {
            (Some(a), AudioAction::Copy)
        } else {
            let source = original.1.or_else(|| video.has_audio().then_some(video));
            let target = container.audio_target();
            match source {
                Some(a) => (
                    original.1.filter(|_| !std::ptr::eq(a, video)),
                    AudioAction::Encode {
                        codec: target,
                        kbps: target.default_kbps(),
                    },
                ),
                None => (None, AudioAction::None),
            }
        };
        let audio = audio.filter(|a| !std::ptr::eq(*a, video));
        variants.push(variant(
            format,
            container,
            (video, audio),
            (video_action, audio_action),
            container.ext(),
            Some(container),
            duration_ms,
        ));
    }
    variants
}

fn original_audio_action(original: (&Format, Option<&Format>)) -> AudioAction {
    if original.1.is_some() || original.0.has_audio() {
        AudioAction::Copy
    } else {
        AudioAction::None
    }
}

/// One choice: the streams, what happens to them, and how it looks to the user.
fn variant(
    format: OutputFormat,
    container: Container,
    (video, audio): (&Format, Option<&Format>),
    (video_action, audio_action): (VideoAction, AudioAction),
    ext: &str,
    known: Option<Container>,
    duration_ms: Option<u64>,
) -> Variant {
    let audio_source = audio.or_else(|| video.has_audio().then_some(video));
    let audio_codec = audio_source.and_then(AudioCodec::of);
    let video_codec = match video_action {
        VideoAction::Encode { codec, .. } => Some(codec),
        _ => VideoCodec::of(video),
    };
    let video_size = match video_action {
        VideoAction::Encode { kbps, .. } => bytes_for(kbps, duration_ms),
        _ => video.size(),
    };
    let audio_size = match (audio_action, audio) {
        (AudioAction::Encode { kbps: Some(k), .. }, _) => bytes_for(k, duration_ms),
        (AudioAction::Encode { kbps: None, .. }, _) => None,
        (_, Some(a)) => a.size(),
        (_, None) => Some(0),
    };
    let selection = match audio {
        Some(a) => format!("{}+{}", video.id, a.id),
        None => video.id.clone(),
    };
    Variant {
        video: Some(video.id.clone()),
        audio: audio.map(|a| a.id.clone()),
        selection,
        container,
        video_action,
        audio_action,
        audio_codec,
        kept_ext: kept_ext(Some(video), audio),
        fit: OutputFit {
            format,
            ext: ext.to_string(),
            video: video_action.stream(),
            audio: audio_action.stream(),
            video_codec: video_codec.map(|c| c.name().to_string()),
            audio_codec: audio_codec.map(|c| c.name().to_string()),
            audio_kbps: audio_source.and_then(audio_rate),
            lossless_source: audio_codec.is_some_and(AudioCodec::lossless),
            size: video_size.zip(audio_size).map(|(v, a)| v + a),
            embeds_subtitles: known.is_some_and(|c| c.subtitle_codec().is_some()),
            chapters: known.is_some_and(Container::chapters),
        },
    }
}

/// The "Save as" choices of the audio-only quality, Original first, then MP3,
/// M4A, Opus, FLAC and WAV. A format whose codec the site offers copies that
/// audio; the others re-encode the best audio there is.
pub(crate) fn audio_variants(
    audios: &[&Format],
    original: &Format,
    original_ext: &str,
    duration_ms: Option<u64>,
) -> Vec<Variant> {
    let mut variants = Vec::with_capacity(6);
    let own = Container::from_ext(original_ext);
    let mut first = audio_variant(
        OutputFormat::Original,
        own.unwrap_or(Container::M4a),
        original,
        AudioAction::Copy,
        original_ext,
        duration_ms,
    );
    first.fit.chapters = own.is_some_and(Container::chapters);
    variants.push(first);
    let best = best_audio(audios.iter().copied()).unwrap_or(original);
    for (format, container) in [
        (OutputFormat::Mp3, Container::Mp3),
        (OutputFormat::M4a, Container::M4a),
        (OutputFormat::Opus, Container::Opus),
        (OutputFormat::Flac, Container::Flac),
        (OutputFormat::Wav, Container::Wav),
    ] {
        let fitting = best_audio(
            audios
                .iter()
                .copied()
                .filter(|a| AudioCodec::of(a).is_some_and(|c| container.holds_audio(c))),
        );
        let (source, action) = if let Some(a) = fitting {
            (a, AudioAction::Copy)
        } else {
            let target = container.audio_target();
            (
                best,
                AudioAction::Encode {
                    codec: target,
                    kbps: target.default_kbps(),
                },
            )
        };
        let mut v = audio_variant(
            format,
            container,
            source,
            action,
            container.ext(),
            duration_ms,
        );
        if container == Container::Wav {
            // 16-bit stereo at 48 kHz.
            v.fit.size = bytes_for(1_536, duration_ms);
        }
        variants.push(v);
    }
    variants
}

fn audio_variant(
    format: OutputFormat,
    container: Container,
    source: &Format,
    action: AudioAction,
    ext: &str,
    duration_ms: Option<u64>,
) -> Variant {
    let codec = AudioCodec::of(source);
    let size = match action {
        AudioAction::Encode { kbps: Some(k), .. } => bytes_for(k, duration_ms),
        AudioAction::Encode { kbps: None, .. } => None,
        _ => source.size(),
    };
    Variant {
        video: None,
        audio: Some(source.id.clone()),
        selection: source.id.clone(),
        container,
        video_action: VideoAction::None,
        audio_action: action,
        audio_codec: codec,
        kept_ext: kept_ext(None, Some(source)),
        fit: OutputFit {
            format,
            ext: ext.to_string(),
            video: StreamAction::None,
            audio: action.stream(),
            video_codec: None,
            audio_codec: codec.map(|c| c.name().to_string()),
            audio_kbps: audio_rate(source),
            lossless_source: codec.is_some_and(AudioCodec::lossless),
            size,
            embeds_subtitles: false,
            chapters: container == Container::M4a,
        },
    }
}

/// Most subtitle languages offered for one video.
const MAX_SUBTITLES: usize = 100;
/// Longest language name kept.
const MAX_NAME_CHARS: usize = 60;
/// Subtitle file formats Wirecatch reads, best first.
const SUBTITLE_EXTS: [&str; 4] = ["vtt", "srt", "ass", "ssa"];

/// The subtitles a page offers: human-written ones first (by name), then
/// auto-generated ones in the video's own language only (a site may list a
/// machine translation into every language; those are left out). Lists without
/// a file Wirecatch can read (live chat, for example) are left out too.
pub(crate) fn subtitles(info: &Info) -> Vec<(MediaSubtitle, SubtitlePick)> {
    let mut found = Vec::new();
    for (auto, list) in [(false, &info.subtitles), (true, &info.automatic_captions)] {
        let mut part: Vec<(MediaSubtitle, SubtitlePick)> = list
            .iter()
            .filter(|(key, files)| plain_key(key) && pick_file(files).is_some())
            .filter(|(_, files)| !auto || pick_file(files).is_some_and(|f| !translated(f)))
            .map(|(key, files)| {
                let language = language_of(key);
                let name = files
                    .iter()
                    .find_map(|f| f.name.as_deref())
                    .map(clean_name)
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| language.clone());
                let id = format!("{}:{key}", if auto { "auto" } else { "sub" });
                let pick = SubtitlePick {
                    key: key.clone(),
                    auto,
                    language: language.clone(),
                    name: name.clone(),
                };
                (
                    MediaSubtitle {
                        id,
                        language,
                        name,
                        auto,
                    },
                    pick,
                )
            })
            .collect();
        part.sort_by(|a, b| a.0.name.cmp(&b.0.name).then(a.0.id.cmp(&b.0.id)));
        found.extend(part);
    }
    found.truncate(MAX_SUBTITLES);
    found
}

/// A key that names a language (`en`, `pt-BR`, `en-orig`), not live chat or the
/// like.
fn plain_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 35
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(key, "live_chat" | "rechat" | "danmaku" | "comments")
}

/// The language code of a subtitle key: `en-orig` (YouTube's name for the
/// language a video is spoken in) is `en`.
pub(crate) fn language_of(key: &str) -> String {
    key.strip_suffix("-orig").unwrap_or(key).to_string()
}

/// A language name from the site, cut to a sensible length, without control
/// characters, and without YouTube's "(Original)".
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    name.trim()
        .trim_end_matches("(Original)")
        .trim()
        .to_string()
}

/// A machine translation of a subtitle (YouTube's `tlang`).
fn translated(file: &SubtitleFile) -> bool {
    file.url
        .as_deref()
        .and_then(|u| wirecatch_engine::Url::parse(u).ok())
        .is_some_and(|u| u.query_pairs().any(|(k, _)| k == "tlang"))
}

/// The file of a subtitle language Wirecatch downloads: `WebVTT`, else `SubRip`,
/// else `SubStation`.
pub(crate) fn pick_file(files: &[SubtitleFile]) -> Option<&SubtitleFile> {
    SUBTITLE_EXTS.iter().find_map(|ext| {
        files
            .iter()
            .find(|f| f.ext.as_deref() == Some(*ext) && f.url.is_some())
    })
}

/// The subtitle files of a page for `pick`, found again after reading it.
pub(crate) fn subtitle_files<'a>(info: &'a Info, pick: &SubtitlePick) -> Option<&'a SubtitleFile> {
    let list = if pick.auto {
        &info.automatic_captions
    } else {
        &info.subtitles
    };
    list.get(&pick.key).and_then(|files| pick_file(files))
}

/// A subtitle file format ffmpeg reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubtitleFormat {
    WebVtt,
    Srt,
    Ass,
}

impl SubtitleFormat {
    /// ffmpeg's demuxer for it.
    pub fn demuxer(self) -> &'static str {
        match self {
            SubtitleFormat::WebVtt => "webvtt",
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::Ass => "ass",
        }
    }

    /// Recognizes a downloaded subtitle file from its first bytes; `None` when
    /// it is not text in the format its extension `ext` promises, so a site
    /// cannot hand ffmpeg anything else.
    pub fn detect(head: &[u8], ext: &str) -> Option<SubtitleFormat> {
        let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
        let text = std::str::from_utf8(head)
            .or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
            .ok()?;
        let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
        match ext {
            "vtt" if first.starts_with("WEBVTT") => Some(SubtitleFormat::WebVtt),
            "ass" | "ssa" if first.eq_ignore_ascii_case("[Script Info]") => {
                Some(SubtitleFormat::Ass)
            }
            "srt" if first.bytes().all(|b| b.is_ascii_digit()) => Some(SubtitleFormat::Srt),
            _ => None,
        }
    }
}

/// A picture format ffmpeg reads for a cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageFormat {
    Jpeg,
    Png,
    Webp,
}

impl ImageFormat {
    /// ffmpeg's demuxer for it.
    pub fn demuxer(self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg_pipe",
            ImageFormat::Png => "png_pipe",
            ImageFormat::Webp => "webp_pipe",
        }
    }

    /// Recognizes a picture from its first bytes.
    pub fn detect(head: &[u8]) -> Option<ImageFormat> {
        if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(ImageFormat::Jpeg)
        } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ImageFormat::Png)
        } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            Some(ImageFormat::Webp)
        } else {
            None
        }
    }
}

/// Most chapters written.
const MAX_CHAPTERS: usize = 1_000;
/// Longest text written into a tag or a chapter title.
const MAX_TAG_CHARS: usize = 300;

/// An ffmpeg metadata file (`;FFMETADATA1`) with the title and artist, when
/// given, and the chapters that make sense (in order, inside the video, not
/// empty).
pub(crate) fn ffmetadata(
    title: Option<&str>,
    artist: Option<&str>,
    chapters: &[Chapter],
    duration_ms: Option<u64>,
) -> String {
    // Line by line, joined at the end.
    let mut lines = vec![";FFMETADATA1".to_string()];
    for (key, value) in [("title", title), ("artist", artist)] {
        if let Some(value) = value.map(escape).filter(|v| !v.is_empty()) {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut last_end = 0u64;
    for chapter in chapters.iter().take(MAX_CHAPTERS) {
        let ms = |s: Option<f64>| {
            s.filter(|s| (0.0..1e9).contains(s))
                .and_then(|s| f64_to_u64((s * 1000.0).round()))
        };
        let (Some(start), Some(end)) = (ms(chapter.start_time), ms(chapter.end_time)) else {
            continue;
        };
        let end = duration_ms.map_or(end, |d| end.min(d));
        if end <= start || start < last_end {
            continue;
        }
        last_end = end;
        lines.push(format!(
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}"
        ));
        if let Some(title) = chapter
            .title
            .as_deref()
            .map(escape)
            .filter(|t| !t.is_empty())
        {
            lines.push(format!("title={title}"));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Whether `chapters` holds any chapter [`ffmetadata`] would write.
pub(crate) fn has_chapters(chapters: &[Chapter]) -> bool {
    ffmetadata(None, None, chapters, None).contains("[CHAPTER]")
}

/// A value for an ffmpeg metadata file: control characters dropped, the
/// characters that file format gives a meaning (`=`, `;`, `#`, `\`) escaped.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TAG_CHARS)
    {
        if matches!(c, '=' | ';' | '#' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// A text for an ffmpeg option value (a track's name or language): no control
/// characters, a sensible length.
fn plain(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

/// The three-letter (ISO 639-2) code for a two-letter one, which MP4 needs for
/// a track's language; other codes are returned as they are.
pub(crate) fn iso639_2(language: &str) -> String {
    let primary = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase();
    let three = match primary.as_str() {
        "ar" => "ara",
        "bg" => "bul",
        "bn" => "ben",
        "ca" => "cat",
        "cs" => "cze",
        "da" => "dan",
        "de" => "ger",
        "el" => "gre",
        "en" => "eng",
        "es" => "spa",
        "et" => "est",
        "fa" => "per",
        "fi" => "fin",
        "fil" | "tl" => "fil",
        "fr" => "fre",
        "he" | "iw" => "heb",
        "hi" => "hin",
        "hr" => "hrv",
        "hu" => "hun",
        "id" => "ind",
        "it" => "ita",
        "ja" => "jpn",
        "ko" => "kor",
        "lt" => "lit",
        "lv" => "lav",
        "ms" => "may",
        "nl" => "dut",
        "no" | "nb" => "nor",
        "pa" => "pan",
        "pl" => "pol",
        "pt" => "por",
        "ro" => "rum",
        "ru" => "rus",
        "sk" => "slo",
        "sl" => "slv",
        "sr" => "srp",
        "sv" => "swe",
        "sw" => "swa",
        "ta" => "tam",
        "te" => "tel",
        "th" => "tha",
        "tr" => "tur",
        "uk" => "ukr",
        "ur" => "urd",
        "vi" => "vie",
        "zh" => "chi",
        other if other.len() == 3 && other.bytes().all(|b| b.is_ascii_lowercase()) => {
            return other.to_string();
        }
        _ => "und",
    };
    three.to_string()
}

/// A video encoder Wirecatch knows how to drive.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Encoder {
    /// ffmpeg's name for it.
    pub name: &'static str,
    /// It uses the graphics chip (or a media engine), so it is probed before use.
    pub hardware: bool,
    /// Options before the inputs (a device to open).
    pub global: &'static [&'static str],
    /// The filter that brings the frames into a form it takes.
    pub filter: &'static str,
    /// Its own options (speed and quality presets).
    pub options: &'static [&'static str],
    /// Operating systems it exists on.
    pub systems: &'static [&'static str],
}

const VAAPI_DEVICE: &[&str] = &["-vaapi_device", "/dev/dri/renderD128"];
const ANY: &[&str] = &["windows", "linux", "macos"];

/// H.264 encoders, in the order they are tried: hardware first (NVIDIA, Intel,
/// AMD, then the system's own Media Foundation on Windows or VA-API on Linux),
/// then software. The media add-on's ffmpeg is an LGPL build without libx264,
/// so its software encoder is `OpenH264`; a build with libx264 uses that instead.
pub(crate) const H264_ENCODERS: &[Encoder] = &[
    Encoder {
        name: "h264_nvenc",
        hardware: true,
        global: &[],
        filter: "format=yuv420p",
        options: &["-preset", "p5"],
        systems: &["windows", "linux"],
    },
    Encoder {
        name: "h264_qsv",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-preset", "medium"],
        systems: &["windows", "linux"],
    },
    Encoder {
        name: "h264_amf",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-quality", "balanced"],
        systems: &["windows", "linux"],
    },
    Encoder {
        name: "h264_mf",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &["-hw_encoding", "1"],
        systems: &["windows"],
    },
    Encoder {
        name: "h264_vaapi",
        hardware: true,
        global: VAAPI_DEVICE,
        filter: "format=nv12,hwupload",
        options: &[],
        systems: &["linux"],
    },
    Encoder {
        name: "libx264",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &["-preset", "veryfast"],
        systems: ANY,
    },
    Encoder {
        name: "libopenh264",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &[
            "-profile:v",
            "high",
            "-coder",
            "cabac",
            "-rc_mode",
            "bitrate",
        ],
        systems: ANY,
    },
];

/// VP9 encoders (for WebM), in the order they are tried.
pub(crate) const VP9_ENCODERS: &[Encoder] = &[
    Encoder {
        name: "vp9_qsv",
        hardware: true,
        global: &[],
        filter: "format=nv12",
        options: &[],
        systems: &["windows", "linux"],
    },
    Encoder {
        name: "vp9_vaapi",
        hardware: true,
        global: VAAPI_DEVICE,
        filter: "format=nv12,hwupload",
        options: &[],
        systems: &["linux"],
    },
    Encoder {
        name: "libvpx-vp9",
        hardware: false,
        global: &[],
        filter: "format=yuv420p",
        options: &[
            "-deadline",
            "good",
            "-cpu-used",
            "5",
            "-row-mt",
            "1",
            "-tile-columns",
            "2",
        ],
        systems: ANY,
    },
];

/// The encoders for `codec` on operating system `os` (`std::env::consts::OS`),
/// in the order they are tried.
pub(crate) fn encoders_for(codec: VideoCodec, os: &str) -> Vec<&'static Encoder> {
    let all = match codec {
        VideoCodec::Vp9 => VP9_ENCODERS,
        _ => H264_ENCODERS,
    };
    all.iter().filter(|e| e.systems.contains(&os)).collect()
}

/// The order to try encoders in, given which ones this ffmpeg has (`listed`,
/// `None` when it could not be asked) and which hardware ones failed a test
/// (`failed`): every listed hardware encoder not known to fail, then the listed
/// software ones. When ffmpeg could not be asked, only the last software encoder
/// (the one the media add-on's build has) is tried.
pub(crate) fn encoder_order<'a>(
    candidates: &[&'a Encoder],
    listed: Option<&HashSet<String>>,
    failed: &HashSet<&str>,
) -> Vec<&'a Encoder> {
    match listed {
        Some(listed) => {
            let has = |e: &&&Encoder| listed.contains(e.name);
            let hardware = candidates
                .iter()
                .filter(|e| e.hardware && !failed.contains(e.name))
                .filter(has);
            let software = candidates.iter().filter(|e| !e.hardware).filter(has);
            hardware.chain(software).copied().collect()
        }
        None => candidates
            .iter()
            .rev()
            .find(|e| !e.hardware)
            .copied()
            .into_iter()
            .collect(),
    }
}

/// The encoder names in `ffmpeg -encoders` output.
pub(crate) fn listed_encoders(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let flags = words.next()?;
            let name = words.next()?;
            // Lines look like ` V....D libx264   libx264 H.264 ...`.
            (flags.len() == 6
                && flags.starts_with(['V', 'A', 'S'])
                && flags
                    .bytes()
                    .skip(1)
                    .all(|b| b == b'.' || b.is_ascii_uppercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
            .then(|| name.to_string())
        })
        .collect()
}

/// The filters before `encoder`: the frame size rounded down to even numbers
/// (which every H.264 and VP9 encoder needs, and which a site's odd-sized video
/// would break otherwise), then the encoder's own.
fn encode_filter(encoder: &Encoder) -> String {
    format!("scale=trunc(iw/2)*2:trunc(ih/2)*2,{}", encoder.filter)
}

/// ffmpeg's arguments to try encoder `encoder` on a short generated clip, which
/// tells whether it works on this computer (a hardware encoder may be listed and
/// still have no device to run on).
pub(crate) fn probe_arguments(encoder: &Encoder) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.extend(encoder.global.iter().map(OsString::from));
    for arg in [
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=25:duration=1",
        "-frames:v",
        "10",
        "-vf",
        &encode_filter(encoder),
        "-c:v",
        encoder.name,
    ] {
        args.push(arg.into());
    }
    args.extend(encoder.options.iter().map(OsString::from));
    for arg in ["-b:v", "1000k", "-f", "null", "-"] {
        args.push(arg.into());
    }
    args
}

/// One audio encoder Wirecatch uses for `codec`, given the encoders ffmpeg
/// lists (`None` when it could not be asked): ffmpeg's name and any options it
/// needs.
pub(crate) fn audio_encoder(
    codec: AudioCodec,
    listed: Option<&HashSet<String>>,
) -> (&'static str, &'static [&'static str]) {
    let candidates: &[(&str, &[&str])] = match codec {
        AudioCodec::Mp3 => &[("libmp3lame", &[]), ("mp3_mf", &[])],
        AudioCodec::Opus => &[("libopus", &[]), ("opus", &["-strict", "experimental"])],
        AudioCodec::Flac => &[("flac", &[])],
        AudioCodec::Pcm => &[("pcm_s16le", &[])],
        _ => &[("aac", &[])],
    };
    candidates
        .iter()
        .find(|(name, _)| listed.is_none_or(|l| l.contains(*name)))
        .copied()
        .unwrap_or(candidates[0])
}

/// The files a conversion reads, all in the download's work folder.
#[derive(Debug, Clone)]
pub(crate) struct Inputs {
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

/// The input arguments for a local file read as `demuxer`: from a file only, as
/// that format only, as one absolute `file:` path (see
/// [`ffmpeg_input_args`]).
fn local_input(demuxer: &str, path: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        demuxer,
        "-f",
        demuxer,
        "-i",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(file_arg(path));
    args
}

/// `path` as one absolute `file:` argument, so a name starting with `-` or
/// holding `:` is never read as an option or a protocol.
pub(crate) fn file_arg(path: &Path) -> OsString {
    let absolute: PathBuf = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut arg = OsString::from("file:");
    arg.push(absolute.as_os_str());
    arg
}

/// ffmpeg's arguments to save `inputs` into `output` as `job` says: streams that
/// fit are copied, the rest re-encoded (video with `encoder`, audio with
/// `audio_encoder`), with subtitles, chapters, tags and a cover where the format
/// holds them, and progress reports on standard output.
pub(crate) fn arguments(
    job: &OutputJob,
    inputs: &Inputs,
    encoder: Option<&Encoder>,
    audio_encoder: (&str, &[&str]),
    output: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    let video_encoder = match job.video {
        VideoAction::Encode { .. } => encoder,
        _ => None,
    };
    if let Some(e) = video_encoder {
        push(&mut args, e.global);
    }
    let numbers = add_inputs(&mut args, job, inputs);
    add_maps(&mut args, job, &numbers);
    add_video_codec(&mut args, job, video_encoder);
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
    match job.container {
        Container::Mp4 | Container::M4a => push(&mut args, &["-movflags", "+faststart"]),
        Container::Mp3 => push(&mut args, &["-id3v2_version", "3"]),
        _ => {}
    }
    push(
        &mut args,
        &[
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            job.container.muxer(),
        ],
    );
    args.push(file_arg(output));
    args
}

/// Appends `words` to `args`.
fn push(args: &mut Vec<OsString>, words: &[&str]) {
    args.extend(words.iter().map(OsString::from));
}

/// The number ffmpeg gives each input that was added, counting from 0 in the
/// order they were added.
struct InputNumbers<'a> {
    video: Option<usize>,
    audio: Option<usize>,
    /// Each embedded subtitle's input, with the subtitle it is.
    subtitles: Vec<(usize, &'a SubtitlePick)>,
    metadata: Option<usize>,
    cover: Option<usize>,
}

/// Adds the inputs `job` uses to `args`, in a fixed order, and returns their
/// numbers for the mapping.
fn add_inputs<'a>(
    args: &mut Vec<OsString>,
    job: &OutputJob,
    inputs: &'a Inputs,
) -> InputNumbers<'a> {
    let mut next = 0usize;
    let mut number = |args: &mut Vec<OsString>, input: Vec<OsString>| {
        args.extend(input);
        next += 1;
        next - 1
    };
    let video = inputs
        .video
        .as_ref()
        .map(|(f, p)| number(args, ffmpeg_input_args(*f, p)));
    let audio = inputs
        .audio
        .as_ref()
        .map(|(f, p)| number(args, ffmpeg_input_args(*f, p)));
    let subtitles = if job.embedded_subtitles() {
        inputs
            .subtitles
            .iter()
            .map(|(f, p, pick)| (number(args, local_input(f.demuxer(), p)), pick))
            .collect()
    } else {
        Vec::new()
    };
    let metadata = inputs
        .metadata
        .as_ref()
        .filter(|_| job.wants_metadata())
        .map(|p| number(args, local_input("ffmetadata", p)));
    let cover = inputs
        .cover
        .as_ref()
        .filter(|_| job.wants_cover())
        .map(|(f, p)| number(args, local_input(f.demuxer(), p)));
    InputNumbers {
        video,
        audio,
        subtitles,
        metadata,
        cover,
    }
}

/// Picks the streams that go into the file: the video, the audio (from the
/// video's input when it came joined), each subtitle and the cover.
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

/// How the video is written: not at all, re-encoded with `encoder` at the
/// job's rate, or copied.
fn add_video_codec(args: &mut Vec<OsString>, job: &OutputJob, encoder: Option<&Encoder>) {
    match (job.video, encoder) {
        (VideoAction::None, _) => {}
        (VideoAction::Encode { kbps, .. }, Some(e)) => {
            push(args, &["-filter:v:0", &encode_filter(e), "-c:v:0", e.name]);
            push(args, e.options);
            let (rate, max, buf) = (kbps, kbps.saturating_mul(3) / 2, kbps.saturating_mul(2));
            for (flag, value) in [
                ("-b:v:0", rate),
                ("-maxrate:v:0", max),
                ("-bufsize:v:0", buf),
            ] {
                args.push(flag.into());
                args.push(format!("{value}k").into());
            }
        }
        _ => push(args, &["-c:v:0", "copy"]),
    }
}

/// How the audio is written: not at all, copied, or re-encoded with
/// `encoder` (its name and options) at the job's rate, if it has one.
fn add_audio_codec(args: &mut Vec<OsString>, job: &OutputJob, encoder: (&str, &[&str])) {
    match job.audio {
        AudioAction::None => {}
        AudioAction::Copy => push(args, &["-c:a", "copy"]),
        AudioAction::Encode { kbps, .. } => {
            push(args, &["-c:a", encoder.0]);
            push(args, encoder.1);
            if let Some(kbps) = kbps {
                args.push("-b:a".into());
                args.push(format!("{kbps}k").into());
            }
        }
    }
}

/// Writes the embedded subtitles with `codec`, each named and labelled with
/// its language.
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

/// Writes the cover picture as an attached JPEG picture after the video, if
/// there is one.
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
pub(crate) fn srt_arguments(format: SubtitleFormat, input: &Path, output: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.extend(local_input(format.demuxer(), input));
    for arg in ["-map", "0:s:0", "-c:s", "srt", "-f", "srt"] {
        args.push(arg.into());
    }
    args.push(file_arg(output));
    args
}

/// How far a conversion is, in milliseconds of media, from one line of
/// ffmpeg's `-progress` report (`out_time_us=1234567`).
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

/// Subtitle languages remembered for next time from the subtitles picked, and
/// whether auto-generated ones were wanted. `None` when the video had no
/// subtitles to pick from, so the last choice stays.
pub(crate) fn remembered_subtitles(
    offered: &[MediaSubtitle],
    picked: &[String],
) -> Option<(Vec<String>, Option<bool>)> {
    if offered.is_empty() {
        return None;
    }
    let mut languages: Vec<String> = Vec::new();
    let mut any_auto = false;
    for subtitle in offered.iter().filter(|s| picked.contains(&s.id)) {
        let language = subtitle.language.to_ascii_lowercase();
        if !languages.contains(&language) {
            languages.push(language);
        }
        any_auto |= subtitle.auto;
    }
    // Auto-generated subtitles are remembered as wanted once one is ticked, and
    // as unwanted once some were offered and none was ticked.
    let auto_offered = offered.iter().any(|s| s.auto);
    let auto = if any_auto {
        Some(true)
    } else if auto_offered {
        Some(false)
    } else {
        None
    };
    Some((languages, auto))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(json: serde_json::Value) -> Format {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn codecs_are_read_from_yt_dlp_names() {
        let v = |c: &str| {
            VideoCodec::of(&format(
                serde_json::json!({"format_id": "1", "vcodec": c, "acodec": "none"}),
            ))
        };
        assert_eq!(v("avc1.640028"), Some(VideoCodec::H264));
        assert_eq!(v("vp09.00.40.08"), Some(VideoCodec::Vp9));
        assert_eq!(v("vp9"), Some(VideoCodec::Vp9));
        assert_eq!(v("av01.0.08M.08"), Some(VideoCodec::Av1));
        assert_eq!(v("hev1.1.6.L93.B0"), Some(VideoCodec::H265));
        assert_eq!(v("none"), None);
        assert_eq!(v("theora"), Some(VideoCodec::Other));
        let a = |c: &str| {
            AudioCodec::of(&format(
                serde_json::json!({"format_id": "1", "vcodec": "none", "acodec": c}),
            ))
        };
        assert_eq!(a("mp4a.40.2"), Some(AudioCodec::Aac));
        assert_eq!(a("mp4a.40.34"), Some(AudioCodec::Mp3));
        assert_eq!(a("opus"), Some(AudioCodec::Opus));
        assert_eq!(a("ec-3"), Some(AudioCodec::Eac3));
        assert_eq!(a("none"), None);
        // Without a codec name, the extension decides.
        let unnamed = format(serde_json::json!({"format_id": "1", "ext": "webm"}));
        assert_eq!(VideoCodec::of(&unnamed), Some(VideoCodec::Vp9));
        assert_eq!(AudioCodec::of(&unnamed), Some(AudioCodec::Opus));
    }

    /// Which container takes which codec as is, and which needs a conversion.
    #[test]
    fn containers_hold_what_their_players_play() {
        use AudioCodec as A;
        use VideoCodec as V;
        for (container, video, fits) in [
            (Container::Mp4, V::H264, true),
            (Container::Mp4, V::H265, true),
            (Container::Mp4, V::Vp9, false),
            (Container::Mp4, V::Av1, false),
            (Container::Webm, V::Vp9, true),
            (Container::Webm, V::Av1, true),
            (Container::Webm, V::H264, false),
            (Container::Mkv, V::H264, true),
            (Container::Mkv, V::Vp9, true),
            (Container::Mkv, V::Other, true),
        ] {
            assert_eq!(
                container.holds_video(video),
                fits,
                "{container:?} {video:?}"
            );
        }
        for (container, audio, fits) in [
            (Container::Mp4, A::Aac, true),
            (Container::Mp4, A::Opus, false),
            (Container::Webm, A::Opus, true),
            (Container::Webm, A::Aac, false),
            (Container::Mkv, A::Opus, true),
            (Container::M4a, A::Aac, true),
            (Container::M4a, A::Opus, false),
            (Container::Mp3, A::Mp3, true),
            (Container::Mp3, A::Aac, false),
            (Container::Opus, A::Opus, true),
            (Container::Flac, A::Opus, false),
            (Container::Wav, A::Pcm, false),
        ] {
            assert_eq!(
                container.holds_audio(audio),
                fits,
                "{container:?} {audio:?}"
            );
        }
        assert_eq!(Container::Mp4.video_target(), V::H264);
        assert_eq!(Container::Webm.video_target(), V::Vp9);
        assert_eq!(Container::Webm.audio_target(), A::Opus);
        assert_eq!(Container::Mp4.audio_target(), A::Aac);
        assert_eq!(Container::Mp4.subtitle_codec(), Some("mov_text"));
        assert_eq!(Container::Mkv.subtitle_codec(), Some("srt"));
        assert_eq!(Container::Webm.subtitle_codec(), Some("webvtt"));
        assert_eq!(Container::Mp3.subtitle_codec(), None);
        assert!(Container::M4a.chapters() && !Container::Mp3.chapters());
        assert!(Container::Mp3.cover() && Container::M4a.cover() && !Container::Opus.cover());
    }

    /// A format as yt-dlp describes one of a YouTube page's.
    fn listed(
        id: &str,
        ext: &str,
        codecs: (&str, &str),
        height: Option<u32>,
        fps: f64,
        tbr: f64,
    ) -> Format {
        format(serde_json::json!({
            "format_id": id, "ext": ext, "vcodec": codecs.0, "acodec": codecs.1,
            "height": height, "fps": fps, "tbr": tbr, "url": "u", "filesize": 1000
        }))
    }

    /// A YouTube-like 1080p row: VP9 is best, but MP4 takes the H.264 of the
    /// same height and frame rate and copies it.
    #[test]
    fn a_container_takes_matching_streams_of_the_same_quality() {
        let vp9 = listed("303", "webm", ("vp9", "none"), Some(1080), 60.0, 4000.0);
        let avc30 = listed(
            "137",
            "mp4",
            ("avc1.640028", "none"),
            Some(1080),
            30.0,
            4500.0,
        );
        let avc60 = listed(
            "299",
            "mp4",
            ("avc1.64002a", "none"),
            Some(1080),
            60.0,
            6000.0,
        );
        let aac = listed("140", "m4a", ("none", "mp4a.40.2"), None, 0.0, 128.0);
        let opus = listed("251", "webm", ("none", "opus"), None, 0.0, 160.0);
        let variants = video_variants(
            &[&vp9, &avc30, &avc60],
            &[&aac, &opus],
            (&vp9, Some(&opus)),
            "webm",
            Some(1080),
            Some(60_000),
        );
        let by = |format: OutputFormat| variants.iter().find(|v| v.fit.format == format).unwrap();
        let original = by(OutputFormat::Original);
        assert_eq!(original.selection, "303+251");
        assert_eq!(
            (original.fit.video, original.fit.audio),
            (StreamAction::Copy, StreamAction::Copy)
        );
        let mp4 = by(OutputFormat::Mp4);
        assert_eq!(mp4.selection, "299+140", "the 60 fps H.264 and the AAC");
        assert_eq!(mp4.video_action, VideoAction::Copy);
        assert_eq!(mp4.audio_action, AudioAction::Copy);
        assert_eq!(mp4.fit.video_codec.as_deref(), Some("H.264"));
        let webm = by(OutputFormat::Webm);
        assert_eq!(webm.selection, "303+251");
        assert_eq!(webm.fit.video, StreamAction::Copy);
        let mkv = by(OutputFormat::Mkv);
        assert_eq!(mkv.selection, "303+251");
        assert_eq!(mkv.container, Container::Mkv);
    }

    /// 2160p comes only as VP9: MP4 re-encodes it, and says so.
    #[test]
    fn a_height_without_a_matching_codec_is_re_encoded() {
        let uhd = listed("313", "webm", ("vp9", "none"), Some(2160), 30.0, 12_000.0);
        let aac = listed("140", "m4a", ("none", "mp4a.40.2"), None, 0.0, 128.0);
        let opus = listed("251", "webm", ("none", "opus"), None, 0.0, 160.0);
        let variants = video_variants(
            &[&uhd],
            &[&aac, &opus],
            (&uhd, Some(&opus)),
            "webm",
            Some(2160),
            Some(60_000),
        );
        let mp4 = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Mp4)
            .unwrap();
        assert_eq!(mp4.selection, "313+140");
        assert_eq!(
            (mp4.fit.video, mp4.fit.audio),
            (StreamAction::Encode, StreamAction::Copy)
        );
        let VideoAction::Encode { codec, kbps } = mp4.video_action else {
            panic!("not re-encoded: {:?}", mp4.video_action);
        };
        assert_eq!(codec, VideoCodec::H264);
        assert_eq!(kbps, 16_800, "VP9 at 12 Mb/s needs more as H.264");
        assert_eq!(mp4.fit.size, Some(16_800 * 60_000 / 8 + 1000));
    }

    /// WebM from a page with only H.264 and AAC: both are re-encoded.
    #[test]
    fn webm_from_h264_and_aac_re_encodes_both() {
        let avc30 = listed(
            "137",
            "mp4",
            ("avc1.640028", "none"),
            Some(1080),
            30.0,
            4500.0,
        );
        let aac = listed("140", "m4a", ("none", "mp4a.40.2"), None, 0.0, 128.0);
        let variants = video_variants(
            &[&avc30],
            &[&aac],
            (&avc30, Some(&aac)),
            "mp4",
            Some(1080),
            None,
        );
        let webm = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Webm)
            .unwrap();
        assert_eq!(
            (webm.fit.video, webm.fit.audio),
            (StreamAction::Encode, StreamAction::Encode)
        );
        assert_eq!(webm.fit.size, None, "no length, no estimate");
        assert!(webm.fit.embeds_subtitles && webm.fit.chapters);
    }

    /// A single file with video and audio (no separate streams).
    #[test]
    fn a_file_with_video_and_audio_is_kept_together_when_it_fits() {
        let both = format(serde_json::json!({
            "format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a.40.2", "height": 360, "url": "u"
        }));
        let variants = video_variants(&[&both], &[], (&both, None), "mp4", Some(360), None);
        let mp4 = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Mp4)
            .unwrap();
        assert_eq!(mp4.selection, "18");
        assert_eq!(
            (mp4.video_action, mp4.audio_action),
            (VideoAction::Copy, AudioAction::Copy)
        );
        let webm = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Webm)
            .unwrap();
        assert_eq!(webm.selection, "18");
        assert!(matches!(
            webm.audio_action,
            AudioAction::Encode {
                codec: AudioCodec::Opus,
                ..
            }
        ));
        assert_eq!(webm.fit.video, StreamAction::Encode);
    }

    #[test]
    fn audio_formats_copy_what_fits_and_encode_the_best_otherwise() {
        let aac = format(
            serde_json::json!({"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.5, "filesize": 100}),
        );
        let opus = format(
            serde_json::json!({"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160}),
        );
        let variants = audio_variants(&[&aac, &opus], &aac, "m4a", Some(8_000));
        let by = |format: OutputFormat| variants.iter().find(|v| v.fit.format == format).unwrap();
        assert_eq!(by(OutputFormat::Original).selection, "140");
        assert!(by(OutputFormat::Original).fit.chapters);
        assert_eq!(by(OutputFormat::M4a).audio_action, AudioAction::Copy);
        assert_eq!(by(OutputFormat::M4a).fit.audio_kbps, Some(130));
        assert_eq!(by(OutputFormat::Opus).selection, "251");
        assert_eq!(by(OutputFormat::Opus).audio_action, AudioAction::Copy);
        let mp3 = by(OutputFormat::Mp3);
        assert_eq!(mp3.selection, "251", "the best audio is re-encoded");
        assert_eq!(
            mp3.audio_action,
            AudioAction::Encode {
                codec: AudioCodec::Mp3,
                kbps: Some(192)
            }
        );
        assert_eq!(mp3.fit.size, Some(192 * 8_000 / 8));
        assert!(!by(OutputFormat::Flac).fit.lossless_source);
        assert_eq!(by(OutputFormat::Wav).fit.audio, StreamAction::Encode);
        assert!(variants.iter().all(|v| !v.fit.embeds_subtitles));
        assert!(variants.iter().all(|v| v.fit.video == StreamAction::None));
    }

    #[test]
    fn re_encoding_aims_near_the_source_rate() {
        use VideoCodec as V;
        assert_eq!(video_kbps(V::H264, V::H264, Some(5000.0), Some(1080)), 5000);
        assert_eq!(video_kbps(V::Av1, V::H264, Some(2000.0), Some(1080)), 3600);
        assert_eq!(video_kbps(V::H264, V::Vp9, Some(1400.0), None), 1000);
        // Unknown rate: a usual one for the height.
        assert_eq!(video_kbps(V::Vp9, V::H264, None, Some(1080)), 8000);
        assert_eq!(video_kbps(V::Vp9, V::H264, Some(f64::NAN), Some(240)), 500);
        // Never absurdly low.
        assert_eq!(video_kbps(V::H264, V::H264, Some(10.0), None), 300);
        // Never absurdly high, even beyond what a whole number holds.
        assert_eq!(video_kbps(V::H264, V::H264, Some(1e20), None), 80_000);
        assert_eq!(video_kbps(V::Av1, V::H264, Some(f64::MAX), None), 80_000);
    }

    /// Hardware first in a fixed order, then software; a hardware encoder that
    /// failed its test is skipped; libx264 is preferred to `OpenH264` when the
    /// build has it; an ffmpeg that cannot be asked gets the add-on's software
    /// encoder only.
    #[test]
    fn encoders_fall_back_from_hardware_to_software() {
        let names = |list: Vec<&Encoder>| list.iter().map(|e| e.name).collect::<Vec<_>>();
        let windows = encoders_for(VideoCodec::H264, "windows");
        assert_eq!(
            names(windows.clone()),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_mf",
                "libx264",
                "libopenh264"
            ]
        );
        let linux = encoders_for(VideoCodec::H264, "linux");
        assert_eq!(
            names(linux.clone()),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_vaapi",
                "libx264",
                "libopenh264"
            ]
        );
        // The media add-on's LGPL build: no libx264.
        let lgpl: HashSet<String> = [
            "h264_nvenc",
            "h264_qsv",
            "h264_amf",
            "h264_mf",
            "libopenh264",
            "aac",
        ]
        .map(String::from)
        .into();
        let none = HashSet::new();
        assert_eq!(
            names(encoder_order(&windows, Some(&lgpl), &none)),
            [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_mf",
                "libopenh264"
            ]
        );
        let failed: HashSet<&str> = ["h264_nvenc", "h264_qsv"].into();
        assert_eq!(
            names(encoder_order(&windows, Some(&lgpl), &failed)),
            ["h264_amf", "h264_mf", "libopenh264"]
        );
        let gpl: HashSet<String> = ["libx264", "libopenh264"].map(String::from).into();
        assert_eq!(
            names(encoder_order(&linux, Some(&gpl), &none)),
            ["libx264", "libopenh264"]
        );
        assert_eq!(names(encoder_order(&linux, None, &none)), ["libopenh264"]);
        let vp9 = encoders_for(VideoCodec::Vp9, "windows");
        assert_eq!(names(encoder_order(&vp9, None, &none)), ["libvpx-vp9"]);
        assert_eq!(
            names(encoders_for(VideoCodec::Vp9, "linux")),
            ["vp9_qsv", "vp9_vaapi", "libvpx-vp9"]
        );
        // Audio encoders fall back too.
        let lame_less: HashSet<String> = ["mp3_mf"].map(String::from).into();
        assert_eq!(audio_encoder(AudioCodec::Mp3, Some(&lame_less)).0, "mp3_mf");
        assert_eq!(audio_encoder(AudioCodec::Mp3, None).0, "libmp3lame");
        assert_eq!(audio_encoder(AudioCodec::Aac, Some(&lgpl)).0, "aac");
    }

    #[test]
    fn the_encoder_list_is_read_from_ffmpeg() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libopenh264          OpenH264 H.264\n V..... h264_qsv             H.264 (Intel)\n A....D aac                  AAC\n S..... mov_text             3GPP\n";
        let listed = listed_encoders(text);
        for name in ["libopenh264", "h264_qsv", "aac", "mov_text"] {
            assert!(listed.contains(name), "{name}");
        }
        assert!(!listed.contains("="));
        assert!(!listed.contains("Video"));
    }

    fn info(json: serde_json::Value) -> Info {
        serde_json::from_value(json).unwrap()
    }

    /// Human-written subtitles first, then the auto-generated ones in the
    /// video's own language only; live chat and unreadable formats are left out.
    #[test]
    fn subtitles_list_human_ones_first_and_skip_translations() {
        let info = info(serde_json::json!({
            "subtitles": {
                "fr": [{"ext": "vtt", "url": "https://s/fr.vtt", "name": "French"}],
                "en-GB": [{"ext": "json3", "url": "https://s/x"}, {"ext": "vtt", "url": "https://s/en.vtt", "name": "English (United Kingdom)"}],
                "live_chat": [{"ext": "json", "url": "https://s/chat"}],
                "de": [{"ext": "json3", "url": "https://s/de"}],
            },
            "automatic_captions": {
                "en-orig": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt", "name": "English (Original)"}],
                "en": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt&tlang=en", "name": "English"}],
                "es": [{"ext": "vtt", "url": "https://s/tt?v=1&fmt=vtt&tlang=es", "name": "Spanish"}],
            }
        }));
        let list = subtitles(&info);
        let shown: Vec<(&str, &str, bool)> = list
            .iter()
            .map(|(s, _)| (s.id.as_str(), s.name.as_str(), s.auto))
            .collect();
        assert_eq!(
            shown,
            [
                ("sub:en-GB", "English (United Kingdom)", false),
                ("sub:fr", "French", false),
                ("auto:en-orig", "English", true),
            ]
        );
        assert_eq!(list[2].0.language, "en");
        assert_eq!(list[2].1.key, "en-orig");
        assert_eq!(
            subtitle_files(&info, &list[0].1).unwrap().url.as_deref(),
            Some("https://s/en.vtt")
        );
    }

    /// yt-dlp sometimes says `null` or something odd: the page is still read.
    #[test]
    fn odd_subtitle_and_chapter_values_are_ignored() {
        let info = info(serde_json::json!({
            "title": "T", "subtitles": null, "automatic_captions": "nope",
            "chapters": [{"start_time": "x"}, 7], "duration": "long", "thumbnail": 5,
        }));
        assert!(info.subtitles.is_empty() && info.automatic_captions.is_empty());
        assert!(info.chapters.is_empty());
        assert_eq!(info.duration, None);
        assert_eq!(info.thumbnail, None);
        assert_eq!(info.title.as_deref(), Some("T"));
    }

    #[test]
    fn last_subtitle_choices_are_remembered() {
        let s = |id: &str, language: &str, auto: bool| MediaSubtitle {
            id: id.into(),
            language: language.into(),
            name: language.into(),
            auto,
        };
        let offered = [
            s("sub:en-US", "en-US", false),
            s("sub:fr", "fr", false),
            s("auto:de-orig", "de", true),
        ];
        // English and the German auto-generated ones were ticked.
        let (languages, auto) =
            remembered_subtitles(&offered, &["sub:en-US".into(), "auto:de-orig".into()]).unwrap();
        assert_eq!(languages, ["en-us", "de"]);
        assert_eq!(auto, Some(true));
        // Nothing offered: the last choice stays.
        assert_eq!(remembered_subtitles(&[], &[]), None);
        // Auto-generated ones offered and none ticked: not wanted any more.
        assert_eq!(
            remembered_subtitles(&offered, &["sub:fr".into()]),
            Some((vec!["fr".to_string()], Some(false)))
        );
        // Only human-written ones offered: whether auto-generated ones are
        // wanted stays as it was.
        assert_eq!(
            remembered_subtitles(&offered[..2], &[]),
            Some((Vec::new(), None))
        );
    }

    #[test]
    fn metadata_files_escape_what_the_site_wrote() {
        let chapters: Vec<Chapter> = serde_json::from_value(serde_json::json!([
            {"start_time": 0, "end_time": 61.5, "title": "Intro; part=1 #a\\b"},
            {"start_time": 61.5, "end_time": 120, "title": "Line\nbreak"},
            {"start_time": 100, "end_time": 90, "title": "Backwards"},
            {"start_time": 130, "end_time": 500, "title": "Past the end"},
        ]))
        .unwrap();
        let text = ffmetadata(Some("A = B"), Some("Me\r\n"), &chapters, Some(150_000));
        assert_eq!(
            text,
            ";FFMETADATA1\ntitle=A \\= B\nartist=Me\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=61500\ntitle=Intro\\; part\\=1 \\#a\\\\b\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=61500\nEND=120000\ntitle=Linebreak\n\
             [CHAPTER]\nTIMEBASE=1/1000\nSTART=130000\nEND=150000\ntitle=Past the end\n"
        );
        assert!(has_chapters(&chapters));
        assert!(!has_chapters(&[]));
    }

    #[test]
    fn languages_get_three_letter_codes_for_mp4() {
        assert_eq!(iso639_2("en"), "eng");
        assert_eq!(iso639_2("pt-BR"), "por");
        assert_eq!(iso639_2("fil"), "fil");
        assert_eq!(iso639_2("zz-top"), "und");
    }

    #[test]
    fn subtitle_and_picture_files_are_recognised_by_their_content() {
        assert_eq!(
            SubtitleFormat::detect(b"\xEF\xBB\xBFWEBVTT\n\n00:00", "vtt"),
            Some(SubtitleFormat::WebVtt)
        );
        assert_eq!(
            SubtitleFormat::detect(b"\n1\n00:00:01,000 --> 00:00:02,000\nHi", "srt"),
            Some(SubtitleFormat::Srt)
        );
        assert_eq!(
            SubtitleFormat::detect(b"[Script Info]\nTitle: x", "ass"),
            Some(SubtitleFormat::Ass)
        );
        // A playlist or anything else named like subtitles is refused.
        assert_eq!(SubtitleFormat::detect(b"#EXTM3U\n", "vtt"), None);
        assert_eq!(SubtitleFormat::detect(b"WEBVTT", "srt"), None);
        assert_eq!(SubtitleFormat::detect(b"", "vtt"), None);
        assert_eq!(
            ImageFormat::detect(b"\xFF\xD8\xFF\xE0"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::detect(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(
            ImageFormat::detect(b"\x89PNG\r\n\x1a\n"),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::detect(b"<svg"), None);
    }

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

    fn job(container: Container, video: VideoAction, audio: AudioAction) -> OutputJob {
        OutputJob {
            container,
            video,
            audio,
            subtitles: Vec::new(),
            subtitle_files: false,
            extras: Extras::default(),
            duration_ms: None,
            original_ext: "webm".into(),
            original: false,
        }
    }

    #[test]
    fn the_original_format_runs_ffmpeg_only_when_something_goes_inside() {
        let mut j = job(Container::Webm, VideoAction::Copy, AudioAction::Copy);
        j.original = true;
        assert!(!j.needs_pass());
        j.subtitles = vec![pick("en", false)];
        assert!(j.needs_pass(), "subtitles go inside a WebM");
        j.subtitle_files = true;
        assert!(!j.needs_pass(), "subtitles go next to it");
        j.extras.chapters = true;
        assert!(j.needs_pass());
        j.original = false;
        j.extras.chapters = false;
        assert!(j.needs_pass(), "another format always runs ffmpeg");
    }

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn pick(language: &str, auto: bool) -> SubtitlePick {
        SubtitlePick {
            key: language.into(),
            auto,
            language: language.into(),
            name: "English".into(),
        }
    }

    /// Every input, subtitles and the metadata file included, is read as its
    /// own format from a local file only, and the output is one `file:` path.
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
        let args = text(&arguments(
            &j,
            &inputs,
            None,
            ("aac", &[]),
            Path::new("/w/output.mkv"),
        ));
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
        let joined = text(&arguments(
            &j,
            &inputs,
            Some(encoder),
            ("aac", &[]),
            Path::new("/w/out.mp4"),
        ))
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
        let joined = text(&arguments(
            &j,
            &inputs,
            None,
            ("libmp3lame", &[]),
            Path::new("/w/out.mp3"),
        ))
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
        let joined = text(&srt_arguments(
            SubtitleFormat::WebVtt,
            Path::new("/w/sub.vtt"),
            Path::new("/w/sub.srt"),
        ))
        .join(" ");
        assert!(
            joined.contains("-protocol_whitelist file -format_whitelist webvtt -f webvtt -i file:"),
            "{joined}"
        );
        assert!(joined.contains("-c:s srt -f srt file:"));
    }

    #[test]
    fn probing_encodes_a_generated_clip_to_nowhere() {
        let nvenc = &H264_ENCODERS[0];
        let joined = text(&probe_arguments(nvenc)).join(" ");
        assert!(joined.contains("-f lavfi -i testsrc2="), "{joined}");
        assert!(joined.contains("-c:v h264_nvenc -preset p5"));
        assert!(joined.ends_with("-f null -"));
        let vaapi = H264_ENCODERS
            .iter()
            .find(|e| e.name == "h264_vaapi")
            .unwrap();
        assert!(text(&probe_arguments(vaapi)).join(" ").starts_with(
            "-hide_banner -nostdin -loglevel error -vaapi_device /dev/dri/renderD128 -f lavfi"
        ));
    }
}
