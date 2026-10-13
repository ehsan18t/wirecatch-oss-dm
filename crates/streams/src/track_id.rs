//! Track ids derived from what a track is, not where it is listed. Core keeps the chosen id to continue a download after a restart, and the playlist is read again then; a refreshed or rebalanced playlist may list its tracks in another order, and a position would then name another quality or language.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use url::Url;

use crate::plan::{Fnv, is_signing_parameter};
use crate::{dash, hls};

/// The one track of a media playlist that is not part of a master playlist.
pub(crate) const SINGLE: &str = "v-playlist";

/// Prefixes, one per kind of id, so ids of different kinds never meet.
const VARIANT: &str = "v";
const RENDITION: &str = "r";
/// A DASH representation named by its own `id`.
const REPRESENTATION: &str = "d";
/// A DASH representation without an `id`, named by its attributes.
const UNNAMED_REPRESENTATION: &str = "dh";
/// Joins an id and what tells it apart from another track with the same id.
const SUFFIX: char = '~';

fn hash(parts: &[&str]) -> String {
    let mut hash = Fnv::default();
    for part in parts {
        hash.write(part.as_bytes());
        hash.write(&[0xFF]);
    }
    format!("{:016x}", hash.0)
}

fn text<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// A link's path and query, without the host (which CDNs change) or the values of signing parameters (which a refreshed link changes).
fn stable_link(url: &Url) -> String {
    let mut link = url.path().to_owned();
    for (name, value) in url.query_pairs() {
        link.push(if link.contains('?') { '&' } else { '?' });
        link.push_str(&name);
        if !is_signing_parameter(&name) {
            link.push('=');
            link.push_str(&value);
        }
    }
    link
}

/// Adds `~<suffix>` to every id that several entries share; `suffix` gets the entry's index and its number among them, in list order, and `None` keeps the id.
fn suffix_shared(ids: &[String], suffix: impl Fn(usize, usize) -> Option<String>) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in ids {
        *counts.entry(id).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            if counts[id.as_str()] < 2 {
                return id.clone();
            }
            let n = seen.entry(id).or_default();
            let numbered = suffix(i, *n);
            *n += 1;
            numbered.map_or_else(|| id.clone(), |s| format!("{id}{SUFFIX}{s}"))
        })
        .collect()
}

/// Tracks that are the same in every respect are told apart by their number among them, in list order: the one case where a reordered list can still swap them.
fn numbered(ids: &[String]) -> Vec<String> {
    suffix_shared(ids, |_, n| Some((n + 1).to_string()))
}

/// Every variant's id: a hash of its playlist link, bandwidth, picture size, frame rate, codecs and audio group.
pub(crate) fn variants(variants: &[hls::Variant]) -> Vec<String> {
    numbered(
        &variants
            .iter()
            .map(|v| {
                let size = text(v.resolution.map(|(w, h)| format!("{w}x{h}")));
                let id = hash(&[
                    &stable_link(&v.url),
                    &text(v.bandwidth),
                    &size,
                    &text(v.frame_rate),
                    v.codecs.as_deref().unwrap_or_default(),
                    v.audio_group.as_deref().unwrap_or_default(),
                ]);
                format!("{VARIANT}-{id}")
            })
            .collect::<Vec<_>>(),
    )
}

fn rendition_kind(kind: hls::RenditionKind) -> &'static str {
    match kind {
        hls::RenditionKind::Audio => "audio",
        hls::RenditionKind::Video => "video",
        hls::RenditionKind::Subtitles => "subtitles",
        hls::RenditionKind::ClosedCaptions => "closed-captions",
    }
}

/// Every rendition's id: a hash of its kind, group, language, name, channels and playlist link.
pub(crate) fn renditions(renditions: &[hls::Rendition]) -> Vec<String> {
    numbered(
        &renditions
            .iter()
            .map(|r| {
                let id = hash(&[
                    rendition_kind(r.kind),
                    &r.group,
                    r.language.as_deref().unwrap_or_default(),
                    r.name.as_deref().unwrap_or_default(),
                    r.channels.as_deref().unwrap_or_default(),
                    &text(r.url.as_ref().map(stable_link)),
                ]);
                format!("{RENDITION}-{id}")
            })
            .collect::<Vec<_>>(),
    )
}

fn representation_kind(kind: dash::Kind) -> &'static str {
    match kind {
        dash::Kind::Video => "video",
        dash::Kind::Audio => "audio",
        dash::Kind::Text => "text",
    }
}

fn attributes(rep: &dash::Rep) -> String {
    hash(&[
        representation_kind(rep.kind),
        &text(rep.bandwidth),
        &text(rep.width),
        &text(rep.height),
        &text(rep.frame_rate),
        rep.codecs.as_deref().unwrap_or_default(),
        rep.mime.as_deref().unwrap_or_default(),
        rep.language.as_deref().unwrap_or_default(),
        rep.label.as_deref().unwrap_or_default(),
        rep.channels.as_deref().unwrap_or_default(),
    ])
}

/// Every representation's id: its own `id`, which the standard requires to be unique and manifests keep stable, or a hash of its attributes when a manifest leaves the id out. Manifests that repeat an id (two audio sets numbered from the same start, say) get the attributes added to tell those apart.
pub(crate) fn representations(reps: &[dash::Rep]) -> Vec<String> {
    let ids: Vec<String> = reps
        .iter()
        .map(|r| {
            if r.id.is_empty() {
                format!("{UNNAMED_REPRESENTATION}-{}", attributes(r))
            } else {
                format!("{REPRESENTATION}-{}", r.id)
            }
        })
        .collect();
    let ids = suffix_shared(&ids, |i, _| {
        (!reps[i].id.is_empty()).then(|| attributes(&reps[i]))
    });
    numbered(&ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Manifests that repeat a representation id (two audio sets numbered from the same start) gave
    // two tracks one id, and the first was taken: another language, silently.
    #[test]
    fn repeated_representation_ids_are_told_apart() {
        let rep = |id: &str, language: &str| dash::Rep {
            id: id.into(),
            kind: dash::Kind::Audio,
            bandwidth: Some(128_000),
            width: None,
            height: None,
            frame_rate: None,
            codecs: Some("mp4a.40.2".into()),
            mime: None,
            language: Some(language.into()),
            label: None,
            channels: None,
            main: false,
            drm: None,
            segments: dash::Segments::Whole(std::sync::Arc::new(
                Url::parse("https://cdn.example/a.mp4").unwrap(),
            )),
        };
        let english = rep("1", "en");
        let ids = representations(&[rep("1", "de"), rep("1", "en"), rep("2", "fr")]);
        assert_eq!(ids.len(), 3);
        assert!(ids[0] != ids[1] && ids[0].starts_with("d-1~"), "{ids:?}");
        assert_eq!(ids[2], "d-2");
        // The same track keeps its id when the manifest lists it elsewhere.
        let moved = representations(&[rep("2", "fr"), english, rep("1", "de")]);
        assert_eq!(moved[1], ids[1]);

        let twins = representations(&[rep("", "en"), rep("", "en")]);
        assert_ne!(twins[0], twins[1]);
        assert!(twins[0].starts_with("dh-"), "{twins:?}");
    }
}
