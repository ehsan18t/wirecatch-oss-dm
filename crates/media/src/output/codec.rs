//! The codecs of yt-dlp's formats, read into the shared codec policy ([`wirecatch_streams::codec`]), and the audio qualities each codec is encoded at.

#![forbid(unsafe_code)]

use wirecatch_protocol::app::OutputFormat;

use super::{AudioCodec, VideoCodec};
use crate::ytdlp::Format;

/// The video codec of a yt-dlp format, from its `vcodec` (for example `avc1.640028`) or, when it gave none, from its extension; `None` when it has no video.
pub(crate) fn video_codec(format: &Format) -> Option<VideoCodec> {
    if !format.has_video() {
        return None;
    }
    let named = format.vcodec.as_deref().filter(|c| !c.trim().is_empty());
    Some(match named {
        Some(name) => VideoCodec::from_name(name).unwrap_or(VideoCodec::Other),
        // yt-dlp did not say: the container is the best guess.
        None => VideoCodec::usual_in(format.ext.as_deref().unwrap_or_default()),
    })
}

/// The audio codec of a yt-dlp format, from its `acodec` (for example `mp4a.40.2`) or, when it gave none, from its extension; `None` when it has no audio.
pub(crate) fn audio_codec(format: &Format) -> Option<AudioCodec> {
    if !format.has_audio() {
        return None;
    }
    let named = format.acodec.as_deref().filter(|c| !c.trim().is_empty());
    Some(match named {
        Some(name) => AudioCodec::from_name(name).unwrap_or(AudioCodec::Other),
        None => AudioCodec::usual_in(format.ext.as_deref().unwrap_or_default()),
    })
}

/// The audio-only format `codec` is encoded into. Its bit rates ([`OutputFormat::kbps_choices`]) are the codec's wherever it is encoded, inside a video too; `None` for codecs Wirecatch never encodes lossy.
fn audio_format(codec: AudioCodec) -> Option<OutputFormat> {
    match codec {
        AudioCodec::Mp3 => Some(OutputFormat::Mp3),
        AudioCodec::Aac => Some(OutputFormat::M4a),
        AudioCodec::Opus => Some(OutputFormat::Opus),
        _ => None,
    }
}

/// The audio qualities offered when encoding to `codec`, in kbps; empty for lossless codecs.
pub(crate) fn kbps_choices(codec: AudioCodec) -> &'static [u32] {
    audio_format(codec).map_or(&[], OutputFormat::kbps_choices)
}

/// The usual quality when encoding to `codec`, in kbps.
pub(crate) fn default_kbps(codec: AudioCodec) -> Option<u32> {
    audio_format(codec).and_then(OutputFormat::default_kbps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::test_support::*;
    use crate::output::*;

    #[test]
    fn codecs_are_read_from_yt_dlp_names() {
        let v = |c: &str| {
            video_codec(&format(
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
            audio_codec(&format(
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
        assert_eq!(video_codec(&unnamed), Some(VideoCodec::Vp9));
        assert_eq!(audio_codec(&unnamed), Some(AudioCodec::Opus));
    }
}
