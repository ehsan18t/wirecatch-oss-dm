//! From a link to a plan: load the playlist or manifest, describe what it offers, and turn a choice of tracks (see [`select`](crate::select)) into the pieces to fetch.

#![forbid(unsafe_code)]

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use url::Url;
use wirecatch_engine::Fetcher;
use wirecatch_engine::units::{KIB_USIZE, MIB};

use crate::codec::Codec;
use crate::dash::{self, Kind, Mpd, Segments};
use crate::error::{Error, Feature, ManifestError, Result};
use crate::ffmpeg::InputFormat;
use crate::hls::{self, Master, Media, Playlist, RenditionKind};
use crate::model::{
    AudioTrack, AutoPicks, Choice, Role, StreamInfo, StreamKind, SubtitleTrack, VideoTrack,
};
use crate::plan::{Piece, Plan, Track};
use crate::select::{self, Ids, Pick, Selected};
use crate::track_id;

/// Largest playlist or manifest read. A ten-hour HLS playlist of 2-second segments with long signed addresses is about 4 MiB.
const MANIFEST_LIMIT: u64 = 8 * MIB;

/// Largest `sidx` index read for a DASH `SegmentBase`.
const INDEX_LIMIT: u64 = 16 * MIB;

/// How much of an XML document is searched for its `MPD` root element.
const DASH_SNIFF_CHARS: usize = 4 * KIB_USIZE;

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
        let response = fetcher.get(fetcher.url(), None, MANIFEST_LIMIT).await?;
        let url = response.url;
        let text = String::from_utf8_lossy(&response.body);
        match detect(&text, response.content_type.as_deref(), &url) {
            Some(StreamKind::Hls) => {
                match hls::parse(&text, &url).map_err(|e| Error::manifest(&url, e))? {
                    Playlist::Master(master) => Ok(Source::HlsMaster { url, master }),
                    Playlist::Media(media) => Ok(Source::HlsMedia { url, media }),
                }
            }
            Some(StreamKind::Dash) => {
                let mpd = dash::parse(&text, &url).map_err(|e| Error::manifest(&url, e))?;
                Ok(Source::Dash { url, mpd })
            }
            None => Err(Error::NotAStream),
        }
    }

    /// The track ids of this source.
    fn ids(&self) -> Ids {
        match self {
            Source::HlsMaster { master, .. } => Ids::of_master(master),
            Source::HlsMedia { .. } => Ids::single(),
            Source::Dash { mpd, .. } => Ids::of_mpd(mpd),
        }
    }

    /// What `video` and `audio` pick, from the playlist or manifest alone.
    fn select(&self, ids: &Ids, video: &Choice, audio: &Choice) -> Result<Selected> {
        match self {
            Source::HlsMaster { master, .. } => select::from_master(master, ids, video, audio),
            Source::HlsMedia { .. } => select::from_media(video, audio),
            Source::Dash { mpd, .. } => select::from_mpd(mpd, ids, video, audio),
        }
    }

    /// Describes what the stream offers. For an HLS master playlist this reads the best quality's playlist too, for the duration and to tell whether the stream is live.
    pub async fn info(&self, fetcher: &Fetcher) -> Result<StreamInfo> {
        let mut info = match self {
            Source::HlsMaster { url, master } => hls_master_info(fetcher, url, master).await?,
            Source::HlsMedia { url, media } => hls_media_info(url, media),
            Source::Dash { url, mpd } => dash_info(url, mpd),
        };
        info.auto = self.auto_picks(&info);
        Ok(info)
    }

    /// What [`Choice::Auto`] picks for each kind, by track id.
    fn auto_picks(&self, info: &StreamInfo) -> AutoPicks {
        let ids = self.ids();
        let id = |pick: Option<Pick>| pick.and_then(|p| ids.of(p)).map(str::to_string);
        // A choice that cannot be made picks nothing, as the download would refuse it.
        let picked =
            |video: &Choice, audio: &Choice| self.select(&ids, video, audio).unwrap_or_default();
        let video = id(picked(&Choice::Auto, &Choice::Skip).video);
        let videos = video
            .iter()
            .cloned()
            .chain(info.video.iter().map(|v| v.id.clone()));
        let mut audio_with = Vec::new();
        for video_id in videos {
            if audio_with.iter().any(|(v, _)| *v == video_id) {
                continue;
            }
            let audio = id(picked(&Choice::Track(video_id.clone()), &Choice::Auto).audio);
            audio_with.push((video_id, audio));
        }
        AutoPicks {
            video,
            audio_with,
            audio_alone: id(picked(&Choice::Skip, &Choice::Auto).audio),
        }
    }

    /// Turns a choice of tracks into a plan, reading the media playlists (or DASH indexes) it needs. Refuses live streams, DRM and anything else that cannot be downloaded, before a single segment is fetched.
    pub async fn plan(self, fetcher: &Fetcher, video: &Choice, audio: &Choice) -> Result<Plan> {
        if let Source::Dash { mpd, .. } = &self {
            if mpd.dynamic {
                return Err(Error::Live);
            }
            if mpd.periods > 1 {
                return Err(Error::Unsupported(Feature::SeveralPeriods));
            }
        }
        let selected = self.select(&self.ids(), video, audio)?;
        let plan = match self {
            Source::HlsMedia { media, .. } => {
                let tracks = match selected.video {
                    Some(_) => vec![hls_track(Role::Video, None, media)?],
                    None => Vec::new(),
                };
                Plan { tracks }
            }
            Source::HlsMaster { master, .. } => plan_hls(fetcher, &master, selected).await?,
            Source::Dash { url, mpd } => plan_dash(fetcher, &url, mpd, selected).await?,
        };
        if plan.tracks.is_empty() {
            return Err(Error::NothingChosen);
        }
        Ok(plan)
    }
}

/// Which format a fetched link is: its content first (most reliable), then its `Content-Type`, then its extension.
pub(crate) fn detect(text: &str, content_type: Option<&str>, url: &Url) -> Option<StreamKind> {
    let start = text.trim_start_matches('\u{feff}').trim_start();
    if start.starts_with("#EXTM3U") {
        return Some(StreamKind::Hls);
    }
    if start.starts_with('<') {
        let head: String = start.chars().take(DASH_SNIFF_CHARS).collect();
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
    let path = url.path();
    if has_extension(path, "m3u8") || has_extension(path, "m3u") {
        Some(StreamKind::Hls)
    } else if has_extension(path, "mpd") {
        Some(StreamKind::Dash)
    } else {
        None
    }
}

/// Whether `path` ends in `.` and `extension`, in any case.
fn has_extension(path: &str, extension: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ending)| ending.eq_ignore_ascii_case(extension))
}

/// A duration read from a playlist or manifest, or `None` when it is missing, zero, negative or too large to be real. The value comes from the network, so it must never panic: `Duration::from_secs_f64` does above about 1.8e19 seconds, and release builds abort on panic.
fn seconds(value: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(value)
        .ok()
        .filter(|duration| !duration.is_zero())
}

/// The format of an HLS track joined as it is: fragmented MP4 when it has init sections, MPEG-TS otherwise.
fn hls_format(media: &Media) -> InputFormat {
    if media.pieces.iter().any(|p| p.init) {
        InputFormat::Mp4
    } else {
        InputFormat::MpegTs
    }
}

/// The format of a DASH representation joined as it is.
fn dash_format(rep: &dash::Rep) -> InputFormat {
    if rep.mime.as_deref().is_some_and(|m| m.contains("webm")) {
        InputFormat::WebM
    } else {
        InputFormat::Mp4
    }
}

/// Audio renditions with their own playlist, then audio-only variants.
fn hls_audio(master: &Master, ids: &Ids, format: InputFormat) -> Vec<AudioTrack> {
    let renditions = master
        .renditions
        .iter()
        .zip(&ids.renditions)
        .filter(|(r, _)| r.kind == RenditionKind::Audio && r.url.is_some())
        .map(|(r, id)| AudioTrack {
            id: id.clone(),
            language: r.language.clone(),
            name: r.name.clone(),
            bandwidth: None,
            codecs: None,
            channels: r.channels.clone(),
            default: r.default,
            format,
        });
    let variants = master
        .variants
        .iter()
        .zip(&ids.variants)
        .filter(|(v, _)| v.audio_only())
        .map(|(v, id)| AudioTrack {
            id: id.clone(),
            language: None,
            name: None,
            bandwidth: v.bandwidth,
            codecs: v.codecs.clone(),
            channels: None,
            default: false,
            format,
        });
    renditions.chain(variants).collect()
}

/// What an HLS master playlist offers; its best quality's playlist is read for the duration and to tell whether the stream is live.
async fn hls_master_info(fetcher: &Fetcher, url: &Url, master: &Master) -> Result<StreamInfo> {
    let best = select::best_variant(master, |v| !v.audio_only())
        .or_else(|| select::best_variant(master, |_| true))
        .and_then(|i| master.variants.get(i))
        .ok_or(Error::NothingChosen)?;
    let media = media_playlist(fetcher, &best.url).await?;
    let format = hls_format(&media);
    let ids = Ids::of_master(master);
    Ok(StreamInfo {
        kind: StreamKind::Hls,
        url: url.clone(),
        duration: seconds(media.duration),
        live: !media.ended,
        video: master
            .variants
            .iter()
            .zip(&ids.variants)
            .filter(|(v, _)| !v.audio_only())
            .map(|(v, id)| VideoTrack {
                id: id.clone(),
                width: v.resolution.map(|r| r.0),
                height: v.resolution.map(|r| r.1),
                bandwidth: v.bandwidth,
                codecs: v.codecs.clone(),
                frame_rate: v.frame_rate,
                format,
            })
            .collect(),
        audio: hls_audio(master, &ids, format),
        subtitles: master
            .renditions
            .iter()
            .zip(&ids.renditions)
            .filter(|(r, _)| r.kind == RenditionKind::Subtitles)
            .map(|(r, id)| SubtitleTrack {
                id: id.clone(),
                language: r.language.clone(),
                name: r.name.clone(),
            })
            .collect(),
        auto: AutoPicks::default(),
    })
}

/// What an HLS media playlist offers: one track.
fn hls_media_info(url: &Url, media: &Media) -> StreamInfo {
    StreamInfo {
        kind: StreamKind::Hls,
        url: url.clone(),
        duration: seconds(media.duration),
        live: !media.ended,
        video: vec![VideoTrack {
            id: track_id::SINGLE.into(),
            width: None,
            height: None,
            bandwidth: None,
            codecs: None,
            frame_rate: None,
            format: hls_format(media),
        }],
        audio: Vec::new(),
        subtitles: Vec::new(),
        auto: AutoPicks::default(),
    }
}

/// What a DASH manifest offers.
fn dash_info(url: &Url, mpd: &Mpd) -> StreamInfo {
    let ids = Ids::of_mpd(mpd);
    let tracks = |kind: Kind| {
        mpd.reps
            .iter()
            .zip(ids.representations.iter().cloned())
            .filter(move |(r, _)| r.kind == kind)
    };
    StreamInfo {
        kind: StreamKind::Dash,
        url: url.clone(),
        duration: mpd.duration.and_then(seconds),
        live: mpd.dynamic,
        video: tracks(Kind::Video)
            .map(|(r, id)| VideoTrack {
                id,
                width: r.width,
                height: r.height,
                bandwidth: r.bandwidth,
                codecs: r.codecs.as_deref().map(String::from),
                frame_rate: r.frame_rate,
                format: dash_format(r),
            })
            .collect(),
        audio: tracks(Kind::Audio)
            .map(|(r, id)| AudioTrack {
                id,
                language: r.language.as_deref().map(String::from),
                name: r.label.as_deref().map(String::from),
                bandwidth: r.bandwidth,
                codecs: r.codecs.as_deref().map(String::from),
                channels: r.channels.as_deref().map(String::from),
                default: r.main,
                format: dash_format(r),
            })
            .collect(),
        subtitles: tracks(Kind::Text)
            .map(|(r, id)| SubtitleTrack {
                id,
                language: r.language.as_deref().map(String::from),
                name: r.label.as_deref().map(String::from),
            })
            .collect(),
        auto: AutoPicks::default(),
    }
}

/// The playlist and codecs of what `pick` names in `master`: a variant's playlist with its codecs, or a rendition's playlist (renditions name none).
fn hls_playlist(master: &Master, pick: Pick) -> Option<(&Url, Option<&str>)> {
    match pick {
        Pick::Variant(i) => master
            .variants
            .get(i)
            .map(|v| (&v.url, v.codecs.as_deref())),
        Pick::Rendition(i) => master
            .renditions
            .get(i)
            .and_then(|r| r.url.as_ref())
            .map(|url| (url, None)),
        Pick::Single | Pick::Representation(_) => None,
    }
}

async fn plan_hls(fetcher: &Fetcher, master: &Master, selected: Selected) -> Result<Plan> {
    let video = selected.video.and_then(|p| hls_playlist(master, p));
    let audio = selected.audio.and_then(|p| hls_playlist(master, p));
    let (video_media, audio_media) = tokio::try_join!(
        optional_playlist(fetcher, video.map(|(url, _)| url)),
        optional_playlist(fetcher, audio.map(|(url, _)| url)),
    )?;
    let mut tracks = Vec::new();
    for (chosen, media, role) in [
        (video, video_media, Role::Video),
        (audio, audio_media, Role::Audio),
    ] {
        if let (Some((_, codecs)), Some(media)) = (chosen, media) {
            tracks.push(hls_track(role, codecs, media)?);
        }
    }
    Ok(Plan { tracks })
}

/// The media playlist at `url`, if there is one to fetch.
async fn optional_playlist(fetcher: &Fetcher, url: Option<&Url>) -> Result<Option<Media>> {
    match url {
        Some(url) => Ok(Some(media_playlist(fetcher, url).await?)),
        None => Ok(None),
    }
}

async fn media_playlist(fetcher: &Fetcher, url: &Url) -> Result<Media> {
    let response = fetcher.get(url, None, MANIFEST_LIMIT).await?;
    let text = String::from_utf8_lossy(&response.body);
    let manifest = |e| Error::manifest(&response.url, e);
    match hls::parse(&text, &response.url).map_err(manifest)? {
        Playlist::Media(media) => Ok(media),
        Playlist::Master(_) => Err(manifest(ManifestError::NestedMaster.into())),
    }
}

fn hls_track(role: Role, codecs: Option<&str>, media: Media) -> Result<Track> {
    if !media.ended {
        return Err(Error::Live);
    }
    if let Some(refusal) = media.refusal {
        return Err(refusal.into());
    }
    Ok(Track {
        role,
        codecs: codecs.map(Codec::list).unwrap_or_default(),
        pieces: media.pieces,
    })
}

/// The tracks `selected` picks from the manifest `mpd`, read from `url`.
async fn plan_dash(fetcher: &Fetcher, url: &Url, mpd: Mpd, selected: Selected) -> Result<Plan> {
    let manifest = |e: ManifestError| Error::manifest(url, e.into());
    let index = |pick: Option<Pick>| match pick {
        Some(Pick::Representation(i)) => Some(i),
        _ => None,
    };
    let chosen = [
        (index(selected.video), Role::Video),
        (index(selected.audio), Role::Audio),
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
            Segments::Indexed { url, index } => {
                indexed_pieces(fetcher, Arc::unwrap_or_clone(url), index).await?
            }
            Segments::Template(template) => template.expand().map_err(manifest)?,
            Segments::List(list) => list.expand().map_err(manifest)?,
            Segments::Whole(url) => vec![Piece::plain(Arc::unwrap_or_clone(url), None)],
        };
        // As for an HLS playlist: a track of nothing (an empty `SegmentList`, or a length of zero) would "complete" as an empty file.
        if !pieces.iter().any(|p| !p.init) {
            return Err(manifest(ManifestError::NoSegments));
        }
        tracks.push(Track {
            role,
            codecs: rep.codecs.as_deref().map(Codec::list).unwrap_or_default(),
            pieces,
        });
    }
    Ok(Plan { tracks })
}

/// Reads a `SegmentBase` file's index to split it into ranges that download in parallel; a file whose index cannot be used is fetched whole.
async fn indexed_pieces(fetcher: &Fetcher, url: Url, index: Range<u64>) -> Result<Vec<Piece>> {
    let response = fetcher.get(&url, Some(index.clone()), INDEX_LIMIT).await?;
    Ok(dash::sidx_pieces(&url, &index, &response.body)
        .unwrap_or_else(|| vec![Piece::plain(url, None)]))
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
    fn extensions_are_compared_in_any_case() {
        assert!(has_extension("/live/index.M3U8", "m3u8"));
        assert!(has_extension("/a.b/manifest.mpd", "mpd"));
        assert!(!has_extension("/a.mpd/manifest", "mpd"));
        assert!(!has_extension("/video.m3u8x", "m3u8"));
        assert!(!has_extension("/m3u8", "m3u8"));
    }
}
