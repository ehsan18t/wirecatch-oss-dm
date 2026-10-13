//! MPEG-DASH manifests (ISO/IEC 23009-1): representations and their segments.
//!
//! Supported: one period; `SegmentTemplate` with `$RepresentationID$`, `$Number$`, `$Time$` and `$Bandwidth$` (with `%0Nd` widths), either duration based or with a `SegmentTimeline` (including `r="-1"`); `SegmentList`; `SegmentBase` with an `indexRange` (read through the file's `sidx` box) or without one (the whole file at once). Attributes and segment information are inherited from the period and adaptation set as the standard says. Several periods are reported as unsupported.

#![forbid(unsafe_code)]

mod list;
mod sidx;
mod template;

pub(crate) use list::ListSegments;
pub(crate) use sidx::sidx_pieces;
pub(crate) use template::TemplateSegments;

use std::ops::Range;
use std::sync::Arc;

use roxmltree::{Document, Node};
use url::Url;

use crate::codec::Codec;
use crate::error::{DrmSystem, Located, ManifestError, excerpt};
use crate::plan::{MAX_REPRESENTATIONS, ManifestBudget};
use template::Template;

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

/// One representation. The names it takes from its adaptation set are shared with the set's other representations.
#[derive(Debug)]
pub(crate) struct Rep {
    /// `Representation@id`, unique within its period; empty when the manifest leaves it out.
    pub id: String,
    pub kind: Kind,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub codecs: Option<Arc<str>>,
    pub mime: Option<Arc<str>>,
    pub language: Option<Arc<str>>,
    pub label: Option<Arc<str>>,
    pub channels: Option<Arc<str>>,
    /// The adaptation set says it is the main one (`Role value="main"`).
    pub main: bool,
    /// Set when the representation is encrypted (`ContentProtection`).
    pub drm: Option<DrmSystem>,
    pub segments: Segments,
}

/// Where a representation's bytes are.
#[derive(Debug)]
pub(crate) enum Segments {
    /// A `SegmentTemplate`, expanded on demand with [`TemplateSegments::expand`].
    Template(TemplateSegments),
    /// A `SegmentList`, resolved on demand with [`ListSegments::expand`].
    List(ListSegments),
    /// A `SegmentBase` with an index: the segment ranges are in the file's `sidx` box at `index`.
    Indexed { url: Arc<Url>, index: Range<u64> },
    /// One file holding everything.
    Whole(Arc<Url>),
}

/// Reads a manifest; relative addresses are resolved against `base` (its own address after redirects). The error says on which line, when the manifest is not XML.
pub(crate) fn parse(text: &str, base: &Url) -> Result<Mpd, Located> {
    let doc = Document::parse(text).map_err(|e| {
        let line = usize::try_from(e.pos().row).unwrap_or(usize::MAX);
        ManifestError::NotXml(e).at(line)
    })?;
    read(&doc, base).map_err(Located::from)
}

/// Reads a parsed manifest.
fn read(doc: &Document<'_>, base: &Url) -> Result<Mpd, ManifestError> {
    let root = doc.root_element();
    if root.tag_name().name() != "MPD" {
        return Err(ManifestError::NotMpd);
    }
    let dynamic = root.attribute("type") == Some("dynamic");
    let duration = root
        .attribute("mediaPresentationDuration")
        .and_then(iso_duration);
    let mpd_base = base_url(&Arc::new(base.clone()), root)?;
    let periods: Vec<Node<'_, '_>> = children(root, "Period").collect();
    let Some(period) = periods.first().copied() else {
        return Err(ManifestError::NoPeriods);
    };
    let period_base = base_url(&mpd_base, period)?;
    let period_length = period_length(period, duration);
    let period_segments = Inherited::default().within(period)?;

    let mut reps = Vec::new();
    let mut manifest = ManifestBudget::default();
    for set in children(period, "AdaptationSet") {
        let Some(set) = SetLevel::read(set, &period_base, &period_segments)? else {
            continue;
        };
        for rep in children(set.node, "Representation") {
            if reps.len() == MAX_REPRESENTATIONS {
                return Err(ManifestError::TooManyRepresentations);
            }
            reps.push(representation(rep, &set, period_length, &mut manifest)?);
        }
    }
    Ok(Mpd {
        dynamic,
        duration,
        periods: periods.len(),
        reps,
    })
}

/// What an adaptation set gives each of its representations, looked up once for all of them: a lookup per representation walks past every other one.
struct SetLevel<'a, 'input> {
    node: Node<'a, 'input>,
    kind: Kind,
    base: Arc<Url>,
    segments: Inherited<'a>,
    codecs: Option<Arc<str>>,
    mime: Option<Arc<str>>,
    language: Option<Arc<str>>,
    label: Option<Arc<str>>,
    channels: Option<Arc<str>>,
    main: bool,
    drm: Option<DrmSystem>,
}

impl<'a, 'input> SetLevel<'a, 'input> {
    /// The adaptation set `set` of a period whose base address is `base` and whose segment information is `period`; `None` for a set that is skipped (trick mode, or not video, audio or text).
    fn read(
        set: Node<'a, 'input>,
        base: &Arc<Url>,
        period: &Inherited<'a>,
    ) -> Result<Option<Self>, ManifestError> {
        if is_trick_mode(set) {
            return Ok(None);
        }
        let Some(kind) = set_kind(set, child(set, "Representation")) else {
            return Ok(None);
        };
        let shared = |name| set.attribute(name).map(Arc::from);
        Ok(Some(SetLevel {
            node: set,
            kind,
            base: base_url(base, set)?,
            segments: period.within(set)?,
            codecs: shared("codecs"),
            mime: shared("mimeType"),
            language: shared("lang"),
            label: child(set, "Label")
                .and_then(|l| l.text())
                .or(set.attribute("label"))
                .map(|l| Arc::from(l.trim())),
            channels: channels(set),
            main: children(set, "Role").any(|r| r.attribute("value") == Some("main")),
            drm: protection(set),
        }))
    }
}

/// The representation `rep` of the adaptation set `set`, in a period of `period` seconds, counting what it keeps against the whole manifest's `manifest`.
fn representation(
    rep: Node<'_, '_>,
    set: &SetLevel<'_, '_>,
    period: Option<f64>,
    manifest: &mut ManifestBudget,
) -> Result<Rep, ManifestError> {
    let get = |name| rep.attribute(name).or_else(|| set.node.attribute(name));
    let own_or = |name, inherited: &Option<Arc<str>>| {
        rep.attribute(name)
            .map(Arc::from)
            .or_else(|| inherited.clone())
    };
    let id = rep.attribute("id").unwrap_or_default().to_string();
    let bandwidth = rep
        .attribute("bandwidth")
        .and_then(|b| b.trim().parse().ok());
    let base = base_url(&set.base, rep)?;
    let segments = set
        .segments
        .within(rep)?
        .segments(base, &id, bandwidth.unwrap_or(0), period)?;
    let rep = Rep {
        kind: set.kind,
        bandwidth,
        width: get("width").and_then(|w| w.trim().parse().ok()),
        height: get("height").and_then(|h| h.trim().parse().ok()),
        frame_rate: get("frameRate").and_then(frame_rate),
        codecs: own_or("codecs", &set.codecs),
        mime: own_or("mimeType", &set.mime),
        language: set.language.clone(),
        label: set.label.clone(),
        channels: channels(rep).or_else(|| set.channels.clone()),
        main: set.main,
        drm: protection(rep).or(set.drm),
        segments,
        id,
    };
    // A name shared with the set is still copied out for every representation when the manifest is reported.
    let names = [
        &rep.codecs,
        &rep.mime,
        &rep.language,
        &rep.label,
        &rep.channels,
    ]
    .into_iter()
    .flatten()
    .map(|name| name.len())
    .sum::<usize>();
    manifest.names(rep.id.len().saturating_add(names))?;
    Ok(rep)
}

/// A node's `AudioChannelConfiguration` value.
fn channels(node: Node<'_, '_>) -> Option<Arc<str>> {
    child(node, "AudioChannelConfiguration")
        .and_then(|c| c.attribute("value"))
        .map(Arc::from)
}

/// How long `period` lasts, in seconds: its own `duration`, or what the presentation's `duration` leaves after the period's `start`.
fn period_length(period: Node<'_, '_>, presentation: Option<f64>) -> Option<f64> {
    period
        .attribute("duration")
        .and_then(iso_duration)
        .or_else(|| {
            let start = period
                .attribute("start")
                .and_then(iso_duration)
                .unwrap_or(0.0);
            presentation.map(|d| (d - start).max(0.0))
        })
}

/// The segment information a representation inherits from the levels around it, innermost last, as the standard says. Read once per level; what it holds is shared, not copied, by the levels inside.
#[derive(Clone, Default)]
struct Inherited<'a> {
    /// The `SegmentTemplate` attributes merged so far, once a level has one.
    template: Option<Template>,
    /// The entries of the innermost `SegmentList` so far.
    list: Option<Arc<[list::Entry]>>,
    /// The innermost `Initialization` of a `SegmentList` so far.
    list_init: Option<Arc<list::Entry>>,
    /// The innermost `SegmentBase@indexRange` so far.
    index: Option<&'a str>,
}

impl<'a> Inherited<'a> {
    /// What the level `node` says, over what the outer levels said.
    fn within(&self, node: Node<'a, '_>) -> Result<Self, ManifestError> {
        let mut inner = self.clone();
        if let Some(template) = child(node, "SegmentTemplate") {
            inner.template.get_or_insert_default().merge(template)?;
        }
        if let Some(list) = child(node, "SegmentList") {
            inner.list = Some(list::Entry::segments(list)?);
            if let Some(init) = child(list, "Initialization") {
                inner.list_init = Some(Arc::new(list::Entry::read(init, "sourceURL", "range")?));
            }
        }
        if let Some(index) = child(node, "SegmentBase").and_then(|b| b.attribute("indexRange")) {
            inner.index = Some(index);
        }
        Ok(inner)
    }

    /// Where the representation `id` at `bandwidth`, whose base address is `base`, finds its bytes: a template before a list before a base.
    fn segments(
        self,
        base: Arc<Url>,
        id: &str,
        bandwidth: u64,
        period: Option<f64>,
    ) -> Result<Segments, ManifestError> {
        if let Some(template) = self.template {
            return Ok(Segments::Template(TemplateSegments::new(
                template, base, id, bandwidth, period,
            )));
        }
        if let Some(segments) = self.list {
            return Ok(Segments::List(ListSegments::new(
                base,
                self.list_init,
                segments,
            )));
        }
        Ok(match self.index.map(byte_range).transpose()? {
            Some(index) => Segments::Indexed { url: base, index },
            None => Segments::Whole(base),
        })
    }
}

/// Reads an ISO 8601 duration such as `PT1H2M3.5S` or `P1DT2H`, in seconds. Years and months (which have no fixed length) give `None` unless zero.
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
fn byte_range(value: &str) -> Result<Range<u64>, ManifestError> {
    let bad = || ManifestError::BadByteRange(excerpt(value));
    let (first, last) = value.trim().split_once('-').ok_or_else(bad)?;
    let first: u64 = first.trim().parse().map_err(|_| bad())?;
    let last: u64 = last.trim().parse().map_err(|_| bad())?;
    if last < first {
        return Err(bad());
    }
    let end = last.checked_add(1).ok_or_else(bad)?;
    Ok(first..end)
}

fn set_kind(set: Node<'_, '_>, first: Option<Node<'_, '_>>) -> Option<Kind> {
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
    } else if Codec::any_video(codecs) {
        Some(Kind::Video)
    } else if Codec::all_audio(codecs) {
        Some(Kind::Audio)
    } else {
        None
    }
}

fn is_trick_mode(set: Node<'_, '_>) -> bool {
    children(set, "EssentialProperty")
        .chain(children(set, "SupplementalProperty"))
        .any(|p| {
            p.attribute("schemeIdUri")
                .is_some_and(|s| s.contains("trickmode"))
        })
}

/// The DRM a node's `ContentProtection` elements name, if it has any: the first system named, else Common Encryption.
fn protection(node: Node<'_, '_>) -> Option<DrmSystem> {
    let mut schemes = children(node, "ContentProtection")
        .map(|p| p.attribute("schemeIdUri").unwrap_or_default())
        .peekable();
    schemes.peek()?;
    Some(
        schemes
            .find_map(DrmSystem::named_by)
            .unwrap_or(DrmSystem::CommonEncryption),
    )
}

/// The address `node`'s `BaseURL` gives, resolved against `parent`; `parent` itself, shared, when it has none.
fn base_url(parent: &Arc<Url>, node: Node<'_, '_>) -> Result<Arc<Url>, ManifestError> {
    match child(node, "BaseURL").and_then(|b| b.text()) {
        Some(text) => join(parent, text).map(Arc::new),
        None => Ok(Arc::clone(parent)),
    }
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    elements(node).find(|c| c.tag_name().name() == name)
}

fn children<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    elements(node).filter(move |c| c.tag_name().name() == name)
}

/// The elements straight inside `node`. Every lookup goes through here, so the tests can count the nodes reading a manifest looks at.
fn elements<'a, 'input: 'a>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children()
        .inspect(|_| looked_at())
        .filter(Node::is_element)
}

#[cfg(test)]
thread_local! {
    static LOOKED_AT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn looked_at() {
    LOOKED_AT.with(|n| n.set(n.get() + 1));
}

#[cfg(not(test))]
fn looked_at() {}

fn join(base: &Url, reference: &str) -> Result<Url, ManifestError> {
    crate::plan::web_address(base, reference.trim())
}

#[cfg(test)]
mod tests;
