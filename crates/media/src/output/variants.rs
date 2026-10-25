//! The "Save as" choices of a quality (D24): which streams each format takes as they are, what it re-encodes, and at which bit rate.

#![forbid(unsafe_code)]

use wirecatch_engine::numeric::{f64_to_u64, saturating_cast};
use wirecatch_protocol::app::{OutputFit, OutputFormat, StreamAction};

use super::codec::{audio_codec, default_kbps, video_codec};
use super::{AudioAction, VideoAction};
use super::{AudioCodec, Container, VideoCodec};
use crate::ytdlp::{Format, FormatId};
use crate::{Content, is_media_ext, safe_ext};

/// The format and streams one "Save as" choice of one quality uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The format ids, as for [`MediaJob::Page`](crate::MediaJob::Page).
    pub video: Option<FormatId>,
    pub audio: Option<FormatId>,
    pub container: Container,
    pub video_action: VideoAction,
    pub audio_action: AudioAction,
    /// The audio codec downloaded, which decides whether an audio quality can be kept as is.
    pub audio_codec: Option<AudioCodec>,
    /// The extension the downloaded streams get when kept as they came ("Keep the original" after a failed conversion).
    pub kept_ext: String,
    pub fit: OutputFit,
}

/// The extension downloaded streams get when kept as they came: MP4 for MP4 video with M4A audio and WebM for WebM with WebM, when that container holds both codecs under the shared policy (D24: AV1 that came in MP4 does not stay in MP4); MKV for any other pair. A single file keeps its own (a media extension, never one a site chose otherwise).
pub(crate) fn kept_ext(video: Option<&Format>, audio: Option<&Format>) -> String {
    match (video, audio) {
        (Some(v), Some(a)) => {
            let (v_ext, a_ext) = (
                safe_ext(v.ext.as_deref(), Content::Video),
                safe_ext(a.ext.as_deref(), Content::Audio),
            );
            // Matroska holds anything, so it is the choice when either side named something that is not a media extension.
            let named = [&v.ext, &a.ext]
                .iter()
                .all(|e| e.as_deref().is_some_and(is_media_ext));
            let same = match (v_ext.as_str(), a_ext.as_str()) {
                ("mp4", "m4a" | "mp4") if named => Container::Mp4,
                ("webm", "webm") if named => Container::Webm,
                _ => Container::Mkv,
            };
            let holds = video_codec(v).is_some_and(|c| same.holds_video(c))
                && audio_codec(a).is_some_and(|c| same.holds_audio(c));
            if holds { same } else { Container::Mkv }.ext().to_string()
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

/// How much less bit rate AV1 needs than H.264 for the same picture (D24).
const AV1_EFFICIENCY: f64 = 1.8;
/// How much less bit rate H.265 and VP9 need than H.264 for the same picture (D24).
const HEVC_VP9_EFFICIENCY: f64 = 1.4;
/// The height assumed when a site gives none.
const DEFAULT_SOURCE_HEIGHT: u32 = 720;
/// A usual H.264 bit rate in kbps for a video up to each height, used when the site gives no rate.
const H264_KBPS_BY_HEIGHT: [(u32, f64); 6] = [
    (240, 500.0),
    (360, 1_000.0),
    (480, 2_000.0),
    (720, 4_000.0),
    (1080, 8_000.0),
    (1440, 16_000.0),
];
/// A usual H.264 bit rate in kbps above the last height of [`H264_KBPS_BY_HEIGHT`].
const H264_KBPS_ABOVE: f64 = 35_000.0;
/// The highest audio bit rate in kbps taken from yt-dlp as real; higher is a mistake in the page.
const MAX_AUDIO_KBPS: f64 = 100_000.0;
/// WAV as Wirecatch writes it: 16-bit stereo PCM at 48 kHz, in kbps.
const PCM_S16_STEREO_48K_KBPS: u32 = 1_536;

/// Video bit rate in kbps re-encoded to `target` from a video of `source` at `source_kbps` (and `height`): about the source's, more when the source codec is more efficient than the target, or a usual rate for the height when the site gave none.
pub(crate) fn video_kbps(
    source: VideoCodec,
    target: VideoCodec,
    source_kbps: Option<f64>,
    height: Option<u32>,
) -> u32 {
    let efficiency = |c: VideoCodec| match c {
        VideoCodec::Av1 => AV1_EFFICIENCY,
        VideoCodec::H265 | VideoCodec::Vp9 => HEVC_VP9_EFFICIENCY,
        _ => 1.0,
    };
    let known = source_kbps
        .filter(|k| k.is_finite() && *k > 0.0)
        .map(|k| k * efficiency(source) / efficiency(target));
    let kbps = known.unwrap_or_else(|| {
        let height = height.unwrap_or(DEFAULT_SOURCE_HEIGHT);
        let h264 = H264_KBPS_BY_HEIGHT
            .iter()
            .find(|(up_to, _)| height <= *up_to)
            .map_or(H264_KBPS_ABOVE, |(_, kbps)| *kbps);
        h264 / efficiency(target)
    });
    // `kbps` is positive here, so a rate that is no `u32` (or no `u64`, or infinite after the efficiency factor) is one too large.
    let whole = f64_to_u64(kbps.round()).map_or(u32::MAX, saturating_cast);
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
        .filter(|r| *r > 0.0 && *r < MAX_AUDIO_KBPS)
        .and_then(|r| f64_to_u64(r.round()))
        .and_then(|r| u32::try_from(r).ok())
}

/// A format's bit rate as yt-dlp reports it, 0 when unknown.
pub(crate) fn rate(format: &Format) -> f64 {
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

/// The best of `formats` for a row whose video runs at `fps`: the same frame rate first, then the highest bit rate.
fn best_for_fps<'a>(formats: impl Iterator<Item = &'a Format>, fps: f64) -> Option<&'a Format> {
    formats.max_by(|a, b| {
        let near = |f: &Format| (f.fps.unwrap_or(fps) - fps).abs() < 1.0;
        near(a).cmp(&near(b)).then(rate(a).total_cmp(&rate(b)))
    })
}

/// The format with the highest bit rate.
pub(crate) fn best_by_rate<'a>(formats: impl Iterator<Item = &'a Format>) -> Option<&'a Format> {
    formats.max_by(|a, b| rate(a).total_cmp(&rate(b)))
}

/// The "Save as" choices of one video quality, Original first: the original formats copied into their own container, then MP4, MKV and WebM. Each takes the streams of this height that fit the container as they are, and re-encodes only what has no such stream.
///
/// `at` holds the video formats of this height; `audios` every audio-only format; `original` the streams of the Original choice (video, and separate audio if any) in `original_ext`.
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
    // Original: the streams of the Original choice, in their own container.
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
        let fits = |f: &&Format| video_codec(f).is_some_and(|c| container.holds_video(c));
        let audio_fits = |f: &&Format| audio_codec(f).is_some_and(|c| container.holds_audio(c));
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
            let from = video_codec(source).unwrap_or(VideoCodec::Other);
            (
                source,
                VideoAction::Encode {
                    codec,
                    kbps: video_kbps(from, codec, video_rate(source), height),
                },
            )
        };
        // The video format's own audio, when it fits, else the best separate audio that fits, else the original's audio re-encoded.
        let own_audio = audio_codec(video).filter(|c| container.holds_audio(*c));
        let (audio, audio_action) = if container == Container::Mkv {
            (original.1, original_audio_action(original))
        } else if own_audio.is_some() && video_action == VideoAction::Copy {
            (None, AudioAction::Copy)
        } else if let Some(a) = best_by_rate(audios.iter().copied().filter(audio_fits)) {
            (Some(a), AudioAction::Copy)
        } else {
            let source = original.1.or_else(|| video.has_audio().then_some(video));
            let target = container.audio_target();
            match source {
                Some(a) => (
                    original.1.filter(|_| !std::ptr::eq(a, video)),
                    AudioAction::Encode {
                        codec: target,
                        kbps: default_kbps(target),
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
    let audio_codec = audio_source.and_then(audio_codec);
    let video_codec = match video_action {
        VideoAction::Encode { codec, .. } => Some(codec),
        _ => video_codec(video),
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
    Variant {
        video: Some(video.id.clone()),
        audio: audio.map(|a| a.id.clone()),
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
            video_codec: video_codec.map(|c| c.label().to_string()),
            audio_codec: audio_codec.map(|c| c.label().to_string()),
            audio_kbps: audio_source.and_then(audio_rate),
            lossless_source: audio_codec.is_some_and(AudioCodec::lossless),
            size: video_size.zip(audio_size).map(|(v, a)| v + a),
            embeds_subtitles: known.is_some_and(|c| c.subtitle_codec().is_some()),
            chapters: known.is_some_and(Container::chapters),
        },
    }
}

/// The "Save as" choices of the audio-only quality, Original first, then MP3, M4A, Opus, FLAC and WAV. A format whose codec the site offers copies that audio; the others re-encode the best audio there is.
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
    let best = best_by_rate(audios.iter().copied()).unwrap_or(original);
    for (format, container) in [
        (OutputFormat::Mp3, Container::Mp3),
        (OutputFormat::M4a, Container::M4a),
        (OutputFormat::Opus, Container::Opus),
        (OutputFormat::Flac, Container::Flac),
        (OutputFormat::Wav, Container::Wav),
    ] {
        let fitting = best_by_rate(
            audios
                .iter()
                .copied()
                .filter(|a| audio_codec(a).is_some_and(|c| container.holds_audio(c))),
        );
        let (source, action) = if let Some(a) = fitting {
            (a, AudioAction::Copy)
        } else {
            let target = container.audio_target();
            (
                best,
                AudioAction::Encode {
                    codec: target,
                    kbps: default_kbps(target),
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
            v.fit.size = bytes_for(PCM_S16_STEREO_48K_KBPS, duration_ms);
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
    let codec = audio_codec(source);
    let size = match action {
        AudioAction::Encode { kbps: Some(k), .. } => bytes_for(k, duration_ms),
        AudioAction::Encode { kbps: None, .. } => None,
        _ => source.size(),
    };
    Variant {
        video: None,
        audio: Some(source.id.clone()),
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
            audio_codec: codec.map(|c| c.label().to_string()),
            audio_kbps: audio_rate(source),
            lossless_source: codec.is_some_and(AudioCodec::lossless),
            size,
            embeds_subtitles: false,
            chapters: container == Container::M4a,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::test_support::*;
    use crate::output::*;
    use crate::ytdlp::ids;

    /// The format ids a variant downloads, as `video+audio`.
    fn sel(variant: &Variant) -> String {
        ids(variant.video.as_ref(), variant.audio.as_ref())
    }

    /// A YouTube-like 1080p row: VP9 is best, but MP4 takes the H.264 of the same height and frame rate and copies it.
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
        assert_eq!(sel(original), "303+251");
        assert_eq!(
            (original.fit.video, original.fit.audio),
            (StreamAction::Copy, StreamAction::Copy)
        );
        let mp4 = by(OutputFormat::Mp4);
        assert_eq!(sel(mp4), "299+140", "the 60 fps H.264 and the AAC");
        assert_eq!(mp4.video_action, VideoAction::Copy);
        assert_eq!(mp4.audio_action, AudioAction::Copy);
        assert_eq!(mp4.fit.video_codec.as_deref(), Some("H.264"));
        let webm = by(OutputFormat::Webm);
        assert_eq!(sel(webm), "303+251");
        assert_eq!(webm.fit.video, StreamAction::Copy);
        let mkv = by(OutputFormat::Mkv);
        assert_eq!(sel(mkv), "303+251");
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
        assert_eq!(sel(mp4), "313+140");
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
        assert_eq!(sel(mp4), "18");
        assert_eq!(
            (mp4.video_action, mp4.audio_action),
            (VideoAction::Copy, AudioAction::Copy)
        );
        let webm = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Webm)
            .unwrap();
        assert_eq!(sel(webm), "18");
        assert!(matches!(
            webm.audio_action,
            AudioAction::Encode {
                codec: AudioCodec::Opus,
                ..
            }
        ));
        assert_eq!(webm.fit.video, StreamAction::Encode);
    }

    /// Streams kept as they came are joined into the container the shared policy picks for their codecs, as streams are: AV1 that a site serves in MP4 goes into MKV, since MP4 is for H.264 and H.265 (D24).
    #[test]
    fn kept_streams_go_in_the_container_the_policy_picks_for_their_codecs() {
        let aac = listed("140", "m4a", ("none", "mp4a.40.2"), None, 0.0, 128.0);
        let opus = listed("251", "webm", ("none", "opus"), None, 0.0, 160.0);
        let av1 = listed(
            "399",
            "mp4",
            ("av01.0.08M.08", "none"),
            Some(1080),
            30.0,
            2000.0,
        );
        let avc = listed(
            "137",
            "mp4",
            ("avc1.640028", "none"),
            Some(1080),
            30.0,
            4500.0,
        );
        let vp9 = listed("303", "webm", ("vp9", "none"), Some(1080), 30.0, 4000.0);
        let unnamed = format(serde_json::json!({
            "format_id": "1", "ext": "mp4", "vcodec": null, "acodec": "none", "height": 720, "url": "u"
        }));
        let policy = |video: &Format, audio: &Format| {
            use wirecatch_streams::codec::Codec;
            let codecs = [
                video_codec(video).map(Codec::Video),
                audio_codec(audio).map(Codec::Audio),
            ];
            Container::for_copy(codecs, false)
        };

        assert_eq!(policy(&av1, &aac), Container::Mkv);
        assert_eq!(kept_ext(Some(&av1), Some(&aac)), "mkv");
        assert_eq!(policy(&avc, &aac), Container::Mp4);
        assert_eq!(kept_ext(Some(&avc), Some(&aac)), "mp4");
        assert_eq!(kept_ext(Some(&vp9), Some(&opus)), "webm");
        assert_eq!(
            kept_ext(Some(&unnamed), Some(&aac)),
            "mp4",
            "unnamed codecs: the extension decides"
        );

        let ext = kept_ext(Some(&av1), Some(&aac));
        let variants = video_variants(&[&av1], &[&aac], (&av1, Some(&aac)), &ext, Some(1080), None);
        let original = variants
            .iter()
            .find(|v| v.fit.format == OutputFormat::Original)
            .unwrap();
        assert_eq!(original.kept_ext, "mkv");
        assert_eq!(original.container, Container::Mkv);
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
        assert_eq!(sel(by(OutputFormat::Original)), "140");
        assert!(by(OutputFormat::Original).fit.chapters);
        assert_eq!(by(OutputFormat::M4a).audio_action, AudioAction::Copy);
        assert_eq!(by(OutputFormat::M4a).fit.audio_kbps, Some(130));
        assert_eq!(sel(by(OutputFormat::Opus)), "251");
        assert_eq!(by(OutputFormat::Opus).audio_action, AudioAction::Copy);
        let mp3 = by(OutputFormat::Mp3);
        assert_eq!(sel(mp3), "251", "the best audio is re-encoded");
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
}
