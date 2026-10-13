//! What a stream offers, and what to download from it.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use url::Url;
use wirecatch_engine::{DownloadOptions, PrivateNetwork};

use crate::codec::{Codec, Container};
use crate::extension;
use crate::ffmpeg::InputFormat;

/// The streaming format of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamKind {
    /// HTTP Live Streaming: `.m3u8` playlists.
    Hls,
    /// MPEG-DASH: `.mpd` manifests.
    Dash,
}

/// What [`inspect`](crate::inspect) found behind a link: the tracks to choose
/// from, how long the video is, and whether it is live.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StreamInfo {
    /// HLS or DASH.
    pub kind: StreamKind,
    /// The playlist or manifest address, after redirects.
    pub url: Url,
    /// Total length of the video, when the stream says.
    pub duration: Option<Duration>,
    /// Whether this is a live broadcast. Downloading a live broadcast is
    /// refused with [`Error::Live`](crate::Error::Live).
    pub live: bool,
    /// Video qualities, in the order the stream lists them.
    pub video: Vec<VideoTrack>,
    /// Audio that comes as its own stream (languages, qualities). Audio that is
    /// already inside a video track is not listed separately.
    pub audio: Vec<AudioTrack>,
    /// Subtitles the stream offers. They are listed for information only; this
    /// version does not download them.
    pub subtitles: Vec<SubtitleTrack>,
    /// What [`Choice::Auto`] picks, so the forecast follows the download.
    pub(crate) auto: AutoPicks,
}

/// What [`Choice::Auto`] picks in a stream, by track id, decided by the same code the download uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AutoPicks {
    /// The video picked automatically.
    pub video: Option<String>,
    /// The audio picked automatically with each video, by the video's id.
    pub audio_with: Vec<(String, Option<String>)>,
    /// The audio picked automatically without video.
    pub audio_alone: Option<String>,
}

/// One track's codecs and format, wherever it is listed.
struct Listed<'a> {
    codecs: Option<&'a str>,
    format: InputFormat,
}

impl StreamInfo {
    /// The extension [`download`](crate::download) will most likely give the
    /// saved file for these choices, with or without ffmpeg, so the caller can
    /// name the output right from the start: `mp4` or `mkv` (`m4a` or `mka` for
    /// audio only) with ffmpeg, or the stream's own format without it (such
    /// as `ts`).
    ///
    /// It is a forecast from what the playlist says, from the same picks the
    /// download makes. The download decides from what it actually receives, and
    /// never replaces another file when the two differ (see
    /// [`StreamRequest::output`]).
    #[must_use]
    pub fn extension(&self, video: &Choice, audio: &Choice, ffmpeg: bool) -> &'static str {
        let video_id = match video {
            Choice::Skip => None,
            Choice::Track(id) => Some(id.as_str()),
            Choice::Auto => self.auto.video.as_deref(),
        };
        let audio_id = match (audio, video_id) {
            (Choice::Skip, _) => None,
            (Choice::Track(id), _) => Some(id.as_str()),
            (Choice::Auto, Some(video)) => self
                .auto
                .audio_with
                .iter()
                .find(|(v, _)| v == video)
                .and_then(|(_, a)| a.as_deref()),
            (Choice::Auto, None) => self.auto.audio_alone.as_deref(),
        };
        let video = video_id.and_then(|id| self.listed(id));
        let audio = audio_id.and_then(|id| self.listed(id));
        if !ffmpeg {
            return video
                .or(audio)
                .map_or(FALLBACK_FORMAT, |t| t.format)
                .extension();
        }
        let codecs = [video, audio]
            .into_iter()
            .flatten()
            .filter_map(|t| t.codecs)
            .flat_map(Codec::list);
        Container::for_copy(codecs, video_id.is_none()).ext()
    }

    /// The track with `id`, among the videos and the audio (an audio-only HLS variant is picked as the video when there is no other).
    fn listed(&self, id: &str) -> Option<Listed<'_>> {
        self.video
            .iter()
            .find(|v| v.id == id)
            .map(|v| Listed {
                codecs: v.codecs.as_deref(),
                format: v.format,
            })
            .or_else(|| {
                self.audio.iter().find(|a| a.id == id).map(|a| Listed {
                    codecs: a.codecs.as_deref(),
                    format: a.format,
                })
            })
    }
}

/// The format a single track is saved as when nothing says what it is: MPEG-TS, the usual HLS segment format.
pub(crate) const FALLBACK_FORMAT: InputFormat = InputFormat::MpegTs;

/// One video quality.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VideoTrack {
    /// Pass this in [`Choice::Track`] to download this quality. It is only
    /// meaningful for the playlist or manifest it came from.
    pub id: String,
    /// Width in pixels, when the stream says.
    pub width: Option<u32>,
    /// Height in pixels, when the stream says.
    pub height: Option<u32>,
    /// Bits per second, when the stream says.
    pub bandwidth: Option<u64>,
    /// Codec names as the stream lists them, for example `avc1.64001f,mp4a.40.2`.
    pub codecs: Option<String>,
    /// Frames per second, when the stream says.
    pub frame_rate: Option<f64>,
    /// The format of this track's segments joined as they are.
    pub(crate) format: InputFormat,
}

/// One audio track.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AudioTrack {
    /// Pass this in [`Choice::Track`] to download this track. It is only
    /// meaningful for the playlist or manifest it came from.
    pub id: String,
    /// Language tag such as `en` or `pt-BR`, when the stream says.
    pub language: Option<String>,
    /// The name the stream gives the track, such as `English (Commentary)`.
    pub name: Option<String>,
    /// Bits per second, when the stream says.
    pub bandwidth: Option<u64>,
    /// Codec names as the stream lists them, for example `mp4a.40.2`.
    pub codecs: Option<String>,
    /// Channel layout as the stream states it, for example `2` or `6`.
    pub channels: Option<String>,
    /// Whether the stream marks this track as the one to play by default.
    pub default: bool,
    /// The format of this track's segments joined as they are.
    pub(crate) format: InputFormat,
}

/// One subtitle track (listed, not downloaded).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SubtitleTrack {
    /// An id for the track, unique within the stream.
    pub id: String,
    /// Language tag, when the stream says.
    pub language: Option<String>,
    /// The name the stream gives the track.
    pub name: Option<String>,
}

/// What a track is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Video, possibly with its own audio inside.
    Video,
    /// Audio alone.
    Audio,
}

impl Role {
    /// `video` or `audio`, for messages.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Role::Video => "video",
            Role::Audio => "audio",
        }
    }
}

/// Which video or audio track to download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Choice {
    /// Let Wirecatch pick: the highest quality video, and the audio the stream
    /// pairs with it (its default), if the audio comes separately.
    #[default]
    Auto,
    /// The track with this id from [`StreamInfo`].
    Track(String),
    /// Leave this kind out, for example `video: Choice::Skip` for audio only.
    Skip,
}

/// What to download and where to.
///
/// Create one with [`StreamRequest::new`] and change the fields you need.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StreamRequest {
    /// The playlist (`.m3u8`) or manifest (`.mpd`) address.
    pub url: Url,
    /// Where to save the video. Its extension is set to the container actually
    /// written (`.mp4`, `.mkv`, `.m4a`, `.mka`, or for a direct join without
    /// ffmpeg, whatever the stream carries, such as `.ts`);
    /// [`StreamInfo::extension`] tells it in advance. A known media extension
    /// is replaced, any other is kept and the new one added. An existing file
    /// at exactly this path is replaced, but when the extension changes and a
    /// file with the new name exists, it is left alone and a free name such as
    /// `video (1).mkv` is used. Temporary files go in a folder next to it.
    pub output: PathBuf,
    /// Which video to download (default [`Choice::Auto`]).
    pub video: Choice,
    /// Which audio to download (default [`Choice::Auto`]).
    pub audio: Choice,
    /// The ffmpeg program used to join tracks into one file, without ever
    /// re-encoding them. Without it, only a stream whose video and audio come
    /// together in one track can be saved, as the stream's own segments joined
    /// end to end.
    pub ffmpeg: Option<PathBuf>,
    /// Whether segments, keys and init sections may come from the local
    /// network (default [`PrivateNetwork::Auto`]: refused with
    /// [`Error::PrivateNetwork`](crate::Error::PrivateNetwork) when the
    /// playlist itself is on the internet, allowed when it is on the local
    /// network too, such as a media server at home).
    pub private_network: PrivateNetwork,
    /// Headers (cookies and the like, bound to the playlist's origin), user
    /// agent, timeouts, retries and speed limits, as for a download.
    /// [`DownloadOptions::connections`] is how many segments are downloaded at
    /// once (default 8).
    pub options: DownloadOptions,
}

impl StreamRequest {
    /// A request for the stream at `url`, saved at `output`, with automatic
    /// track choice and default options.
    pub fn new(url: Url, output: impl Into<PathBuf>) -> Self {
        Self {
            url,
            output: output.into(),
            video: Choice::Auto,
            audio: Choice::Auto,
            ffmpeg: None,
            private_network: PrivateNetwork::Auto,
            options: DownloadOptions::default(),
        }
    }
}

/// How a stream download that did not fail ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The video is saved and the temporary files are gone.
    #[non_exhaustive]
    Completed {
        /// Where the video was saved (with the extension that was chosen).
        path: PathBuf,
        /// Size of the saved file.
        bytes: u64,
    },
    /// Cancelled. Running the same request again continues: segments already
    /// downloaded are kept and not downloaded again.
    #[non_exhaustive]
    Paused {
        /// Segments still to download (0 when the joining step was cancelled).
        segments_left: usize,
    },
}

/// What a stream download is doing; see [`StreamProgress::stage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stage {
    /// Reading the playlists.
    Preparing,
    /// Downloading segments.
    Downloading,
    /// Joining the segments into the final file.
    Joining,
    /// Done, or stopped.
    Finished,
}

/// Live counters for one stream download, cheap to read from any task or
/// thread.
#[derive(Debug, Default)]
pub struct StreamProgress {
    bytes: AtomicU64,
    segments_done: AtomicUsize,
    segments_total: AtomicUsize,
    stage: AtomicU8,
}

impl StreamProgress {
    /// Bytes downloaded so far, including those kept from an earlier run.
    /// Speed is the change of this over time.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Segments finished so far, including those kept from an earlier run.
    pub fn segments_done(&self) -> usize {
        self.segments_done.load(Ordering::Relaxed)
    }

    /// How many segments the download has (0 until the playlists are read).
    pub fn segments_total(&self) -> usize {
        self.segments_total.load(Ordering::Relaxed)
    }

    /// The current stage.
    pub fn stage(&self) -> Stage {
        match self.stage.load(Ordering::Relaxed) {
            0 => Stage::Preparing,
            1 => Stage::Downloading,
            2 => Stage::Joining,
            _ => Stage::Finished,
        }
    }

    pub(crate) fn set_stage(&self, stage: Stage) {
        let value = match stage {
            Stage::Preparing => 0,
            Stage::Downloading => 1,
            Stage::Joining => 2,
            Stage::Finished => 3,
        };
        self.stage.store(value, Ordering::Relaxed);
    }

    pub(crate) fn start(&self, total: usize, done: usize, bytes: u64) {
        self.segments_total.store(total, Ordering::Relaxed);
        self.segments_done.store(done, Ordering::Relaxed);
        self.bytes.store(bytes, Ordering::Relaxed);
    }

    pub(crate) fn add_bytes(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn segment_done(&self) {
        self.segments_done.fetch_add(1, Ordering::Relaxed);
    }
}

/// `output` with its extension set to `ext`; see [`StreamRequest::output`].
pub(crate) fn with_extension(output: &Path, ext: &str) -> PathBuf {
    let known = output
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(extension::is_replaceable);
    if known {
        output.with_extension(ext)
    } else {
        let mut name = output.as_os_str().to_owned();
        name.push(".");
        name.push(ext);
        PathBuf::from(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_extension_is_replaced_only_when_it_is_a_media_one() {
        assert_eq!(
            with_extension(Path::new("/v/clip.mp4"), "mkv"),
            Path::new("/v/clip.mkv")
        );
        assert_eq!(
            with_extension(Path::new("/v/clip"), "mp4"),
            Path::new("/v/clip.mp4")
        );
        assert_eq!(
            with_extension(Path::new("/v/Episode 1.5"), "mp4"),
            Path::new("/v/Episode 1.5.mp4")
        );
        assert_eq!(
            with_extension(Path::new("/v/index.M3U8"), "ts"),
            Path::new("/v/index.ts")
        );
    }

    #[test]
    fn every_media_extension_core_names_is_replaced() {
        for (name, expected) in [
            ("/v/clip.flac", "/v/clip.m4a"),
            ("/v/clip.opus", "/v/clip.m4a"),
            ("/v/clip.ogg", "/v/clip.m4a"),
            ("/v/clip.wav", "/v/clip.m4a"),
            ("/v/clip.3gp", "/v/clip.m4a"),
            ("/v/clip.flv", "/v/clip.m4a"),
            ("/v/seg.m4s", "/v/seg.m4a"),
        ] {
            assert_eq!(with_extension(Path::new(name), "m4a"), Path::new(expected));
        }
    }

    fn video(codecs: &str) -> VideoTrack {
        VideoTrack {
            id: "v".into(),
            width: Some(1920),
            height: Some(1080),
            bandwidth: Some(1),
            codecs: Some(codecs.into()),
            frame_rate: None,
            format: InputFormat::Mp4,
        }
    }

    #[test]
    fn a_stream_in_vp9_or_av1_is_saved_as_matroska_like_a_page() {
        let info = |codecs: &str| StreamInfo {
            kind: StreamKind::Dash,
            url: Url::parse("https://x.example/m.mpd").unwrap(),
            duration: None,
            live: false,
            video: vec![video(codecs)],
            audio: Vec::new(),
            subtitles: Vec::new(),
            auto: AutoPicks {
                video: Some("v".into()),
                ..AutoPicks::default()
            },
        };
        let ext = |codecs: &str| info(codecs).extension(&Choice::Auto, &Choice::Auto, true);
        assert_eq!(ext("avc1.640028,mp4a.40.2"), "mp4");
        assert_eq!(ext("vp09.00.40.08,mp4a.40.2"), "mkv");
        assert_eq!(ext("av01.0.08M.08,mp4a.40.2"), "mkv");
    }
}
