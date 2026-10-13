//! Which tracks a choice picks, decided once from the playlist or manifest alone, before anything else is fetched. [`Source::plan`](crate::source::Source::plan) downloads what this picks, and [`StreamInfo::extension`](crate::StreamInfo::extension) forecasts the file from the same picks, so the two cannot disagree.

#![forbid(unsafe_code)]

use crate::dash::{Kind, Mpd};
use crate::error::{Error, Result};
use crate::hls::{self, Master, RenditionKind};
use crate::model::{Choice, Role};
use crate::track_id;

/// One track of a playlist or manifest, by its place in the parsed lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    /// The only track of an HLS media playlist.
    Single,
    /// An HLS master playlist's variant.
    Variant(usize),
    /// An HLS master playlist's rendition.
    Rendition(usize),
    /// A DASH representation.
    Representation(usize),
}

/// What a choice of video and audio picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Selected {
    pub video: Option<Pick>,
    pub audio: Option<Pick>,
}

/// The track ids of a source, by kind, in list order (see [`track_id`]).
pub(crate) struct Ids {
    pub variants: Vec<String>,
    pub renditions: Vec<String>,
    pub representations: Vec<String>,
}

impl Ids {
    pub fn of_master(master: &Master) -> Self {
        Self {
            variants: track_id::variants(&master.variants),
            renditions: track_id::renditions(&master.renditions),
            representations: Vec::new(),
        }
    }

    pub fn of_mpd(mpd: &Mpd) -> Self {
        Self {
            variants: Vec::new(),
            renditions: Vec::new(),
            representations: track_id::representations(&mpd.reps),
        }
    }

    pub fn single() -> Self {
        Self {
            variants: Vec::new(),
            renditions: Vec::new(),
            representations: Vec::new(),
        }
    }

    /// The id of `pick`.
    pub fn of(&self, pick: Pick) -> Option<&str> {
        match pick {
            Pick::Single => Some(track_id::SINGLE),
            Pick::Variant(i) => self.variants.get(i).map(String::as_str),
            Pick::Rendition(i) => self.renditions.get(i).map(String::as_str),
            Pick::Representation(i) => self.representations.get(i).map(String::as_str),
        }
    }
}

fn no_such(kind: Role, id: &str) -> Error {
    Error::NoSuchTrack {
        kind,
        id: id.to_string(),
    }
}

/// What a choice picks from an HLS media playlist: its one track, as video.
pub(crate) fn from_media(video: &Choice, audio: &Choice) -> Result<Selected> {
    let video = match video {
        Choice::Skip => None,
        Choice::Auto => Some(Pick::Single),
        Choice::Track(id) if id == track_id::SINGLE => Some(Pick::Single),
        Choice::Track(id) => return Err(no_such(Role::Video, id)),
    };
    if let Choice::Track(id) = audio {
        return Err(no_such(Role::Audio, id));
    }
    Ok(Selected { video, audio: None })
}

/// What a choice picks from an HLS master playlist. Automatic video is the best quality with video (or, without any, the best of all); automatic audio is the audio the chosen video is paired with when it comes separately, or for audio alone the default rendition, else the best audio-only variant.
pub(crate) fn from_master(
    master: &Master,
    ids: &Ids,
    video: &Choice,
    audio: &Choice,
) -> Result<Selected> {
    let variant = match video {
        Choice::Skip => None,
        Choice::Auto => Some(
            best_variant(master, |v| !v.audio_only())
                .or_else(|| best_variant(master, |_| true))
                .ok_or(Error::NothingChosen)?,
        ),
        Choice::Track(id) => Some(
            variant_named(master, &ids.variants, id, false)
                .ok_or_else(|| no_such(Role::Video, id))?,
        ),
    };
    let audio = match audio {
        Choice::Skip => None,
        Choice::Track(id) => Some(audio_named(master, ids, id)?),
        Choice::Auto => match variant {
            Some(v) => paired_audio(master, v),
            None => audio_alone(master),
        },
    };
    Ok(Selected {
        video: variant.map(Pick::Variant),
        audio,
    })
}

/// The audio track named `id`: an audio rendition with its own playlist, or an audio-only variant.
fn audio_named(master: &Master, ids: &Ids, id: &str) -> Result<Pick> {
    let rendition = master
        .renditions
        .iter()
        .zip(&ids.renditions)
        .position(|(r, rid)| r.kind == RenditionKind::Audio && rid == id);
    match rendition {
        Some(r) if master.renditions[r].url.is_some() => Ok(Pick::Rendition(r)),
        Some(_) => Err(no_such(Role::Audio, id)),
        None => variant_named(master, &ids.variants, id, true)
            .map(Pick::Variant)
            .ok_or_else(|| no_such(Role::Audio, id)),
    }
}

/// The audio variant `v` is paired with, if it comes separately: the default rendition of its audio group, else the one to select automatically, else the first.
fn paired_audio(master: &Master, v: usize) -> Option<Pick> {
    let group = master.variants.get(v)?.audio_group.as_ref()?;
    let in_group: Vec<usize> = (0..master.renditions.len())
        .filter(|&i| {
            let r = &master.renditions[i];
            r.kind == RenditionKind::Audio && &r.group == group && r.url.is_some()
        })
        .collect();
    in_group
        .iter()
        .find(|&&i| master.renditions[i].default)
        .or_else(|| in_group.iter().find(|&&i| master.renditions[i].autoselect))
        .or_else(|| in_group.first())
        .map(|&i| Pick::Rendition(i))
}

/// Audio without video: the default rendition, else the best audio-only variant.
fn audio_alone(master: &Master) -> Option<Pick> {
    (0..master.renditions.len())
        .filter(|&i| {
            let r = &master.renditions[i];
            r.kind == RenditionKind::Audio && r.url.is_some()
        })
        .max_by_key(|&i| {
            (
                master.renditions[i].default,
                master.renditions[i].autoselect,
            )
        })
        .map(Pick::Rendition)
        .or_else(|| best_variant(master, hls::Variant::audio_only).map(Pick::Variant))
}

/// The highest bandwidth variant (then the largest picture) among those `keep` accepts.
pub(crate) fn best_variant(master: &Master, keep: impl Fn(&hls::Variant) -> bool) -> Option<usize> {
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

/// The index of the variant with id `id` among the variants with video, or with `audio_only` among the audio-only ones. `ids` are the variants' ids.
fn variant_named(master: &Master, ids: &[String], id: &str, audio_only: bool) -> Option<usize> {
    (0..master.variants.len()).find(|&i| {
        master.variants[i].audio_only() == audio_only && ids.get(i).is_some_and(|v| v == id)
    })
}

/// What a choice picks from a DASH manifest: automatically, the main representation of each kind, then the highest bandwidth, then the largest picture.
pub(crate) fn from_mpd(mpd: &Mpd, ids: &Ids, video: &Choice, audio: &Choice) -> Result<Selected> {
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
    let pick = |choice: &Choice, kind: Kind, role: Role| -> Result<Option<Pick>> {
        let index = match choice {
            Choice::Skip => None,
            Choice::Auto => best(kind),
            Choice::Track(id) => Some(
                (0..mpd.reps.len())
                    .find(|&i| {
                        mpd.reps[i].kind == kind
                            && ids.representations.get(i).is_some_and(|r| r == id)
                    })
                    .ok_or_else(|| no_such(role, id))?,
            ),
        };
        Ok(index.map(Pick::Representation))
    };
    Ok(Selected {
        video: pick(video, Kind::Video, Role::Video)?,
        audio: pick(audio, Kind::Audio, Role::Audio)?,
    })
}
