//! `SegmentTemplate`: reading it over the levels it appears on, and expanding it into the addresses of a representation's segments.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::sync::Arc;

use roxmltree::Node;
use url::Url;
use wirecatch_engine::numeric::{approx_f64, f64_to_u64};

use super::{child, children, join};
use crate::error::{ManifestError, excerpt};
use crate::plan::{AddressBudget, Piece, check_room, check_room_for};

/// The widest `%0<width>d` a segment address may ask for. Wider than any number, and a hostile manifest cannot make one address huge.
const MAX_WIDTH: usize = 32;

/// A representation's `SegmentTemplate` with what it is filled in with.
#[derive(Debug)]
pub(crate) struct TemplateSegments {
    template: Template,
    base: Arc<Url>,
    id: String,
    bandwidth: u64,
    /// Length of the period, in seconds.
    period: Option<f64>,
}

/// The attributes of a `SegmentTemplate`, merged over the levels it appears on. What an outer level gives is shared by the representations inside it, not copied into each.
#[derive(Debug, Clone, Default)]
pub(super) struct Template {
    media: Option<Arc<str>>,
    initialization: Option<Arc<str>>,
    start_number: Option<u64>,
    end_number: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    presentation_time_offset: Option<u64>,
    timeline: Option<Arc<[Entry]>>,
}

/// One `S` element of a `SegmentTimeline`.
#[derive(Debug, Clone, Copy)]
struct Entry {
    t: Option<u64>,
    d: u64,
    r: i64,
}

impl Template {
    /// Takes in what `node` says, over what outer levels said.
    pub(super) fn merge(&mut self, node: Node<'_, '_>) -> Result<(), ManifestError> {
        let text = |name| node.attribute(name).map(Arc::from);
        let number = |name| attribute_number::<u64>(node, name);
        self.media = text("media").or(self.media.take());
        self.initialization = text("initialization")
            .or_else(|| {
                child(node, "Initialization")
                    .and_then(|i| i.attribute("sourceURL"))
                    .map(Arc::from)
            })
            .or(self.initialization.take());
        self.start_number = number("startNumber")?.or(self.start_number);
        self.end_number = number("endNumber")?.or(self.end_number);
        self.timescale = number("timescale")?.or(self.timescale);
        self.duration = number("duration")?.or(self.duration);
        self.presentation_time_offset =
            number("presentationTimeOffset")?.or(self.presentation_time_offset);
        if let Some(timeline) = child(node, "SegmentTimeline") {
            self.timeline = Some(
                children(timeline, "S")
                    .map(entry)
                    .collect::<Result<_, _>>()?,
            );
        }
        Ok(())
    }
}

/// One `S` element. A repeat count below -1 means nothing, so it is refused like a value that is not a number.
fn entry(s: Node<'_, '_>) -> Result<Entry, ManifestError> {
    let r = attribute_number::<i64>(s, "r")?.unwrap_or(0);
    if r < -1 {
        return Err(ManifestError::BadNumber {
            name: "r",
            value: r.to_string(),
        });
    }
    Ok(Entry {
        t: attribute_number(s, "t")?,
        d: attribute_number(s, "d")?.unwrap_or(0),
        r,
    })
}

/// The number in attribute `name` of `node`, if it has one; refused when it is not a number.
fn attribute_number<T: std::str::FromStr>(
    node: Node<'_, '_>,
    name: &'static str,
) -> Result<Option<T>, ManifestError> {
    node.attribute(name)
        .map(|value| {
            value.trim().parse().map_err(|_| ManifestError::BadNumber {
                name,
                value: excerpt(value),
            })
        })
        .transpose()
}

/// A period of `seconds` (finite and not negative, as [`iso_duration`] gives) in ticks of `timescale` per second, rounded. A period too long to count in ticks is counted as the most there can be, so its segments are too many.
fn period_ticks(seconds: f64, timescale: u64) -> u64 {
    f64_to_u64((seconds * approx_f64(timescale)).round()).unwrap_or(u64::MAX)
}

impl TemplateSegments {
    /// `template`, for the representation `id` at `bandwidth` in a period of `period` seconds, its addresses resolved against `base`.
    pub(super) fn new(
        template: Template,
        base: Arc<Url>,
        id: &str,
        bandwidth: u64,
        period: Option<f64>,
    ) -> Self {
        TemplateSegments {
            template,
            base,
            id: id.to_string(),
            bandwidth,
            period,
        }
    }

    /// The pieces to fetch, init section first.
    pub fn expand(&self) -> Result<Vec<Piece>, ManifestError> {
        expand(
            &self.template,
            &self.base,
            &self.id,
            self.bandwidth,
            self.period,
        )
    }
}

fn expand(
    template: &Template,
    base: &Url,
    id: &str,
    bandwidth: u64,
    period: Option<f64>,
) -> Result<Vec<Piece>, ManifestError> {
    let timescale = match template.timescale {
        Some(0) => return Err(ManifestError::ZeroTimescale),
        timescale => timescale.unwrap_or(1),
    };
    let media = template
        .media
        .as_deref()
        .ok_or(ManifestError::TemplateWithoutMedia)?;
    let mut track = TrackPieces {
        base,
        id,
        bandwidth,
        address: String::new(),
        budget: AddressBudget::default(),
        pieces: Vec::new(),
    };
    if let Some(init) = &template.initialization {
        track.push(init, None, true)?;
    }
    let start = template.start_number.unwrap_or(1);
    let offset = template.presentation_time_offset.unwrap_or(0);
    let period_ticks = period.map(|seconds| period_ticks(seconds, timescale));
    if let Some(timeline) = &template.timeline {
        let period_end = period_ticks.map(|p| offset.saturating_add(p));
        timeline_pieces(&mut track, media, timeline, start, period_end)?;
    } else {
        let duration = template
            .duration
            .filter(|d| *d > 0)
            .ok_or(ManifestError::TemplateWithoutDuration)?;
        let count = match (template.end_number, period_ticks) {
            (Some(end), _) => end.saturating_add(1).saturating_sub(start),
            (None, Some(ticks)) => ticks.div_ceil(duration),
            (None, None) => return Err(ManifestError::UnknownLength),
        };
        check_room_for(&track.pieces, count)?;
        for n in 0..count {
            let number = start.checked_add(n).ok_or(ManifestError::NumberTooLarge)?;
            let time = n
                .checked_mul(duration)
                .and_then(|t| t.checked_add(offset))
                .ok_or(ManifestError::NumberTooLarge)?;
            track.push(media, Some((number, time)), false)?;
        }
    }
    Ok(track.pieces)
}

/// A template's pieces as they are filled in: one address buffer serves every piece, and the address text is counted against the track's budget.
struct TrackPieces<'a> {
    base: &'a Url,
    id: &'a str,
    bandwidth: u64,
    address: String,
    budget: AddressBudget,
    pieces: Vec<Piece>,
}

impl TrackPieces<'_> {
    /// Adds the piece `template` names, for a segment's number and time or, without them, the init section.
    fn push(
        &mut self,
        template: &str,
        segment: Option<(u64, u64)>,
        init: bool,
    ) -> Result<(), ManifestError> {
        check_room(&self.pieces)?;
        self.address.clear();
        let (number, time) = segment.unzip();
        substitute_into(
            &mut self.address,
            template,
            self.id,
            self.bandwidth,
            number,
            time,
        )?;
        let url = join(self.base, &self.address)?;
        self.budget.spend(&url)?;
        let mut piece = Piece::plain(url, None);
        piece.init = init;
        self.pieces.push(piece);
        Ok(())
    }
}

/// Adds the segments a `SegmentTimeline` lists, numbered from `start`.
fn timeline_pieces(
    track: &mut TrackPieces<'_>,
    media: &str,
    timeline: &[Entry],
    start: u64,
    period_end: Option<u64>,
) -> Result<(), ManifestError> {
    let mut time = 0u64;
    let mut number = start;
    for (i, entry) in timeline.iter().enumerate() {
        if let Some(t) = entry.t {
            if t < time {
                return Err(ManifestError::TimeGoesBack);
            }
            time = t;
        }
        if entry.d == 0 {
            return Err(ManifestError::TimelineEntryWithoutDuration);
        }
        // A negative repeat count means "until the next entry's time, or the end of the period".
        let count = if let Ok(repeats) = u64::try_from(entry.r) {
            repeats + 1
        } else {
            let end = timeline
                .get(i + 1)
                .and_then(|next| next.t)
                .or(period_end)
                .ok_or(ManifestError::UnknownLength)?;
            end.saturating_sub(time).div_ceil(entry.d)
        };
        check_room_for(&track.pieces, count)?;
        for _ in 0..count {
            track.push(media, Some((number, time)), false)?;
            time = time
                .checked_add(entry.d)
                .ok_or(ManifestError::NumberTooLarge)?;
            number = number.checked_add(1).ok_or(ManifestError::NumberTooLarge)?;
        }
    }
    Ok(())
}

/// Fills in a segment address template: `$RepresentationID$`, `$Number$`, `$Time$` and `$Bandwidth$`, each optionally with a width such as `$Number%05d$`, and `$$` for a literal `$`. The result is appended to `out`, so one buffer serves every segment of a track.
pub(crate) fn substitute_into(
    out: &mut String,
    template: &str,
    id: &str,
    bandwidth: u64,
    number: Option<u64>,
    time: Option<u64>,
) -> Result<(), ManifestError> {
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('$').ok_or(ManifestError::UnclosedTemplate)?;
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
            _ => return Err(ManifestError::UnknownIdentifier(excerpt(identifier))),
        }
        .ok_or_else(|| ManifestError::IdentifierWithoutValue(name.to_string()))?;
        // No width writes the number as it is, as a width of zero does.
        let width: usize = match format {
            None => 0,
            Some(format) => format
                .strip_suffix('d')
                .and_then(|w| {
                    if w.is_empty() {
                        Some(0)
                    } else {
                        w.parse().ok()
                    }
                })
                .filter(|w| *w <= MAX_WIDTH)
                .ok_or_else(|| ManifestError::BadWidth(excerpt(identifier)))?,
        };
        write_number(out, value, width);
    }
    out.push_str(rest);
    Ok(())
}

/// Appends `value` padded with zeros to `width` digits.
fn write_number(out: &mut String, value: u64, width: usize) {
    writing_to_a_string_cannot_fail(write!(out, "{value:0width$}"));
}

/// `fmt::Write` for `String` never fails: it only appends to memory.
fn writing_to_a_string_cannot_fail(written: std::fmt::Result) {
    let (Ok(()) | Err(_)) = written;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn substitute(
        template: &str,
        id: &str,
        bandwidth: u64,
        number: Option<u64>,
        time: Option<u64>,
    ) -> Result<String, ManifestError> {
        let mut out = String::new();
        substitute_into(&mut out, template, id, bandwidth, number, time).map(|()| out)
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
        assert_eq!(
            substitute("$Number$", "x", 0, None, None),
            Err(ManifestError::IdentifierWithoutValue("Number".into()))
        );
        assert_eq!(
            substitute("$Nope$", "x", 0, Some(1), None),
            Err(ManifestError::UnknownIdentifier("Nope".into()))
        );
        assert_eq!(
            substitute("$Number", "x", 0, Some(1), None),
            Err(ManifestError::UnclosedTemplate)
        );
        assert_eq!(
            substitute("$Number%99d$", "x", 0, Some(1), None),
            Err(ManifestError::BadWidth("Number%99d".into()))
        );
    }

    #[test]
    fn period_lengths_are_counted_in_ticks() {
        assert_eq!(period_ticks(9.6, 1000), 9600);
        assert_eq!(period_ticks(0.0, 48_000), 0);
        // Rounded to the nearest tick, not cut off.
        assert_eq!(period_ticks(1.000_01, 90_000), 90_001);
        assert_eq!(period_ticks(1e30, 1000), u64::MAX);
        assert_eq!(period_ticks(f64::MAX, u64::MAX), u64::MAX);
    }
}
