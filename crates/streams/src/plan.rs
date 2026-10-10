//! From a link to a plan: load the playlist or manifest, describe what it
//! offers, and turn a choice of tracks into the list of pieces to fetch.

use std::ops::Range;
use std::time::Duration;

use url::Url;
use wirecatch_engine::Fetcher;

use crate::dash::{self, Kind, Mpd, Segments};
use crate::error::{Error, Result};
use crate::hls::{self, Master, Media, Playlist, RenditionKind};
use crate::model::{AudioTrack, Choice, StreamInfo, StreamKind, SubtitleTrack, VideoTrack};

/// Largest playlist or manifest read. A ten-hour HLS playlist of 2-second
/// segments with long signed addresses is about 4 MiB.
const MANIFEST_LIMIT: u64 = 8 * 1024 * 1024;

/// The most pieces one track may have (about 55 hours of 1-second
/// segments), so a broken or hostile playlist cannot exhaust memory.
pub(crate) const MAX_SEGMENTS: u64 = 200_000;

/// The error for a track over [`MAX_SEGMENTS`].
pub(crate) fn too_many_segments() -> String {
    format!(
        "it lists more than {MAX_SEGMENTS} segments, more than any video Wirecatch can download"
    )
}

/// Largest `sidx` index read for a DASH `SegmentBase`.
const INDEX_LIMIT: u64 = 16 * 1024 * 1024;

/// One thing to fetch: a whole resource or a byte range of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub url: Url,
    /// Start inclusive, end exclusive.
    pub range: Option<Range<u64>>,
    pub encryption: Option<Encryption>,
    /// An init section (HLS `EXT-X-MAP`, DASH initialization).
    pub init: bool,
}

impl Piece {
    pub fn plain(url: Url, range: Option<Range<u64>>) -> Self {
        Self {
            url,
            range,
            encryption: None,
            init: false,
        }
    }
}

/// HLS AES-128: where the key is and which IV to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Encryption {
    pub key_url: Url,
    pub iv: [u8; 16],
}

/// Why a track cannot be downloaded.
#[derive(Debug, Clone)]
pub(crate) enum Refusal {
    Drm(String),
    Unsupported(String),
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Drm(name) => Error::Drm(name),
            Refusal::Unsupported(what) => Error::Unsupported(what),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// Video, possibly with its own audio inside.
    Video,
    Audio,
}

/// One track to download and join.
#[derive(Debug, Clone)]
pub(crate) struct Track {
    pub role: Role,
    pub codecs: Option<String>,
    pub pieces: Vec<Piece>,
}

/// Everything to download, in the order the tracks are given to ffmpeg.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub tracks: Vec<Track>,
}

impl Plan {
    /// Identifies the plan across runs, to tell whether segments on disk
    /// belong to it. Query strings and hosts are left out: they often carry
    /// tokens or CDN choices that change when a link is refreshed.
    pub fn fingerprint(&self) -> u64 {
        let mut hash = Fnv::default();
        hash.write(&(self.tracks.len() as u64).to_le_bytes());
        for track in &self.tracks {
            hash.write(&[track.role as u8]);
            hash.write(&(track.pieces.len() as u64).to_le_bytes());
            for piece in &track.pieces {
                hash.write(piece.url.path().as_bytes());
                if let Some(range) = &piece.range {
                    hash.write(&range.start.to_le_bytes());
                    hash.write(&range.end.to_le_bytes());
                }
                if let Some(encryption) = &piece.encryption {
                    hash.write(&encryption.iv);
                }
                hash.write(&[piece.init as u8, 0xFF]);
            }
        }
        hash.0
    }
}

/// FNV-1a, 64 bits: stable across runs and Rust versions, unlike the
/// standard library's hasher.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A loaded playlist or manifest.
#[derive(Debug)]
pub(crate) enum Source {
    HlsMaster { url: Url, master: Master },
    HlsMedia { url: Url, media: Media },
    Dash { url: Url, mpd: Mpd },
}

impl Source {
    /// Fetches and reads the link the fetcher was made for.
    pub async fn load(fetcher: &Fetcher) -> Result<Self> {
        let fetched = fetcher.get(fetcher.url(), None, MANIFEST_LIMIT).await?;
        let url = fetched.url;
        let text = String::from_utf8_lossy(&fetched.body);
        match detect(&text, fetched.content_type.as_deref(), &url) {
            Some(StreamKind::Hls) => match hls::parse(&text, &url).map_err(Error::Manifest)? {
                Playlist::Master(master) => Ok(Source::HlsMaster { url, master }),
                Playlist::Media(media) => Ok(Source::HlsMedia { url, media }),
            },
            Some(StreamKind::Dash) => {
                let mpd = dash::parse(&text, &url).map_err(Error::Manifest)?;
                Ok(Source::Dash { url, mpd })
            }
            None => Err(Error::NotAStream),
        }
    }

    /// Describes what the stream offers. For an HLS master playlist this
    /// reads the best quality's playlist too, for the duration and to tell
    /// whether the stream is live.
    pub async fn info(&self, fetcher: &Fetcher) -> Result<StreamInfo> {
        match self {
            Source::HlsMaster { url, master } => {
                let best = best_variant(master, |v| !v.audio_only())
                    .or_else(|| best_variant(master, |_| true))
                    .ok_or(Error::NothingChosen)?;
                let media = media_playlist(fetcher, &master.variants[best].url).await?;
                Ok(StreamInfo {
                    kind: StreamKind::Hls,
                    url: url.clone(),
                    duration: seconds(media.duration),
                    live: !media.ended,
                    video: master
                        .variants
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| !v.audio_only())
                        .map(|(i, v)| VideoTrack {
                            id: format!("v{i}"),
                            width: v.resolution.map(|r| r.0),
                            height: v.resolution.map(|r| r.1),
                            bandwidth: v.bandwidth,
                            codecs: v.codecs.clone(),
                            frame_rate: v.frame_rate,
                        })
                        .collect(),
                    audio: hls_audio(master),
                    subtitles: master
                        .renditions
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.kind == RenditionKind::Subtitles)
                        .map(|(i, r)| SubtitleTrack {
                            id: format!("r{i}"),
                            language: r.language.clone(),
                            name: r.name.clone(),
                        })
                        .collect(),
                })
            }
            Source::HlsMedia { url, media } => Ok(StreamInfo {
                kind: StreamKind::Hls,
                url: url.clone(),
                duration: seconds(media.duration),
                live: !media.ended,
                video: vec![VideoTrack {
                    id: "v0".into(),
                    width: None,
                    height: None,
                    bandwidth: None,
                    codecs: None,
                    frame_rate: None,
                }],
                audio: Vec::new(),
                subtitles: Vec::new(),
            }),
            Source::Dash { url, mpd } => Ok(StreamInfo {
                kind: StreamKind::Dash,
                url: url.clone(),
                duration: mpd.duration.and_then(seconds),
                live: mpd.dynamic,
                video: mpd
                    .reps
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.kind == Kind::Video)
                    .map(|(i, r)| VideoTrack {
                        id: format!("d{i}"),
                        width: r.width,
                        height: r.height,
                        bandwidth: r.bandwidth,
                        codecs: r.codecs.clone(),
                        frame_rate: r.frame_rate,
                    })
                    .collect(),
                audio: mpd
                    .reps
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.kind == Kind::Audio)
                    .map(|(i, r)| AudioTrack {
                        id: format!("d{i}"),
                        language: r.language.clone(),
                        name: r.label.clone(),
                        bandwidth: r.bandwidth,
                        codecs: r.codecs.clone(),
                        channels: r.channels.clone(),
                        default: r.main,
                    })
                    .collect(),
                subtitles: mpd
                    .reps
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.kind == Kind::Text)
                    .map(|(i, r)| SubtitleTrack {
                        id: format!("d{i}"),
                        language: r.language.clone(),
                        name: r.label.clone(),
                    })
                    .collect(),
            }),
        }
    }

    /// Turns a choice of tracks into a plan, reading the media playlists (or
    /// DASH indexes) it needs. Refuses live streams, DRM and anything else
    /// that cannot be downloaded, before a single segment is fetched.
    pub async fn plan(self, fetcher: &Fetcher, video: &Choice, audio: &Choice) -> Result<Plan> {
        let plan = match self {
            Source::HlsMedia { media, .. } => {
                let tracks = match video {
                    Choice::Skip => Vec::new(),
                    Choice::Auto => vec![hls_track(Role::Video, None, media)?],
                    Choice::Track(id) if id == "v0" => vec![hls_track(Role::Video, None, media)?],
                    Choice::Track(id) => {
                        return Err(Error::NoSuchTrack {
                            kind: "video",
                            id: id.clone(),
                        });
                    }
                };
                if let Choice::Track(id) = audio {
                    return Err(Error::NoSuchTrack {
                        kind: "audio",
                        id: id.clone(),
                    });
                }
                Plan { tracks }
            }
            Source::HlsMaster { master, .. } => plan_hls(fetcher, &master, video, audio).await?,
            Source::Dash { mpd, .. } => plan_dash(fetcher, mpd, video, audio).await?,
        };
        if plan.tracks.is_empty() {
            return Err(Error::NothingChosen);
        }
        Ok(plan)
    }
}

/// Which format a fetched link is: its content first (most reliable), then
/// its `Content-Type`, then its extension.
pub(crate) fn detect(text: &str, content_type: Option<&str>, url: &Url) -> Option<StreamKind> {
    let start = text.trim_start_matches('\u{feff}').trim_start();
    if start.starts_with("#EXTM3U") {
        return Some(StreamKind::Hls);
    }
    if start.starts_with('<') {
        let head: String = start.chars().take(4096).collect();
        if head.contains("<MPD") || head.contains(":MPD") {
            return Some(StreamKind::Dash);
        }
    }
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    if content_type.contains("mpegurl") {
        return Some(StreamKind::Hls);
    }
    if content_type.contains("dash+xml") {
        return Some(StreamKind::Dash);
    }
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".m3u8") || path.ends_with(".m3u") {
        Some(StreamKind::Hls)
    } else if path.ends_with(".mpd") {
        Some(StreamKind::Dash)
    } else {
        None
    }
}

fn seconds(value: f64) -> Option<Duration> {
    (value.is_finite() && value > 0.0).then(|| Duration::from_secs_f64(value))
}

/// Audio renditions with their own playlist, then audio-only variants.
fn hls_audio(master: &Master) -> Vec<AudioTrack> {
    let renditions = master
        .renditions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == RenditionKind::Audio && r.url.is_some())
        .map(|(i, r)| AudioTrack {
            id: format!("r{i}"),
            language: r.language.clone(),
            name: r.name.clone(),
            bandwidth: None,
            codecs: None,
            channels: r.channels.clone(),
            default: r.default,
        });
    let variants = master
        .variants
        .iter()
        .enumerate()
        .filter(|(_, v)| v.audio_only())
        .map(|(i, v)| AudioTrack {
            id: format!("v{i}"),
            language: None,
            name: None,
            bandwidth: v.bandwidth,
            codecs: v.codecs.clone(),
            channels: None,
            default: false,
        });
    renditions.chain(variants).collect()
}

/// The highest bandwidth variant (then the largest picture) among those `keep` accepts.
fn best_variant(master: &Master, keep: impl Fn(&hls::Variant) -> bool) -> Option<usize> {
    master
        .variants
        .iter()
        .enumerate()
        .filter(|(_, v)| keep(v))
        .max_by_key(|(_, v)| {
            (
                v.bandwidth.unwrap_or(0),
                v.resolution.map_or(0, |(w, h)| u64::from(w) * u64::from(h)),
            )
        })
        .map(|(i, _)| i)
}

/// Parses `id` as `<prefix><index>`.
fn index_of(id: &str, prefix: char) -> Option<usize> {
    id.strip_prefix(prefix)?.parse().ok()
}

async fn plan_hls(
    fetcher: &Fetcher,
    master: &Master,
    video: &Choice,
    audio: &Choice,
) -> Result<Plan> {
    let no_video = |id: &str| Error::NoSuchTrack {
        kind: "video",
        id: id.to_string(),
    };
    let no_audio = |id: &str| Error::NoSuchTrack {
        kind: "audio",
        id: id.to_string(),
    };

    let variant = match video {
        Choice::Skip => None,
        Choice::Auto => Some(
            best_variant(master, |v| !v.audio_only())
                .or_else(|| best_variant(master, |_| true))
                .ok_or(Error::NothingChosen)?,
        ),
        Choice::Track(id) => Some(
            index_of(id, 'v')
                .filter(|i| master.variants.get(*i).is_some_and(|v| !v.audio_only()))
                .ok_or_else(|| no_video(id))?,
        ),
    };

    // The audio as a playlist to fetch; `None` when there is no separate audio.
    let audio_url: Option<(Url, Option<String>)> = match audio {
        Choice::Skip => None,
        Choice::Track(id) => {
            if let Some(i) = index_of(id, 'r') {
                let rendition = master
                    .renditions
                    .get(i)
                    .filter(|r| r.kind == RenditionKind::Audio)
                    .ok_or_else(|| no_audio(id))?;
                Some((rendition.url.clone().ok_or_else(|| no_audio(id))?, None))
            } else {
                let i = index_of(id, 'v')
                    .filter(|i| master.variants.get(*i).is_some_and(|v| v.audio_only()))
                    .ok_or_else(|| no_audio(id))?;
                Some((
                    master.variants[i].url.clone(),
                    master.variants[i].codecs.clone(),
                ))
            }
        }
        Choice::Auto => match variant {
            // The audio the chosen video is paired with, if it comes separately.
            Some(v) => master.variants[v].audio_group.as_ref().and_then(|group| {
                let in_group = |r: &&hls::Rendition| {
                    r.kind == RenditionKind::Audio && &r.group == group && r.url.is_some()
                };
                let renditions: Vec<_> = master.renditions.iter().filter(in_group).collect();
                renditions
                    .iter()
                    .find(|r| r.default)
                    .or_else(|| renditions.iter().find(|r| r.autoselect))
                    .or_else(|| renditions.first())
                    .and_then(|r| r.url.clone())
                    .map(|url| (url, None))
            }),
            // Audio only: the default rendition, else the best audio-only variant.
            None => master
                .renditions
                .iter()
                .filter(|r| r.kind == RenditionKind::Audio && r.url.is_some())
                .max_by_key(|r| (r.default, r.autoselect))
                .and_then(|r| r.url.clone())
                .map(|url| (url, None))
                .or_else(|| {
                    best_variant(master, |v| v.audio_only()).map(|i| {
                        (
                            master.variants[i].url.clone(),
                            master.variants[i].codecs.clone(),
                        )
                    })
                }),
        },
    };

    let video_fetch = async {
        match variant {
            Some(v) => Ok(Some(
                media_playlist(fetcher, &master.variants[v].url).await?,
            )),
            None => Ok::<_, Error>(None),
        }
    };
    let audio_fetch = async {
        match &audio_url {
            Some((url, _)) => Ok(Some(media_playlist(fetcher, url).await?)),
            None => Ok::<_, Error>(None),
        }
    };
    let (video_media, audio_media) = tokio::try_join!(video_fetch, audio_fetch)?;

    let mut tracks = Vec::new();
    if let (Some(v), Some(media)) = (variant, video_media) {
        tracks.push(hls_track(
            Role::Video,
            master.variants[v].codecs.clone(),
            media,
        )?);
    }
    if let (Some((_, codecs)), Some(media)) = (audio_url, audio_media) {
        tracks.push(hls_track(Role::Audio, codecs, media)?);
    }
    Ok(Plan { tracks })
}

async fn media_playlist(fetcher: &Fetcher, url: &Url) -> Result<Media> {
    let fetched = fetcher.get(url, None, MANIFEST_LIMIT).await?;
    let text = String::from_utf8_lossy(&fetched.body);
    match hls::parse(&text, &fetched.url).map_err(Error::Manifest)? {
        Playlist::Media(media) => Ok(media),
        Playlist::Master(_) => Err(Error::Manifest(
            "a quality's playlist is itself a list of qualities".into(),
        )),
    }
}

fn hls_track(role: Role, codecs: Option<String>, media: Media) -> Result<Track> {
    if !media.ended {
        return Err(Error::Live);
    }
    if let Some(refusal) = media.refusal {
        return Err(refusal.into());
    }
    Ok(Track {
        role,
        codecs,
        pieces: media.pieces,
    })
}

async fn plan_dash(fetcher: &Fetcher, mpd: Mpd, video: &Choice, audio: &Choice) -> Result<Plan> {
    if mpd.dynamic {
        return Err(Error::Live);
    }
    if mpd.periods > 1 {
        return Err(Error::Unsupported(
            "a DASH manifest made of several periods (often a video with ads joined in)".into(),
        ));
    }
    let best = |kind: Kind| {
        mpd.reps
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == kind)
            .max_by_key(|(_, r)| {
                (
                    r.main,
                    r.bandwidth.unwrap_or(0),
                    u64::from(r.width.unwrap_or(0)) * u64::from(r.height.unwrap_or(0)),
                )
            })
            .map(|(i, _)| i)
    };
    let pick = |choice: &Choice, kind: Kind, name: &'static str| -> Result<Option<usize>> {
        match choice {
            Choice::Skip => Ok(None),
            Choice::Auto => Ok(best(kind)),
            Choice::Track(id) => index_of(id, 'd')
                .filter(|i| mpd.reps.get(*i).is_some_and(|r| r.kind == kind))
                .map(Some)
                .ok_or_else(|| Error::NoSuchTrack {
                    kind: name,
                    id: id.clone(),
                }),
        }
    };
    let chosen = [
        (pick(video, Kind::Video, "video")?, Role::Video),
        (pick(audio, Kind::Audio, "audio")?, Role::Audio),
    ];

    let mut reps: Vec<Option<dash::Rep>> = mpd.reps.into_iter().map(Some).collect();
    let mut tracks = Vec::new();
    for (index, role) in chosen {
        let Some(rep) = index.and_then(|i| reps.get_mut(i)).and_then(Option::take) else {
            continue;
        };
        if let Some(drm) = rep.drm {
            return Err(Error::Drm(drm));
        }
        let pieces = match rep.segments {
            Segments::Indexed { url, index } => indexed_pieces(fetcher, url, index).await?,
            segments => segments
                .into_pieces()
                .map_err(Error::Manifest)?
                .unwrap_or_default(),
        };
        tracks.push(Track {
            role,
            codecs: rep.codecs,
            pieces,
        });
    }
    Ok(Plan { tracks })
}

/// Reads a `SegmentBase` file's index to split it into ranges that download
/// in parallel; a file whose index cannot be used is fetched whole.
async fn indexed_pieces(fetcher: &Fetcher, url: Url, index: Range<u64>) -> Result<Vec<Piece>> {
    let fetched = fetcher.get(&url, Some(index.clone()), INDEX_LIMIT).await?;
    Ok(dash::sidx_pieces(&url, &index, &fetched.body)
        .unwrap_or_else(|| vec![Piece::plain(url, None)]))
}

/// Codec names (as in `CODECS` or `codecs`) that are video.
pub(crate) fn is_video_codec(codec: &str) -> bool {
    const VIDEO: [&str; 12] = [
        "avc1", "avc3", "hvc1", "hev1", "dvh1", "dvhe", "av01", "vp09", "vp9", "vp8", "mp4v",
        "theora",
    ];
    let name = codec
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    VIDEO.contains(&name.as_str())
}

/// Codec names that are audio.
pub(crate) fn is_audio_codec(codec: &str) -> bool {
    const AUDIO: [&str; 9] = [
        "mp4a", "ac-3", "ec-3", "ac-4", "opus", "vorbis", "flac", "mp3", "alac",
    ];
    let name = codec
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    AUDIO.contains(&name.as_str())
}

/// Codecs an MP4 file can hold with a plain stream copy.
pub(crate) fn mp4_compatible(codec: &str) -> bool {
    const MP4: [&str; 12] = [
        "avc1", "avc3", "hvc1", "hev1", "dvh1", "dvhe", "av01", "vp09", "mp4a", "ac-3", "ec-3",
        "mp4v",
    ];
    let name = codec
        .trim()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    MP4.contains(&name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_prefers_content_then_type_then_extension() {
        let url = Url::parse("https://x.example/watch").unwrap();
        assert_eq!(
            detect("\u{feff}#EXTM3U\n", None, &url),
            Some(StreamKind::Hls)
        );
        assert_eq!(
            detect("<?xml version=\"1.0\"?>\n<MPD>", Some("text/plain"), &url),
            Some(StreamKind::Dash)
        );
        assert_eq!(
            detect("", Some("application/vnd.apple.mpegurl"), &url),
            Some(StreamKind::Hls)
        );
        assert_eq!(
            detect("", Some("application/dash+xml"), &url),
            Some(StreamKind::Dash)
        );
        let mpd = Url::parse("https://x.example/a/manifest.MPD?token=1").unwrap();
        assert_eq!(detect("", None, &mpd), Some(StreamKind::Dash));
        assert_eq!(detect("<html>", Some("text/html"), &url), None);
    }

    #[test]
    fn codecs_are_classified() {
        assert!(is_video_codec("avc1.64001f"));
        assert!(is_audio_codec("mp4a.40.2"));
        assert!(!is_audio_codec("avc1"));
        assert!(mp4_compatible("hvc1.1.6.L93.B0"));
        assert!(!mp4_compatible("vp8"));
        assert!(!mp4_compatible("vorbis"));
    }

    #[test]
    fn fingerprints_ignore_tokens_but_not_segments() {
        let piece = |u: &str| Piece::plain(Url::parse(u).unwrap(), None);
        let plan = |urls: &[&str]| Plan {
            tracks: vec![Track {
                role: Role::Video,
                codecs: None,
                pieces: urls.iter().map(|u| piece(u)).collect(),
            }],
        };
        let a = plan(&[
            "https://cdn1.example/s/1.ts?token=a",
            "https://cdn1.example/s/2.ts?token=a",
        ]);
        let b = plan(&[
            "https://cdn2.example/s/1.ts?token=b",
            "https://cdn2.example/s/2.ts?token=b",
        ]);
        let c = plan(&["https://cdn1.example/s/1.ts", "https://cdn1.example/s/3.ts"]);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c.fingerprint());
    }
}
