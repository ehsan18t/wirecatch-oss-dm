//! MPEG-DASH manifests (ISO/IEC 23009-1): representations and their segments.
//!
//! Supported: one period; `SegmentTemplate` with `$RepresentationID$`,
//! `$Number$`, `$Time$` and `$Bandwidth$` (with `%0Nd` widths), either
//! duration based or with a `SegmentTimeline` (including `r="-1"`);
//! `SegmentList`; `SegmentBase` with an `indexRange` (read through the file's
//! `sidx` box) or without one (the whole file at once). Attributes and segment
//! information are inherited from the period and adaptation set as the
//! standard says. Several periods are reported as unsupported.

use std::ops::Range;

use roxmltree::{Document, Node};
use url::Url;

use crate::plan::{MAX_SEGMENTS, Piece, is_audio_codec, is_video_codec, too_many_segments};

/// For segment numbers or times that would pass the largest number.
const TOO_LARGE: &str = "its segment numbers or times are too large";

/// What a manifest offers, from its first period.
#[derive(Debug)]
pub(crate) struct Mpd {
    /// `type="dynamic"`: a live stream.
    pub dynamic: bool,
    /// `mediaPresentationDuration`, in seconds.
    pub duration: Option<f64>,
    pub periods: usize,
    pub reps: Vec<Rep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Video,
    Audio,
    Text,
}

/// One representation.
#[derive(Debug)]
pub(crate) struct Rep {
    pub kind: Kind,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub codecs: Option<String>,
    pub language: Option<String>,
    pub label: Option<String>,
    pub channels: Option<String>,
    /// The adaptation set says it is the main one (`Role value="main"`).
    pub main: bool,
    /// Set when the representation is encrypted (`ContentProtection`).
    pub drm: Option<String>,
    pub segments: Segments,
}

/// Where a representation's bytes are.
#[derive(Debug)]
pub(crate) enum Segments {
    /// A `SegmentTemplate`, expanded on demand with [`Segments::pieces`].
    Template {
        template: Template,
        base: Url,
        id: String,
        bandwidth: u64,
        /// Length of the period, in seconds.
        period: Option<f64>,
    },
    /// A `SegmentList`, already resolved.
    List(Vec<Piece>),
    /// A `SegmentBase` with an index: the segment ranges are in the file's
    /// `sidx` box at `index`.
    Indexed { url: Url, index: Range<u64> },
    /// One file holding everything.
    Whole(Url),
}

/// The attributes of a `SegmentTemplate`, merged over the levels it appears on.
#[derive(Debug, Clone, Default)]
pub(crate) struct Template {
    media: Option<String>,
    initialization: Option<String>,
    start_number: Option<u64>,
    end_number: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    presentation_time_offset: Option<u64>,
    timeline: Option<Vec<Entry>>,
}

/// One `S` element of a `SegmentTimeline`.
#[derive(Debug, Clone, Copy)]
struct Entry {
    t: Option<u64>,
    d: u64,
    r: i64,
}

impl Template {
    fn merge(&mut self, node: Node) {
        let text = |name| node.attribute(name).map(str::to_string);
        let number = |name| node.attribute(name).and_then(|v| v.trim().parse().ok());
        self.media = text("media").or(self.media.take());
        self.initialization = text("initialization")
            .or_else(|| {
                child(node, "Initialization")
                    .and_then(|i| i.attribute("sourceURL"))
                    .map(str::to_string)
            })
            .or(self.initialization.take());
        self.start_number = number("startNumber").or(self.start_number);
        self.end_number = number("endNumber").or(self.end_number);
        self.timescale = number("timescale").or(self.timescale);
        self.duration = number("duration").or(self.duration);
        self.presentation_time_offset =
            number("presentationTimeOffset").or(self.presentation_time_offset);
        if let Some(timeline) = child(node, "SegmentTimeline") {
            self.timeline = Some(
                children(timeline, "S")
                    .map(|s| Entry {
                        t: s.attribute("t").and_then(|v| v.trim().parse().ok()),
                        d: s.attribute("d")
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0),
                        r: s.attribute("r")
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0),
                    })
                    .collect(),
            );
        }
    }
}

impl Segments {
    /// The pieces to fetch, init section first. [`Segments::Indexed`] needs
    /// the index read first and gives `None` here.
    pub fn pieces(&self) -> Result<Option<Vec<Piece>>, String> {
        match self {
            Segments::Template {
                template,
                base,
                id,
                bandwidth,
                period,
            } => expand(template, base, id, *bandwidth, *period).map(Some),
            Segments::List(pieces) => Ok(Some(pieces.clone())),
            Segments::Indexed { .. } => Ok(None),
            Segments::Whole(url) => Ok(Some(vec![Piece::plain(url.clone(), None)])),
        }
    }

    /// Like [`Segments::pieces`], moving a resolved list instead of copying it.
    pub fn into_pieces(self) -> Result<Option<Vec<Piece>>, String> {
        match self {
            Segments::List(pieces) => Ok(Some(pieces)),
            other => other.pieces(),
        }
    }
}

fn expand(
    template: &Template,
    base: &Url,
    id: &str,
    bandwidth: u64,
    period: Option<f64>,
) -> Result<Vec<Piece>, String> {
    let timescale = template.timescale.unwrap_or(1).max(1);
    let start = template.start_number.unwrap_or(1);
    let media = template
        .media
        .as_deref()
        .ok_or("a segment template has no media address")?;
    let mut pieces = Vec::new();
    if let Some(init) = &template.initialization {
        let url = join(base, &substitute(init, id, bandwidth, None, None)?)?;
        let mut piece = Piece::plain(url, None);
        piece.init = true;
        pieces.push(piece);
    }
    let offset = template.presentation_time_offset.unwrap_or(0);
    let period_ticks = period.map(|p| (p * timescale as f64).round() as u64);
    let mut push = |number: u64, time: u64| -> Result<(), String> {
        if pieces.len() as u64 > MAX_SEGMENTS {
            return Err(too_many_segments());
        }
        let address = substitute(media, id, bandwidth, Some(number), Some(time))?;
        pieces.push(Piece::plain(join(base, &address)?, None));
        Ok(())
    };

    if let Some(timeline) = &template.timeline {
        let period_end = period_ticks.map(|p| offset.saturating_add(p));
        let mut time = 0u64;
        let mut number = start;
        for (i, entry) in timeline.iter().enumerate() {
            if let Some(t) = entry.t {
                time = t;
            }
            if entry.d == 0 {
                return Err("a segment timeline entry has no duration".into());
            }
            let count = if entry.r >= 0 {
                entry.r as u64 + 1
            } else {
                let end = timeline
                    .get(i + 1)
                    .and_then(|next| next.t)
                    .or(period_end)
                    .ok_or("the stream does not say how long it is")?;
                end.saturating_sub(time).div_ceil(entry.d)
            };
            if count > MAX_SEGMENTS {
                return Err(too_many_segments());
            }
            for _ in 0..count {
                push(number, time)?;
                time = time.checked_add(entry.d).ok_or(TOO_LARGE)?;
                number = number.checked_add(1).ok_or(TOO_LARGE)?;
            }
        }
    } else {
        let duration = template
            .duration
            .filter(|d| *d > 0)
            .ok_or("a segment template has neither a duration nor a timeline")?;
        let count = match (template.end_number, period_ticks) {
            (Some(end), _) => end.saturating_add(1).saturating_sub(start),
            (None, Some(ticks)) => ticks.div_ceil(duration),
            (None, None) => return Err("the stream does not say how long it is".into()),
        };
        if count > MAX_SEGMENTS {
            return Err(too_many_segments());
        }
        for n in 0..count {
            let number = start.checked_add(n).ok_or(TOO_LARGE)?;
            let time = n
                .checked_mul(duration)
                .and_then(|t| t.checked_add(offset))
                .ok_or(TOO_LARGE)?;
            push(number, time)?;
        }
    }
    Ok(pieces)
}

/// Fills in a segment address template: `$RepresentationID$`, `$Number$`,
/// `$Time$` and `$Bandwidth$`, each optionally with a width such as
/// `$Number%05d$`, and `$$` for a literal `$`.
pub(crate) fn substitute(
    template: &str,
    id: &str,
    bandwidth: u64,
    number: Option<u64>,
    time: Option<u64>,
) -> Result<String, String> {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after
            .find('$')
            .ok_or("a segment address has an unclosed $")?;
        let identifier = &after[..end];
        rest = &after[end + 1..];
        if identifier.is_empty() {
            out.push('$');
            continue;
        }
        let (name, format) = match identifier.split_once('%') {
            Some((name, format)) => (name, Some(format)),
            None => (identifier, None),
        };
        let value = match name {
            "RepresentationID" if format.is_none() => {
                out.push_str(id);
                continue;
            }
            "Number" => number,
            "Time" => time,
            "Bandwidth" => Some(bandwidth),
            _ => return Err(format!("a segment address uses the unknown ${identifier}$")),
        }
        .ok_or_else(|| format!("${name}$ is used where it has no value"))?;
        match format {
            None => out.push_str(&value.to_string()),
            Some(format) => {
                let width: usize = format
                    .strip_suffix('d')
                    .and_then(|w| {
                        if w.is_empty() {
                            Some(0)
                        } else {
                            w.parse().ok()
                        }
                    })
                    .filter(|w| *w <= 32)
                    .ok_or_else(|| format!("${identifier}$ has a width it cannot use"))?;
                out.push_str(&format!("{value:0width$}"));
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// Reads a manifest; relative addresses are resolved against `base` (its own
/// address after redirects).
pub(crate) fn parse(text: &str, base: &Url) -> Result<Mpd, String> {
    let doc = Document::parse(text).map_err(|e| format!("it is not valid XML ({e})"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "MPD" {
        return Err("it is not a DASH manifest".into());
    }
    let dynamic = root.attribute("type") == Some("dynamic");
    let duration = root
        .attribute("mediaPresentationDuration")
        .and_then(iso_duration);
    let mpd_base = base_url(base, root);
    let periods: Vec<Node> = children(root, "Period").collect();
    let Some(period) = periods.first().copied() else {
        return Err("it has no periods".into());
    };
    let period_base = base_url(&mpd_base, period);
    let period_length = period
        .attribute("duration")
        .and_then(iso_duration)
        .or_else(|| {
            let start = period
                .attribute("start")
                .and_then(iso_duration)
                .unwrap_or(0.0);
            duration.map(|d| (d - start).max(0.0))
        });

    let mut reps = Vec::new();
    for set in children(period, "AdaptationSet") {
        if is_trick_mode(set) {
            continue;
        }
        let first = child(set, "Representation");
        let Some(kind) = set_kind(set, first) else {
            continue;
        };
        let set_base = base_url(&period_base, set);
        let main = children(set, "Role").any(|r| r.attribute("value") == Some("main"));
        let set_drm = protection(set);
        let label = child(set, "Label")
            .and_then(|l| l.text())
            .or(set.attribute("label"))
            .map(|l| l.trim().to_string());
        for rep in children(set, "Representation") {
            let get = |name| rep.attribute(name).or_else(|| set.attribute(name));
            let id = rep.attribute("id").unwrap_or_default().to_string();
            let bandwidth = rep
                .attribute("bandwidth")
                .and_then(|b| b.trim().parse().ok());
            let rep_base = base_url(&set_base, rep);
            let segments = segments(
                [period, set, rep],
                &rep_base,
                &id,
                bandwidth.unwrap_or(0),
                period_length,
            )?;
            reps.push(Rep {
                kind,
                bandwidth,
                width: get("width").and_then(|w| w.trim().parse().ok()),
                height: get("height").and_then(|h| h.trim().parse().ok()),
                frame_rate: get("frameRate").and_then(frame_rate),
                codecs: get("codecs").map(str::to_string),
                language: set.attribute("lang").map(str::to_string),
                label: label.clone(),
                channels: child(rep, "AudioChannelConfiguration")
                    .or_else(|| child(set, "AudioChannelConfiguration"))
                    .and_then(|c| c.attribute("value"))
                    .map(str::to_string),
                main,
                drm: protection(rep).or_else(|| set_drm.clone()),
                segments,
            });
        }
    }
    Ok(Mpd {
        dynamic,
        duration,
        periods: periods.len(),
        reps,
    })
}

/// Finds a representation's segment information on its own level, its
/// adaptation set's or its period's (`levels` goes from outer to inner).
fn segments(
    levels: [Node; 3],
    base: &Url,
    id: &str,
    bandwidth: u64,
    period: Option<f64>,
) -> Result<Segments, String> {
    let templates: Vec<Node> = levels
        .iter()
        .filter_map(|l| child(*l, "SegmentTemplate"))
        .collect();
    if !templates.is_empty() {
        let mut template = Template::default();
        for node in templates {
            template.merge(node);
        }
        return Ok(Segments::Template {
            template,
            base: base.clone(),
            id: id.to_string(),
            bandwidth,
            period,
        });
    }

    let lists: Vec<Node> = levels
        .iter()
        .filter_map(|l| child(*l, "SegmentList"))
        .collect();
    if let Some(inner) = lists.last() {
        let mut pieces = Vec::new();
        if let Some(init) = lists.iter().rev().find_map(|l| child(*l, "Initialization")) {
            let url = match init.attribute("sourceURL") {
                Some(source) => join(base, source)?,
                None => base.clone(),
            };
            let range = init.attribute("range").map(byte_range).transpose()?;
            let mut piece = Piece::plain(url, range);
            piece.init = true;
            pieces.push(piece);
        }
        for segment in children(*inner, "SegmentURL") {
            if pieces.len() as u64 > MAX_SEGMENTS {
                return Err(too_many_segments());
            }
            let url = match segment.attribute("media") {
                Some(media) => join(base, media)?,
                None => base.clone(),
            };
            let range = segment
                .attribute("mediaRange")
                .map(byte_range)
                .transpose()?;
            pieces.push(Piece::plain(url, range));
        }
        return Ok(Segments::List(pieces));
    }

    let index = levels
        .iter()
        .rev()
        .filter_map(|l| child(*l, "SegmentBase"))
        .find_map(|b| b.attribute("indexRange"))
        .map(byte_range)
        .transpose()?;
    Ok(match index {
        Some(index) => Segments::Indexed {
            url: base.clone(),
            index,
        },
        None => Segments::Whole(base.clone()),
    })
}

/// Splits a file indexed by a `sidx` box into its init section (everything
/// before the first media byte, the index included) and its subsegments.
/// `data` holds the bytes of `index`. `None` when the index is not a plain
/// single-level `sidx`; the caller then fetches the whole file.
pub(crate) fn sidx_pieces(url: &Url, index: &Range<u64>, data: &[u8]) -> Option<Vec<Piece>> {
    let u32_at = |at: usize| -> Option<u64> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?).into())
    };
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
    };
    if data.get(4..8)? != b"sidx" {
        return None;
    }
    let (box_size, mut at) = match u32_at(0)? {
        1 => (u64_at(8)?, 16),
        0 => (data.len() as u64, 8),
        size => (size, 8),
    };
    let version = *data.get(at)?;
    at += 4 + 4 + 4; // version and flags, reference_ID, timescale
    let first_offset = if version == 0 {
        at += 4;
        let first = u32_at(at)?;
        at += 4;
        first
    } else {
        at += 8;
        let first = u64_at(at)?;
        at += 8;
        first
    };
    at += 2; // reserved
    let count = u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?);
    at += 2;

    let mut offset = index
        .start
        .checked_add(box_size)?
        .checked_add(first_offset)?;
    let mut init = Piece::plain(url.clone(), Some(0..offset));
    init.init = true;
    let mut pieces = vec![init];
    for _ in 0..count {
        let reference = u32_at(at)?;
        at += 12;
        if reference >> 31 == 1 {
            // A reference to another index: not handled.
            return None;
        }
        let size = reference & 0x7FFF_FFFF;
        let end = offset.checked_add(size)?;
        pieces.push(Piece::plain(url.clone(), Some(offset..end)));
        offset = end;
    }
    (pieces.len() > 1).then_some(pieces)
}

/// Reads an ISO 8601 duration such as `PT1H2M3.5S` or `P1DT2H`, in seconds.
/// Years and months (which have no fixed length) give `None` unless zero.
pub(crate) fn iso_duration(value: &str) -> Option<f64> {
    let rest = value.trim().strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    let mut total = 0.0;
    for (part, units) in [
        (
            date,
            &[
                ('Y', f64::NAN),
                ('M', f64::NAN),
                ('W', 604_800.0),
                ('D', 86_400.0),
            ][..],
        ),
        (time, &[('H', 3_600.0), ('M', 60.0), ('S', 1.0)][..]),
    ] {
        let mut number = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                number.push(c);
                continue;
            }
            let (_, seconds) = units.iter().find(|(unit, _)| *unit == c)?;
            let amount: f64 = number.parse().ok()?;
            number.clear();
            if amount != 0.0 {
                if seconds.is_nan() {
                    return None;
                }
                total += amount * seconds;
            }
        }
        if !number.is_empty() {
            return None;
        }
    }
    total.is_finite().then_some(total)
}

/// `30000/1001` or `25`.
fn frame_rate(value: &str) -> Option<f64> {
    let rate = match value.split_once('/') {
        Some((n, d)) => n.trim().parse::<f64>().ok()? / d.trim().parse::<f64>().ok()?,
        None => value.trim().parse().ok()?,
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

/// `first-last`, both inclusive.
fn byte_range(value: &str) -> Result<Range<u64>, String> {
    let bad = || format!("the byte range {value:?} is not valid");
    let (first, last) = value.trim().split_once('-').ok_or_else(bad)?;
    let first: u64 = first.trim().parse().map_err(|_| bad())?;
    let last: u64 = last.trim().parse().map_err(|_| bad())?;
    if last < first {
        return Err(bad());
    }
    let end = last.checked_add(1).ok_or_else(bad)?;
    Ok(first..end)
}

fn set_kind(set: Node, first: Option<Node>) -> Option<Kind> {
    match set.attribute("contentType") {
        Some("video") => return Some(Kind::Video),
        Some("audio") => return Some(Kind::Audio),
        Some("text") => return Some(Kind::Text),
        Some(_) => return None,
        None => {}
    }
    let mime = set
        .attribute("mimeType")
        .or_else(|| first.and_then(|r| r.attribute("mimeType")))
        .unwrap_or_default();
    let codecs = set
        .attribute("codecs")
        .or_else(|| first.and_then(|r| r.attribute("codecs")))
        .unwrap_or_default();
    if mime.starts_with("video/") {
        Some(Kind::Video)
    } else if mime.starts_with("audio/") {
        Some(Kind::Audio)
    } else if mime.starts_with("text/")
        || mime.contains("ttml")
        || codecs.starts_with("stpp")
        || codecs.starts_with("wvtt")
    {
        Some(Kind::Text)
    } else if codecs.split(',').any(|c| is_video_codec(c.trim())) {
        Some(Kind::Video)
    } else if !codecs.is_empty() && codecs.split(',').all(|c| is_audio_codec(c.trim())) {
        Some(Kind::Audio)
    } else {
        None
    }
}

fn is_trick_mode(set: Node) -> bool {
    children(set, "EssentialProperty")
        .chain(children(set, "SupplementalProperty"))
        .any(|p| {
            p.attribute("schemeIdUri")
                .is_some_and(|s| s.contains("trickmode"))
        })
}

/// The DRM a node's `ContentProtection` elements name, if it has any.
fn protection(node: Node) -> Option<String> {
    let schemes: Vec<String> = children(node, "ContentProtection")
        .map(|p| {
            p.attribute("schemeIdUri")
                .unwrap_or_default()
                .to_ascii_lowercase()
        })
        .collect();
    if schemes.is_empty() {
        return None;
    }
    let named = [
        ("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed", "Google Widevine"),
        (
            "9a04f079-9840-4286-ab92-e65be0885f95",
            "Microsoft PlayReady",
        ),
        ("94ce86fb-07ff-4f43-adb8-93d2fa968ca2", "Apple FairPlay"),
        ("1077efec-c0b2-4d02-ace3-3c1e52e2fb4b", "W3C Clear Key"),
    ];
    let name = named
        .iter()
        .find(|(id, _)| schemes.iter().any(|s| s.contains(id)))
        .map_or("Common Encryption", |(_, name)| name);
    Some(name.to_string())
}

fn base_url(parent: &Url, node: Node) -> Url {
    child(node, "BaseURL")
        .and_then(|b| b.text())
        .and_then(|t| parent.join(t.trim()).ok())
        .unwrap_or_else(|| parent.clone())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn children<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn join(base: &Url, reference: &str) -> Result<Url, String> {
    base.join(reference.trim())
        .map_err(|_| format!("it links to {reference:?}, which is not a valid address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://site.example/dash/manifest.mpd").unwrap()
    }

    fn names(pieces: &[Piece]) -> Vec<String> {
        pieces
            .iter()
            .map(|p| p.url.as_str().trim_start_matches("https://").to_string())
            .collect()
    }

    #[test]
    fn templates_fill_in_identifiers_and_widths() {
        let s = substitute(
            "$RepresentationID$/seg-$Number%05d$-$Time$-$Bandwidth$$$.m4s",
            "v1",
            800,
            Some(42),
            Some(9000),
        )
        .unwrap();
        assert_eq!(s, "v1/seg-00042-9000-800$.m4s");
        assert_eq!(
            substitute("a$Number%d$", "x", 0, Some(7), None).unwrap(),
            "a7"
        );
        assert!(substitute("$Number$", "x", 0, None, None).is_err());
        assert!(substitute("$Nope$", "x", 0, Some(1), None).is_err());
        assert!(substitute("$Number", "x", 0, Some(1), None).is_err());
        assert!(substitute("$Number%99d$", "x", 0, Some(1), None).is_err());
    }

    #[test]
    fn iso_durations() {
        assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(iso_duration("PT0.5S"), Some(0.5));
        assert_eq!(iso_duration("P1DT1S"), Some(86_401.0));
        assert_eq!(iso_duration("P0Y0M0DT10S"), Some(10.0));
        assert_eq!(iso_duration("P1M"), None);
        assert_eq!(iso_duration("PT"), Some(0.0));
        for bad in ["", "1H", "PT1X", "PT1", "PTxS"] {
            assert_eq!(iso_duration(bad), None, "{bad}");
        }
    }

    const NUMBER_MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT9.6S">
  <BaseURL>media/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4" codecs="avc1.64001f" frameRate="30000/1001">
      <SegmentTemplate timescale="1000" duration="3200" startNumber="5"
        initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/$Number%03d$.m4s"/>
      <Representation id="v720" bandwidth="2000000" width="1280" height="720"/>
      <Representation id="v360" bandwidth="600000" width="640" height="360">
        <SegmentTemplate startNumber="1"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="audio" lang="en">
      <Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
      <AudioChannelConfiguration schemeIdUri="x" value="2"/>
      <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2">
        <BaseURL>https://cdn.example/audio/</BaseURL>
        <SegmentTemplate timescale="48000" media="seg$Number$.m4s" initialization="init.mp4" duration="153600"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="image"><Representation id="thumbs" bandwidth="1"/></AdaptationSet>
  </Period>
</MPD>"#;

    #[test]
    fn number_templates_inherit_and_count_from_the_duration() {
        let mpd = parse(NUMBER_MPD, &base()).unwrap();
        assert!(!mpd.dynamic);
        assert_eq!(mpd.duration, Some(9.6));
        assert_eq!(mpd.reps.len(), 3, "the image set is left out");
        let v720 = &mpd.reps[0];
        assert_eq!(v720.kind, Kind::Video);
        assert_eq!((v720.width, v720.height), (Some(1280), Some(720)));
        assert!((v720.frame_rate.unwrap() - 29.97).abs() < 0.01);
        assert_eq!(v720.codecs.as_deref(), Some("avc1.64001f"));
        // 9.6 s in 3.2 s segments is exactly 3, not 4 from rounding errors.
        assert_eq!(
            names(&v720.segments.pieces().unwrap().unwrap()),
            [
                "site.example/dash/media/v720/init.mp4",
                "site.example/dash/media/v720/005.m4s",
                "site.example/dash/media/v720/006.m4s",
                "site.example/dash/media/v720/007.m4s"
            ]
        );
        let v360 = mpd.reps[1].segments.pieces().unwrap().unwrap();
        assert_eq!(names(&v360)[1], "site.example/dash/media/v360/001.m4s");

        let audio = &mpd.reps[2];
        assert_eq!(audio.kind, Kind::Audio);
        assert!(audio.main);
        assert_eq!(audio.language.as_deref(), Some("en"));
        assert_eq!(audio.channels.as_deref(), Some("2"));
        let pieces = audio.segments.pieces().unwrap().unwrap();
        assert_eq!(names(&pieces)[0], "cdn.example/audio/init.mp4");
        assert_eq!(names(&pieces)[3], "cdn.example/audio/seg3.m4s");
        assert!(pieces[0].init && !pieces[1].init);
    }

    #[test]
    fn timelines_give_times_and_numbers() {
        let text = r#"<MPD type="static" mediaPresentationDuration="PT10S"><Period>
          <AdaptationSet mimeType="audio/mp4">
            <Representation id="a" bandwidth="1">
              <SegmentTemplate timescale="10" presentationTimeOffset="100" media="t$Time$-n$Number$.m4s">
                <SegmentTimeline>
                  <S t="100" d="20" r="1"/>
                  <S d="30"/>
                  <S t="200" d="25" r="-1"/>
                </SegmentTimeline>
              </SegmentTemplate>
            </Representation>
          </AdaptationSet></Period></MPD>"#;
        let mpd = parse(text, &base()).unwrap();
        let pieces = mpd.reps[0].segments.pieces().unwrap().unwrap();
        // Period end is 100 + 10 s * 10 = 200 ticks... the last entry starts at
        // 200, so r=-1 repeats until the period end: none past it.
        assert_eq!(
            names(&pieces),
            [
                "site.example/dash/t100-n1.m4s",
                "site.example/dash/t120-n2.m4s",
                "site.example/dash/t140-n3.m4s",
            ]
        );
        let text = text.replace("PT10S", "PT15S");
        let mpd = parse(&text, &base()).unwrap();
        let pieces = mpd.reps[0].segments.pieces().unwrap().unwrap();
        // Now the period ends at 250: two segments of 25 ticks from 200.
        assert_eq!(
            names(&pieces)[3..],
            [
                "site.example/dash/t200-n4.m4s",
                "site.example/dash/t225-n5.m4s"
            ]
        );
    }

    #[test]
    fn segment_lists_and_bases() {
        let text = r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
          <AdaptationSet mimeType="video/mp4">
            <Representation id="list" bandwidth="1">
              <BaseURL>file.mp4</BaseURL>
              <SegmentList>
                <Initialization range="0-99"/>
                <SegmentURL mediaRange="100-199"/>
                <SegmentURL media="other.mp4" mediaRange="0-9"/>
              </SegmentList>
            </Representation>
            <Representation id="base" bandwidth="2">
              <BaseURL>indexed.mp4</BaseURL>
              <SegmentBase indexRange="700-799"><Initialization range="0-699"/></SegmentBase>
            </Representation>
            <Representation id="whole" bandwidth="3"><BaseURL>whole.webm</BaseURL></Representation>
          </AdaptationSet></Period></MPD>"#;
        let mpd = parse(text, &base()).unwrap();
        let list = mpd.reps[0].segments.pieces().unwrap().unwrap();
        assert_eq!(
            list.iter().map(|p| p.range.clone()).collect::<Vec<_>>(),
            [Some(0..100), Some(100..200), Some(0..10)]
        );
        assert_eq!(names(&list)[2], "site.example/dash/other.mp4");
        match &mpd.reps[1].segments {
            Segments::Indexed { url, index } => {
                assert_eq!(url.as_str(), "https://site.example/dash/indexed.mp4");
                assert_eq!(*index, 700..800);
            }
            other => panic!("{other:?}"),
        }
        let whole = mpd.reps[2].segments.pieces().unwrap().unwrap();
        assert_eq!(names(&whole), ["site.example/dash/whole.webm"]);
        assert_eq!(whole[0].range, None);
    }

    #[test]
    fn sidx_boxes_give_subsegment_ranges() {
        let mut sidx = Vec::new();
        let body_len = 4 + 4 + 4 + 4 + 4 + 2 + 2 + 2 * 12;
        sidx.extend_from_slice(&(8 + body_len as u32).to_be_bytes());
        sidx.extend_from_slice(b"sidx");
        sidx.extend_from_slice(&[0, 0, 0, 0]); // version 0, flags
        sidx.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
        sidx.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        sidx.extend_from_slice(&0u32.to_be_bytes()); // earliest presentation time
        sidx.extend_from_slice(&10u32.to_be_bytes()); // first_offset
        sidx.extend_from_slice(&[0, 0, 0, 2]); // reserved, reference_count
        for size in [500u32, 300] {
            sidx.extend_from_slice(&size.to_be_bytes());
            sidx.extend_from_slice(&[0; 8]);
        }
        let url = Url::parse("https://cdn.example/v.mp4").unwrap();
        let index = 700..700 + sidx.len() as u64;
        let pieces = sidx_pieces(&url, &index, &sidx).unwrap();
        let media_start = 700 + sidx.len() as u64 + 10;
        assert_eq!(
            pieces.iter().map(|p| p.range.clone()).collect::<Vec<_>>(),
            [
                Some(0..media_start),
                Some(media_start..media_start + 500),
                Some(media_start + 500..media_start + 800)
            ]
        );
        assert!(pieces[0].init);
        // Truncated or foreign data is refused, never a panic.
        assert!(sidx_pieces(&url, &index, &sidx[..30]).is_none());
        assert!(sidx_pieces(&url, &index, b"0000moof").is_none());
        assert!(sidx_pieces(&url, &index, &[]).is_none());
    }

    #[test]
    fn live_drm_and_periods_are_reported() {
        let text = r#"<MPD type="dynamic"><Period id="1"><AdaptationSet mimeType="video/mp4">
            <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/>
            <ContentProtection schemeIdUri="urn:uuid:EDEF8BA9-79D6-4ACE-A3C8-27DCD51D21ED"/>
            <Representation id="v" bandwidth="1"><SegmentTemplate media="$Number$.m4s" duration="1"/></Representation>
          </AdaptationSet></Period><Period id="2"/></MPD>"#;
        let mpd = parse(text, &base()).unwrap();
        assert!(mpd.dynamic);
        assert_eq!(mpd.periods, 2);
        assert_eq!(mpd.reps[0].drm.as_deref(), Some("Google Widevine"));
        // Without a duration, a duration-based template cannot be counted.
        assert!(mpd.reps[0].segments.pieces().is_err());
    }

    #[test]
    fn numbers_that_overflow_are_errors() {
        let max = u64::MAX;
        let mpd = |inner: &str| {
            format!(
                r#"<MPD mediaPresentationDuration="PT10S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="1">{inner}</Representation></AdaptationSet></Period></MPD>"#
            )
        };
        // startNumber + n, times past the largest number, and `last + 1`.
        for inner in [
            format!(r#"<SegmentTemplate media="$Number$" duration="1" startNumber="{max}"/>"#),
            format!(
                r#"<SegmentTemplate media="$Time$" duration="1" presentationTimeOffset="{max}"/>"#
            ),
            format!(
                r#"<SegmentTemplate media="$Time$"><SegmentTimeline><S t="{}" d="5" r="3"/></SegmentTimeline></SegmentTemplate>"#,
                max - 6
            ),
        ] {
            let parsed = parse(&mpd(&inner), &base()).unwrap();
            let error = parsed.reps[0].segments.pieces().unwrap_err();
            assert!(error.contains("too large"), "{error}");
        }
        let inner = format!(r#"<SegmentList><SegmentURL mediaRange="0-{max}"/></SegmentList>"#);
        assert!(
            parse(&mpd(&inner), &base())
                .unwrap_err()
                .contains("byte range")
        );
    }

    #[test]
    fn hostile_manifests_are_errors_not_panics() {
        for text in [
            "",
            "not xml",
            "<html/>",
            "<MPD/>",
            r#"<MPD mediaPresentationDuration="PT100000000S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentTemplate media="$Number$" duration="1"/></Representation></AdaptationSet></Period></MPD>"#,
            r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentTemplate media="$Time$"><SegmentTimeline><S d="1" r="999999999"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#,
            r#"<MPD><Period><AdaptationSet mimeType="video/mp4"><Representation id="v"><SegmentList><SegmentURL mediaRange="9-1"/></SegmentList></Representation></AdaptationSet></Period></MPD>"#,
        ] {
            let failed = match parse(text, &base()) {
                Err(_) => true,
                Ok(mpd) => {
                    mpd.reps.iter().all(|r| r.segments.pieces().is_err()) || mpd.reps.is_empty()
                }
            };
            assert!(failed, "{text}");
        }
    }
}
