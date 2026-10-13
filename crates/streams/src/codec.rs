//! The one codec and container policy for every video and audio Wirecatch saves, whether it came from an HLS or DASH stream (this crate) or from a page read by yt-dlp (`wirecatch-core`), so the same video gets the same container whichever way it was found.
//!
//! The enums are deliberately not `#[non_exhaustive]`: a new codec or container must make every `match` in the workspace decide what it means.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// A video codec, as far as choosing a container goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    /// H.264 (AVC).
    H264,
    /// H.265 (HEVC), Dolby Vision included.
    H265,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// AV1.
    Av1,
    /// Any other video codec, such as MPEG-4 Part 2 or Theora.
    Other,
}

impl VideoCodec {
    /// The video codec a name stands for, as an HLS `CODECS` or DASH `codecs` attribute writes it (RFC 6381, for example `avc1.640028`) or as yt-dlp's `vcodec` does (`h264`, `vp9`); `None` when the name is not one of a video codec.
    #[must_use]
    pub fn from_name(name: &str) -> Option<VideoCodec> {
        let name = name.trim().to_ascii_lowercase();
        let starts = |prefixes: &[&str]| prefixes.iter().any(|p| name.starts_with(p));
        Some(if starts(&["avc", "h264"]) {
            VideoCodec::H264
        } else if starts(&["hev", "hvc", "h265", "dvh"]) {
            VideoCodec::H265
        } else if starts(&["vp8", "vp08"]) {
            VideoCodec::Vp8
        } else if starts(&["vp9", "vp09"]) {
            VideoCodec::Vp9
        } else if starts(&["av01", "av1"]) {
            VideoCodec::Av1
        } else if starts(&["mp4v", "theora"]) {
            VideoCodec::Other
        } else {
            return None;
        })
    }

    /// The video codec a file with extension `ext` (without the dot, any case) most likely holds, for a file whose codec is not named: H.264 in MP4, VP9 in WebM.
    #[must_use]
    pub fn usual_in(ext: &str) -> VideoCodec {
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => VideoCodec::H264,
            "webm" => VideoCodec::Vp9,
            _ => VideoCodec::Other,
        }
    }

    /// The codec in plain words.
    #[must_use]
    pub fn label(self) -> &'static str {
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
pub enum AudioCodec {
    /// AAC.
    Aac,
    /// MPEG audio layer 3.
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// FLAC.
    Flac,
    /// Apple Lossless.
    Alac,
    /// Dolby Digital.
    Ac3,
    /// Dolby Digital Plus.
    Eac3,
    /// Uncompressed PCM.
    Pcm,
    /// Any other audio codec, such as Dolby AC-4.
    Other,
}

/// The RFC 6381 names of codecs other than AAC carried in MP4 as `mp4a` with their object type, which would otherwise read as AAC: MP3, AC-3, E-AC-3, Opus and the DTS family.
const NOT_AAC_IN_MP4: [(&str, AudioCodec); 9] = [
    ("mp4a.40.34", AudioCodec::Mp3),
    ("mp4a.6b", AudioCodec::Mp3),
    ("mp4a.69", AudioCodec::Mp3),
    ("mp4a.a5", AudioCodec::Ac3),
    ("mp4a.a6", AudioCodec::Eac3),
    ("mp4a.ad", AudioCodec::Opus),
    ("mp4a.a9", AudioCodec::Other),
    ("mp4a.aa", AudioCodec::Other),
    ("mp4a.ac", AudioCodec::Other),
];

impl AudioCodec {
    /// The audio codec a name stands for, as an RFC 6381 codecs attribute (`mp4a.40.2`, `ec-3`) or yt-dlp's `acodec` (`opus`, `aac`) writes it; `None` when the name is not one of an audio codec.
    #[must_use]
    pub fn from_name(name: &str) -> Option<AudioCodec> {
        let name = name.trim().to_ascii_lowercase();
        if let Some((_, codec)) = NOT_AAC_IN_MP4.iter().find(|(n, _)| *n == name) {
            return Some(*codec);
        }
        let starts = |prefixes: &[&str]| prefixes.iter().any(|p| name.starts_with(p));
        Some(if starts(&["mp4a", "aac"]) {
            AudioCodec::Aac
        } else if starts(&["mp3"]) {
            AudioCodec::Mp3
        } else if starts(&["opus"]) {
            AudioCodec::Opus
        } else if starts(&["vorbis"]) {
            AudioCodec::Vorbis
        } else if starts(&["flac"]) {
            AudioCodec::Flac
        } else if starts(&["alac"]) {
            AudioCodec::Alac
        } else if starts(&["ac-3", "ac3"]) {
            AudioCodec::Ac3
        } else if starts(&["ec-3", "eac3"]) {
            AudioCodec::Eac3
        } else if starts(&["pcm"]) || name == "wav" {
            AudioCodec::Pcm
        } else if starts(&["ac-4"]) {
            AudioCodec::Other
        } else {
            return None;
        })
    }

    /// The audio codec a file with extension `ext` (without the dot, any case) most likely holds, for a file whose codec is not named.
    #[must_use]
    pub fn usual_in(ext: &str) -> AudioCodec {
        match ext.to_ascii_lowercase().as_str() {
            "m4a" | "mp4" | "aac" => AudioCodec::Aac,
            "webm" | "opus" => AudioCodec::Opus,
            "mp3" => AudioCodec::Mp3,
            "ogg" => AudioCodec::Vorbis,
            "flac" => AudioCodec::Flac,
            "wav" => AudioCodec::Pcm,
            _ => AudioCodec::Other,
        }
    }

    /// The codec in plain words.
    #[must_use]
    pub fn label(self) -> &'static str {
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
    #[must_use]
    pub fn lossless(self) -> bool {
        matches!(self, AudioCodec::Flac | AudioCodec::Alac | AudioCodec::Pcm)
    }
}

/// One codec a track names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    /// A video codec.
    Video(VideoCodec),
    /// An audio codec.
    Audio(AudioCodec),
}

impl Codec {
    /// The codec a name stands for (see [`VideoCodec::from_name`] and [`AudioCodec::from_name`]); `None` for a name that is neither, such as a subtitle format (`stpp`, `wvtt`) or one this policy does not know.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Codec> {
        VideoCodec::from_name(name)
            .map(Codec::Video)
            .or_else(|| AudioCodec::from_name(name).map(Codec::Audio))
    }

    /// Every codec of a comma-separated list such as HLS `CODECS="avc1.64001f,mp4a.40.2"`, each `None` when it is not known; empty entries are left out.
    #[must_use]
    pub fn list(names: &str) -> Vec<Option<Codec>> {
        names
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(Codec::from_name)
            .collect()
    }

    /// Whether every name in the comma-separated `names` is an audio codec, and there is at least one.
    #[must_use]
    pub fn all_audio(names: &str) -> bool {
        let list = Codec::list(names);
        !list.is_empty() && list.iter().all(|c| matches!(c, Some(Codec::Audio(_))))
    }

    /// Whether any name in the comma-separated `names` is a video codec.
    #[must_use]
    pub fn any_video(names: &str) -> bool {
        Codec::list(names)
            .iter()
            .any(|c| matches!(c, Some(Codec::Video(_))))
    }
}

/// The file a video or audio is saved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    /// MP4 video.
    Mp4,
    /// Matroska video.
    Mkv,
    /// WebM video.
    Webm,
    /// MP3 audio.
    Mp3,
    /// MP4 audio.
    M4a,
    /// Matroska audio.
    Mka,
    /// Opus audio in Ogg.
    Opus,
    /// FLAC audio.
    Flac,
    /// WAV audio.
    Wav,
}

impl Container {
    /// The container a file with extension `ext` (any case) is, when it is one Wirecatch writes.
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Container> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "mp4" => Container::Mp4,
            "mkv" => Container::Mkv,
            "webm" => Container::Webm,
            "mp3" => Container::Mp3,
            "m4a" => Container::M4a,
            "mka" => Container::Mka,
            "opus" => Container::Opus,
            "flac" => Container::Flac,
            "wav" => Container::Wav,
            _ => return None,
        })
    }

    /// The file extension, without the dot.
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::Webm => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "m4a",
            Container::Mka => "mka",
            Container::Opus => "opus",
            Container::Flac => "flac",
            Container::Wav => "wav",
        }
    }

    /// The format's name for a person, for example "MP4".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::Mkv => "MKV",
            Container::Webm => "WebM",
            Container::Mp3 => "MP3",
            Container::M4a => "M4A",
            Container::Mka => "MKA",
            Container::Opus => "Opus",
            Container::Flac => "FLAC",
            Container::Wav => "WAV",
        }
    }

    /// ffmpeg's muxer for it, always named so the extension never decides.
    #[must_use]
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv | Container::Mka => "matroska",
            Container::Webm => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "ipod",
            Container::Opus => "opus",
            Container::Flac => "flac",
            Container::Wav => "wav",
        }
    }

    /// Whether it holds video at all.
    #[must_use]
    pub fn is_video(self) -> bool {
        matches!(self, Container::Mp4 | Container::Mkv | Container::Webm)
    }

    /// The same kind of file for audio alone: M4A for MP4, MKA for Matroska.
    #[must_use]
    pub fn for_audio_only(self) -> Container {
        match self {
            Container::Mp4 => Container::M4a,
            Container::Mkv => Container::Mka,
            other => other,
        }
    }

    /// Whether a video in `codec` goes in as is. MP4 takes H.264 and H.265 only: VP9 and AV1 in MP4 are valid but many players and editors refuse them, and MP4 is chosen for those players (D24).
    #[must_use]
    pub fn holds_video(self, codec: VideoCodec) -> bool {
        match self {
            Container::Mkv => true,
            Container::Mp4 => matches!(codec, VideoCodec::H264 | VideoCodec::H265),
            Container::Webm => matches!(codec, VideoCodec::Vp8 | VideoCodec::Vp9 | VideoCodec::Av1),
            _ => false,
        }
    }

    /// Whether audio in `codec` goes in as is.
    #[must_use]
    pub fn holds_audio(self, codec: AudioCodec) -> bool {
        match self {
            Container::Mkv | Container::Mka => true,
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

    /// Whether `codec` goes in as is; an unknown codec (`None`) fits only Matroska.
    #[must_use]
    pub fn holds(self, codec: Option<Codec>) -> bool {
        match codec {
            Some(Codec::Video(c)) => self.holds_video(c),
            Some(Codec::Audio(c)) => self.holds_audio(c),
            None => matches!(self, Container::Mkv | Container::Mka),
        }
    }

    /// The container tracks with `codecs` are copied into, never re-encoded: MP4 (M4A for `audio_only`) when every codec fits it, else Matroska (MKA). Tracks that name no codecs at all try MP4; [`Container::fallback`] is the container to try when that fails.
    #[must_use]
    pub fn for_copy(
        codecs: impl IntoIterator<Item = Option<Codec>>,
        audio_only: bool,
    ) -> Container {
        let mut codecs = codecs.into_iter();
        let (mp4, matroska) = if audio_only {
            (Container::M4a, Container::Mka)
        } else {
            (Container::Mp4, Container::Mkv)
        };
        if codecs.all(|c| mp4.holds(c)) {
            mp4
        } else {
            matroska
        }
    }

    /// The container to copy into when this one refused the streams: Matroska, which takes any codec; `None` for Matroska itself.
    #[must_use]
    pub fn fallback(self) -> Option<Container> {
        match self {
            Container::Mkv | Container::Mka => None,
            other if other.is_video() => Some(Container::Mkv),
            _ => Some(Container::Mka),
        }
    }

    /// Whether ffmpeg moves the index to the front (`-movflags +faststart`), so a player can start before it has read the whole file.
    #[must_use]
    pub fn faststart(self) -> bool {
        matches!(self, Container::Mp4 | Container::M4a)
    }

    /// The video codec a video is re-encoded to when it does not fit.
    #[must_use]
    pub fn video_target(self) -> VideoCodec {
        match self {
            Container::Webm => VideoCodec::Vp9,
            _ => VideoCodec::H264,
        }
    }

    /// The audio codec audio is re-encoded to when it does not fit.
    #[must_use]
    pub fn audio_target(self) -> AudioCodec {
        match self {
            Container::Mp4 | Container::Mkv | Container::M4a | Container::Mka => AudioCodec::Aac,
            Container::Webm | Container::Opus => AudioCodec::Opus,
            Container::Mp3 => AudioCodec::Mp3,
            Container::Flac => AudioCodec::Flac,
            Container::Wav => AudioCodec::Pcm,
        }
    }

    /// ffmpeg's encoder for subtitles inside it; `None` when subtitles are saved as files next to it.
    #[must_use]
    pub fn subtitle_codec(self) -> Option<&'static str> {
        match self {
            Container::Mp4 => Some("mov_text"),
            Container::Mkv => Some("srt"),
            Container::Webm => Some("webvtt"),
            _ => None,
        }
    }

    /// Whether it can hold chapters.
    #[must_use]
    pub fn chapters(self) -> bool {
        matches!(
            self,
            Container::Mp4 | Container::Mkv | Container::Webm | Container::M4a | Container::Mka
        )
    }

    /// Whether it can hold a cover picture.
    #[must_use]
    pub fn cover(self) -> bool {
        matches!(self, Container::Mp3 | Container::M4a | Container::Flac)
    }

    /// Whether title and artist tags are written into it.
    #[must_use]
    pub fn tags(self) -> bool {
        matches!(
            self,
            Container::Mp3 | Container::M4a | Container::Flac | Container::Opus
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_names_are_read_from_rfc_6381_and_yt_dlp() {
        assert_eq!(VideoCodec::from_name("avc1.640028"), Some(VideoCodec::H264));
        assert_eq!(VideoCodec::from_name("AVC3.4d401f"), Some(VideoCodec::H264));
        assert_eq!(
            VideoCodec::from_name("hev1.1.6.L93.B0"),
            Some(VideoCodec::H265)
        );
        assert_eq!(VideoCodec::from_name("dvh1.05.06"), Some(VideoCodec::H265));
        assert_eq!(
            VideoCodec::from_name("vp09.00.40.08"),
            Some(VideoCodec::Vp9)
        );
        assert_eq!(VideoCodec::from_name("vp9"), Some(VideoCodec::Vp9));
        assert_eq!(
            VideoCodec::from_name("av01.0.08M.08"),
            Some(VideoCodec::Av1)
        );
        assert_eq!(VideoCodec::from_name("mp4v.20.9"), Some(VideoCodec::Other));
        assert_eq!(VideoCodec::from_name("mp4a.40.2"), None);
        assert_eq!(AudioCodec::from_name("mp4a.40.2"), Some(AudioCodec::Aac));
        assert_eq!(AudioCodec::from_name("mp4a.40.34"), Some(AudioCodec::Mp3));
        assert_eq!(AudioCodec::from_name("ec-3"), Some(AudioCodec::Eac3));
        // `mp4a` with the object types of AC-3, E-AC-3, Opus and DTS is none of them AAC.
        assert_eq!(AudioCodec::from_name("mp4a.a5"), Some(AudioCodec::Ac3));
        assert_eq!(AudioCodec::from_name("MP4A.A6"), Some(AudioCodec::Eac3));
        assert_eq!(AudioCodec::from_name("mp4a.ad"), Some(AudioCodec::Opus));
        assert_eq!(AudioCodec::from_name("mp4a.a9"), Some(AudioCodec::Other));
        assert_eq!(
            AudioCodec::from_name("ac-4.02.01.01"),
            Some(AudioCodec::Other)
        );
        assert_eq!(AudioCodec::from_name("avc1"), None);
        assert_eq!(Codec::from_name("stpp.ttml.im1t"), None);
        assert!(Codec::all_audio("mp4a.40.2, ec-3"));
        assert!(!Codec::all_audio("mp4a.40.2,avc1.4d401e"));
        assert!(!Codec::all_audio(""));
        assert!(Codec::any_video("mp4a.40.2,avc1.4d401e"));
    }

    #[test]
    fn mp4_takes_only_what_its_players_play() {
        let copy = |codecs: &str, audio_only| Container::for_copy(Codec::list(codecs), audio_only);
        assert_eq!(copy("avc1.64001f,mp4a.40.2", false), Container::Mp4);
        assert_eq!(copy("hvc1.1.6.L93.B0,ec-3", false), Container::Mp4);
        // VP9 and AV1 in MP4 are valid, but many players refuse them (D24).
        assert_eq!(copy("vp09.00.40.08,mp4a.40.2", false), Container::Mkv);
        assert_eq!(copy("av01.0.08M.08", false), Container::Mkv);
        assert_eq!(copy("avc1.64001f,something", false), Container::Mkv);
        assert_eq!(copy("mp4a.40.2", true), Container::M4a);
        assert_eq!(copy("opus", true), Container::Mka);
        assert_eq!(Container::for_copy([], false), Container::Mp4);
        assert_eq!(Container::Mp4.fallback(), Some(Container::Mkv));
        assert_eq!(Container::M4a.fallback(), Some(Container::Mka));
        assert_eq!(Container::Mkv.fallback(), None);
        assert_eq!(Container::Mp4.for_audio_only(), Container::M4a);
    }

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

    #[test]
    fn containers_are_named_by_their_extension_in_any_case() {
        for container in [
            Container::Mp4,
            Container::Mkv,
            Container::Webm,
            Container::Mp3,
            Container::M4a,
            Container::Mka,
            Container::Opus,
            Container::Flac,
            Container::Wav,
        ] {
            assert_eq!(Container::from_ext(container.ext()), Some(container));
            assert_eq!(
                Container::from_ext(&container.ext().to_ascii_uppercase()),
                Some(container)
            );
        }
        assert_eq!(Container::from_ext("exe"), None);
    }
}
