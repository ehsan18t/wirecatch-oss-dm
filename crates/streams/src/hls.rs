//! HLS playlists (RFC 8216): master playlists with their variants and renditions, and media playlists turned into the list of pieces to fetch.
//!
//! Tags are read in order, so the rules that depend on order hold: an `EXT-X-KEY` applies to the segments and `EXT-X-MAP` sections after it, an `EXT-X-BYTERANGE` without an offset continues where the previous segment of the same file ended, and a segment without an explicit IV uses its media sequence number.

#![forbid(unsafe_code)]

use std::ops::Range;
use std::sync::Arc;

use url::Url;

use crate::codec::Codec;
use crate::error::{DrmSystem, Feature, Located, ManifestError, excerpt};
use crate::plan::{
    AddressBudget, Encryption, ManifestBudget, Piece, Refusal, check_room_for, check_web_address,
};

/// The longest a segment may say it lasts (`#EXTINF`): a day. No real segment comes near it; a larger value comes from a broken or hostile playlist and counts as unknown, so it never shows a video billions of years long or skews the size estimate.
const MAX_SEGMENT_SECONDS: f64 = 24.0 * 60.0 * 60.0;

/// A parsed playlist.
#[derive(Debug)]
pub(crate) enum Playlist {
    Master(Master),
    Media(Media),
}

/// A master playlist: the qualities and the extra renditions.
#[derive(Debug, Default)]
pub(crate) struct Master {
    pub variants: Vec<Variant>,
    pub renditions: Vec<Rendition>,
}

/// One `EXT-X-STREAM-INF` entry.
#[derive(Debug, Clone)]
pub(crate) struct Variant {
    pub url: Url,
    pub bandwidth: Option<u64>,
    pub codecs: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub frame_rate: Option<f64>,
    pub audio_group: Option<String>,
}

impl Variant {
    /// Whether the variant carries only audio, judging by its codecs.
    pub fn audio_only(&self) -> bool {
        self.resolution.is_none() && self.codecs.as_deref().is_some_and(Codec::all_audio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenditionKind {
    Audio,
    Video,
    Subtitles,
    ClosedCaptions,
}

/// One `EXT-X-MEDIA` entry.
#[derive(Debug, Clone)]
pub(crate) struct Rendition {
    pub kind: RenditionKind,
    /// `None` when the rendition is inside the variant's own stream.
    pub url: Option<Url>,
    pub group: String,
    pub language: Option<String>,
    pub name: Option<String>,
    pub default: bool,
    pub autoselect: bool,
    pub channels: Option<String>,
}

/// A media playlist, ready to download.
#[derive(Debug, Default)]
pub(crate) struct Media {
    /// Init sections and segments, in the order they are joined.
    pub pieces: Vec<Piece>,
    /// Total of the segment durations, in seconds.
    pub duration: f64,
    /// `EXT-X-ENDLIST` or `EXT-X-PLAYLIST-TYPE:VOD`: the playlist is complete.
    pub ended: bool,
    /// Set when a segment is encrypted in a way this crate cannot undo.
    pub refusal: Option<Refusal>,
}

/// Reads a playlist; relative addresses in it are resolved against `base` (the playlist's own address after redirects). The error says on which line, counting from 1, when one line is to blame.
pub(crate) fn parse(text: &str, base: &Url) -> Result<Playlist, Located> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.trim_start_matches('\u{feff}').trim()))
        .filter(|(_, l)| !l.is_empty());
    if lines.next().map(|(_, l)| l) != Some("#EXTM3U") {
        return Err(ManifestError::NotM3u.into());
    }
    let lines: Vec<(usize, &str)> = lines.collect();
    if lines
        .iter()
        .any(|(_, l)| l.starts_with("#EXT-X-STREAM-INF"))
    {
        parse_master(&lines, base).map(Playlist::Master)
    } else {
        parse_media(&lines, base).map(Playlist::Media)
    }
}

fn parse_master(lines: &[(usize, &str)], base: &Url) -> Result<Master, Located> {
    let mut reader = MasterReader::default();
    for &(number, line) in lines {
        reader.line(line, base).map_err(|cause| cause.at(number))?;
    }
    let master = reader.master;
    if master.variants.is_empty() {
        return Err(ManifestError::NoVariants.into());
    }
    Ok(master)
}

/// A master playlist being read, line by line.
#[derive(Default)]
struct MasterReader {
    master: Master,
    /// The attributes of a quality whose address line comes next.
    pending: Option<Vec<(String, String)>>,
    budget: ManifestBudget,
}

impl MasterReader {
    /// Reads one line, counting each quality and rendition against the playlist's budget.
    fn line(&mut self, line: &str, base: &Url) -> Result<(), ManifestError> {
        let Self {
            master,
            pending,
            budget,
        } = self;
        if let Some((name, value)) = tag(line) {
            match name {
                "#EXT-X-STREAM-INF" => *pending = Some(attributes(value)),
                "#EXT-X-MEDIA" => {
                    if let Some(rendition) = rendition(&attributes(value), base)? {
                        match &rendition.url {
                            Some(url) => budget.spend(url)?,
                            None => budget.count(1)?,
                        }
                        master.renditions.push(rendition);
                    }
                }
                _ => {}
            }
        } else if !line.starts_with('#')
            && let Some(attrs) = pending.take()
        {
            let url = join(base, line)?;
            budget.spend(&url)?;
            master.variants.push(Variant {
                url,
                bandwidth: number(&attrs, "BANDWIDTH"),
                codecs: attr(&attrs, "CODECS").map(str::to_string),
                resolution: attr(&attrs, "RESOLUTION").and_then(|r| {
                    let (w, h) = r.split_once(['x', 'X'])?;
                    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
                }),
                frame_rate: attr(&attrs, "FRAME-RATE")
                    .and_then(|f| f.parse().ok())
                    .filter(|f: &f64| f.is_finite() && *f > 0.0),
                audio_group: attr(&attrs, "AUDIO").map(str::to_string),
            });
        }
        Ok(())
    }
}

fn rendition(attrs: &[(String, String)], base: &Url) -> Result<Option<Rendition>, ManifestError> {
    let kind = match attr(attrs, "TYPE") {
        Some("AUDIO") => RenditionKind::Audio,
        Some("VIDEO") => RenditionKind::Video,
        Some("SUBTITLES") => RenditionKind::Subtitles,
        Some("CLOSED-CAPTIONS") => RenditionKind::ClosedCaptions,
        _ => return Ok(None),
    };
    Ok(Some(Rendition {
        kind,
        url: attr(attrs, "URI").map(|u| join(base, u)).transpose()?,
        group: attr(attrs, "GROUP-ID").unwrap_or_default().to_string(),
        language: attr(attrs, "LANGUAGE").map(str::to_string),
        name: attr(attrs, "NAME").map(str::to_string),
        default: attr(attrs, "DEFAULT") == Some("YES"),
        autoselect: attr(attrs, "AUTOSELECT") == Some("YES"),
        channels: attr(attrs, "CHANNELS").map(str::to_string),
    }))
}

/// How an `EXT-X-KEY` encrypts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyMethod {
    /// `AES-128`: whole segments in AES-128 CBC, which this crate decrypts.
    Aes128,
    /// Any other method, such as `SAMPLE-AES`, as the playlist names it.
    Other(String),
}

impl KeyMethod {
    /// From `METHOD`; `None` for `NONE`, which ends encryption.
    fn parse(value: &str) -> Option<KeyMethod> {
        match value {
            "NONE" => None,
            "AES-128" => Some(KeyMethod::Aes128),
            other => Some(KeyMethod::Other(other.to_string())),
        }
    }
}

/// The `KEYFORMAT` of an `EXT-X-KEY`: whose key it is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyFormat {
    /// `identity`, the default: a plain key at the key's address.
    Identity,
    /// A DRM system's key format, as the playlist names it.
    Other(String),
}

impl KeyFormat {
    fn parse(value: Option<&str>) -> KeyFormat {
        match value {
            None | Some("identity") => KeyFormat::Identity,
            Some(other) => KeyFormat::Other(other.to_string()),
        }
    }
}

/// One `EXT-X-KEY` in force.
#[derive(Debug, Clone)]
struct Key {
    method: KeyMethod,
    format: KeyFormat,
    /// Shared by every segment the key encrypts.
    url: Option<Arc<Url>>,
    iv: Option<[u8; 16]>,
}

/// An `EXT-X-MAP` in force, with the key that applied where it appeared.
#[derive(Debug, Clone)]
struct Map {
    url: Url,
    range: Option<Range<u64>>,
    key: Option<Key>,
}

fn parse_media(lines: &[(usize, &str)], base: &Url) -> Result<Media, Located> {
    let mut reader = MediaReader::new(base);
    for &(number, line) in lines {
        let read = if let Some((name, value)) = tag(line) {
            reader.tag(name, value)
        } else if !line.starts_with('#') {
            reader.segment(line)
        } else {
            Ok(())
        };
        read.map_err(|cause| cause.at(number))?;
    }
    if reader.index == 0 {
        return Err(ManifestError::NoSegments.into());
    }
    Ok(reader.media)
}

/// A media playlist being read, line by line: what is read so far and what the tags in force say about the next segment.
struct MediaReader<'a> {
    base: &'a Url,
    media: Media,
    media_sequence: u64,
    /// Segments read so far.
    index: u64,
    keys: Vec<Key>,
    /// The init section in force, numbered: a new number for every init section that differs from the one before.
    map: Option<(u64, Map)>,
    /// The number of the last init section added, so each one is added once.
    emitted_map: Option<u64>,
    /// An `EXT-X-BYTERANGE` for the next segment: its length and offset.
    pending_range: Option<(u64, Option<u64>)>,
    /// Where the last segment's byte range ended, and that segment's place in the pieces (whose address it shares).
    previous_end: Option<(usize, u64)>,
    /// The `EXTINF` duration of the next segment.
    pending_duration: f64,
    /// The address text of the pieces so far.
    budget: AddressBudget,
}

impl<'a> MediaReader<'a> {
    fn new(base: &'a Url) -> Self {
        Self {
            base,
            media: Media::default(),
            media_sequence: 0,
            index: 0,
            keys: Vec::new(),
            map: None,
            emitted_map: None,
            pending_range: None,
            previous_end: None,
            pending_duration: 0.0,
            budget: AddressBudget::default(),
        }
    }

    fn tag(&mut self, name: &str, value: &str) -> Result<(), ManifestError> {
        match name {
            "#EXT-X-MEDIA-SEQUENCE" => {
                self.media_sequence = value
                    .trim()
                    .parse()
                    .map_err(|_| ManifestError::BadMediaSequence(excerpt(value)))?;
            }
            "#EXTINF" => {
                self.pending_duration = value
                    .split(',')
                    .next()
                    .and_then(|d| d.trim().parse::<f64>().ok())
                    .filter(|d| (0.0..=MAX_SEGMENT_SECONDS).contains(d))
                    .unwrap_or(0.0);
            }
            "#EXT-X-BYTERANGE" => self.pending_range = Some(byte_range(value)?),
            "#EXT-X-KEY" => update_keys(&mut self.keys, &attributes(value), self.base)?,
            "#EXT-X-MAP" => {
                let map = self.read_map(value)?;
                let number = match &self.map {
                    Some((n, current)) if current.url == map.url && current.range == map.range => {
                        *n
                    }
                    Some((n, _)) => n.saturating_add(1),
                    None => 0,
                };
                self.map = Some((number, map));
            }
            "#EXT-X-ENDLIST" => self.media.ended = true,
            "#EXT-X-PLAYLIST-TYPE" if value.trim() == "VOD" => self.media.ended = true,
            _ => {}
        }
        Ok(())
    }

    /// An `EXT-X-MAP`, encrypted with the key in force where it appears.
    fn read_map(&self, value: &str) -> Result<Map, ManifestError> {
        let attrs = attributes(value);
        let url = attr(&attrs, "URI")
            .ok_or(ManifestError::MapWithoutUri)
            .and_then(|u| join(self.base, u))?;
        let range = attr(&attrs, "BYTERANGE")
            .map(byte_range)
            .transpose()?
            .map(|(len, offset)| span(offset.unwrap_or(0), len))
            .transpose()?;
        Ok(Map {
            url,
            range,
            key: usable_key(&self.keys).cloned(),
        })
    }

    /// A segment's address line: the segment, after its init section when it is the first to use that one.
    fn segment(&mut self, line: &str) -> Result<(), ManifestError> {
        // Checked before anything is resolved or stored, so a huge playlist fails at once instead of filling memory first. A new init section goes in before the segment, so both need room.
        let new_init = self
            .map
            .as_ref()
            .is_some_and(|(number, _)| self.emitted_map != Some(*number));
        check_room_for(&self.media.pieces, 1 + u64::from(new_init))?;
        let url = join(self.base, line)?;
        self.budget.spend(&url)?;
        let sequence = self
            .media_sequence
            .checked_add(self.index)
            .ok_or(ManifestError::NumberTooLarge)?;
        self.init_section(sequence)?;
        let range = self.segment_range(&url)?;
        self.previous_end = range.as_ref().map(|r| (self.media.pieces.len(), r.end));
        let encryption = if let Some(key) = usable_key(&self.keys) {
            Some(encryption(key, sequence)?)
        } else {
            if let Some(key) = self.keys.first()
                && self.media.refusal.is_none()
            {
                self.media.refusal = Some(refusal(key));
            }
            None
        };
        self.media.pieces.push(Piece {
            url,
            range,
            encryption,
            init: false,
        });
        self.media.duration += self.pending_duration;
        self.pending_duration = 0.0;
        self.index += 1;
        Ok(())
    }

    /// Adds the init section in force, unless the segment before used it too.
    fn init_section(&mut self, sequence: u64) -> Result<(), ManifestError> {
        let Some((number, map)) = &self.map else {
            return Ok(());
        };
        if self.emitted_map != Some(*number) {
            self.budget.spend(&map.url)?;
            self.media.pieces.push(Piece {
                url: map.url.clone(),
                range: map.range.clone(),
                encryption: map
                    .key
                    .as_ref()
                    .map(|k| encryption(k, sequence))
                    .transpose()?,
                init: true,
            });
            self.emitted_map = Some(*number);
        }
        Ok(())
    }

    /// The byte range of the segment at `url`: the pending `EXT-X-BYTERANGE`, which without an offset continues the previous part of the same file.
    fn segment_range(&mut self, url: &Url) -> Result<Option<Range<u64>>, ManifestError> {
        match self.pending_range.take() {
            None => Ok(None),
            Some((len, Some(offset))) => Ok(Some(span(offset, len)?)),
            Some((len, None)) => match &self.previous_end {
                Some((previous, end))
                    if self
                        .media
                        .pieces
                        .get(*previous)
                        .is_some_and(|p| p.url == *url) =>
                {
                    Ok(Some(span(*end, len)?))
                }
                _ => Err(ManifestError::ByteRangeWithoutStart),
            },
        }
    }
}

fn update_keys(
    keys: &mut Vec<Key>,
    attrs: &[(String, String)],
    base: &Url,
) -> Result<(), ManifestError> {
    let Some(method) = KeyMethod::parse(attr(attrs, "METHOD").unwrap_or("NONE")) else {
        keys.clear();
        return Ok(());
    };
    let format = KeyFormat::parse(attr(attrs, "KEYFORMAT"));
    // A key address that is not a URL at all (such as `skd://…` for FairPlay) is kept as missing: it is never fetched, only reported.
    let url = attr(attrs, "URI")
        .and_then(|u| base.join(u).ok())
        .map(Arc::new);
    let iv = attr(attrs, "IV").map(parse_iv).transpose()?;
    keys.retain(|k| k.format != format);
    keys.push(Key {
        method,
        format,
        url,
        iv,
    });
    Ok(())
}

/// The key this crate can decrypt with: AES-128 in the plain `identity` format.
fn usable_key(keys: &[Key]) -> Option<&Key> {
    keys.iter()
        .find(|k| k.method == KeyMethod::Aes128 && k.format == KeyFormat::Identity)
}

fn encryption(key: &Key, sequence: u64) -> Result<Encryption, ManifestError> {
    let key_url = key
        .url
        .clone()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or(ManifestError::KeyWithoutAddress)?;
    check_web_address(&key_url)?;
    Ok(Encryption {
        key_url,
        iv: key.iv.unwrap_or_else(|| iv_from_sequence(sequence)),
    })
}

/// Why an encrypted segment cannot be downloaded: the DRM system its key format names, or the method this crate does not decrypt.
fn refusal(key: &Key) -> Refusal {
    match (&key.format, &key.method) {
        (KeyFormat::Other(format), _) => {
            Refusal::Drm(DrmSystem::named_by(format).unwrap_or(DrmSystem::Unknown))
        }
        (KeyFormat::Identity, KeyMethod::Other(method)) => {
            Refusal::Unsupported(Feature::Encryption(method.clone()))
        }
        (KeyFormat::Identity, KeyMethod::Aes128) => {
            Refusal::Unsupported(Feature::Encryption("AES-128".into()))
        }
    }
}

/// The IV of a segment without an explicit one: its media sequence number as a 128-bit big-endian number.
pub(crate) fn iv_from_sequence(sequence: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// Reads `0x` followed by up to 32 hex digits.
pub(crate) fn parse_iv(value: &str) -> Result<[u8; 16], ManifestError> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .filter(|h| !h.is_empty() && h.len() <= 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| ManifestError::BadIv(excerpt(value)))?;
    let padded = format!("{hex:0>32}");
    let mut iv = [0; 16];
    for (i, byte) in iv.iter_mut().enumerate() {
        // Every character is a hex digit, checked above.
        *byte = u8::from_str_radix(&padded[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    Ok(iv)
}

/// `<length>[@<offset>]`.
fn byte_range(value: &str) -> Result<(u64, Option<u64>), ManifestError> {
    let bad = || ManifestError::BadByteRange(excerpt(value));
    let value = value.trim();
    let (len, offset) = match value.split_once('@') {
        Some((len, offset)) => (len, Some(offset.trim().parse().map_err(|_| bad())?)),
        None => (value, None),
    };
    Ok((len.trim().parse().map_err(|_| bad())?, offset))
}

/// `[start, start + len)`, refused when it is empty (no request can ask for no bytes) or would pass the largest offset.
fn span(start: u64, len: u64) -> Result<Range<u64>, ManifestError> {
    if len == 0 {
        return Err(ManifestError::EmptyByteRange { start });
    }
    start
        .checked_add(len)
        .map(|end| start..end)
        .ok_or(ManifestError::ByteRangeOverflow { start, length: len })
}

/// Splits `#EXT-X-NAME:value` into its name and value.
fn tag(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with("#EXT") {
        return None;
    }
    Some(line.split_once(':').unwrap_or((line, "")))
}

/// Reads an attribute list such as `BANDWIDTH=1280000,CODECS="avc1,mp4a"`. Quoted values may contain commas; quotes are removed.
pub(crate) fn attributes(value: &str) -> Vec<(String, String)> {
    let mut list = Vec::new();
    let mut rest = value;
    loop {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim().to_string();
        rest = &rest[eq + 1..];
        let value;
        if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            value = &quoted[..end];
            rest = quoted.get(end + 1..).unwrap_or("");
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim();
            rest = &rest[end..];
        }
        list.push((name, value.to_string()));
    }
    list
}

fn attr<'a>(list: &'a [(String, String)], name: &str) -> Option<&'a str> {
    list.iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn number(list: &[(String, String)], name: &str) -> Option<u64> {
    attr(list, name).and_then(|v| v.trim().parse().ok())
}

fn join(base: &Url, reference: &str) -> Result<Url, ManifestError> {
    crate::plan::web_address(base, reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://site.example/video/index.m3u8").unwrap()
    }

    fn media(text: &str) -> Media {
        match parse(text, &base()).unwrap() {
            Playlist::Media(media) => media,
            Playlist::Master(_) => panic!("expected a media playlist"),
        }
    }

    #[test]
    fn attribute_lists_handle_quotes_and_commas() {
        let attrs = attributes(
            r#"BANDWIDTH=1280000,CODECS="avc1.4d401f,mp4a.40.2",RESOLUTION=640x360, NAME="a=b""#,
        );
        assert_eq!(attr(&attrs, "BANDWIDTH"), Some("1280000"));
        assert_eq!(attr(&attrs, "CODECS"), Some("avc1.4d401f,mp4a.40.2"));
        assert_eq!(attr(&attrs, "RESOLUTION"), Some("640x360"));
        assert_eq!(attr(&attrs, "NAME"), Some("a=b"));
        assert_eq!(attributes(""), Vec::<(String, String)>::new());
        // An unterminated quote does not panic.
        assert_eq!(attr(&attributes(r#"URI="abc"#), "URI"), Some("abc"));
    }

    #[test]
    fn master_playlists_list_variants_and_renditions() {
        let text = "#EXTM3U\r\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",LANGUAGE=\"en\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"audio/en.m3u8\"\r\n\
            #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",LANGUAGE=\"fr\",NAME=\"Français\",URI=\"subs/fr.m3u8\"\r\n\
            #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\",FRAME-RATE=29.970,AUDIO=\"aud\"\r\n\
            low/index.m3u8\r\n\
            #EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=100000,URI=\"iframes.m3u8\"\r\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2400000,RESOLUTION=1280x720,AUDIO=\"aud\"\r\n\
            https://cdn.example/high/index.m3u8\r\n";
        let Playlist::Master(master) = parse(text, &base()).unwrap() else {
            panic!("expected a master playlist");
        };
        assert_eq!(master.variants.len(), 2);
        let low = &master.variants[0];
        assert_eq!(
            low.url.as_str(),
            "https://site.example/video/low/index.m3u8"
        );
        assert_eq!(low.bandwidth, Some(800_000));
        assert_eq!(low.resolution, Some((640, 360)));
        assert_eq!(low.frame_rate, Some(29.97));
        assert_eq!(low.audio_group.as_deref(), Some("aud"));
        assert_eq!(
            master.variants[1].url.as_str(),
            "https://cdn.example/high/index.m3u8"
        );
        assert_eq!(master.renditions.len(), 2);
        let audio = &master.renditions[0];
        assert_eq!(audio.kind, RenditionKind::Audio);
        assert!(audio.default);
        assert_eq!(audio.language.as_deref(), Some("en"));
        assert_eq!(
            audio.url.as_ref().map(Url::as_str),
            Some("https://site.example/video/audio/en.m3u8")
        );
        assert_eq!(master.renditions[1].kind, RenditionKind::Subtitles);
    }

    #[test]
    fn media_playlists_resolve_segments_and_duration() {
        let m = media(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.006,\nseg0.ts\n#EXTINF:5.5,title\n/abs/seg1.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(m.ended);
        assert!((m.duration - 11.506).abs() < 1e-9);
        assert_eq!(m.pieces.len(), 2);
        assert_eq!(
            m.pieces[0].url.as_str(),
            "https://site.example/video/seg0.ts"
        );
        assert_eq!(m.pieces[1].url.as_str(), "https://site.example/abs/seg1.ts");
        assert!(
            m.pieces
                .iter()
                .all(|p| p.encryption.is_none() && p.range.is_none())
        );
    }

    #[test]
    fn no_endlist_means_live() {
        let m = media("#EXTM3U\n#EXTINF:4,\na.ts\n");
        assert!(!m.ended);
        let m = media("#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4,\na.ts\n");
        assert!(m.ended);
        let m = media("#EXTM3U\n#EXT-X-PLAYLIST-TYPE:EVENT\n#EXTINF:4,\na.ts\n");
        assert!(!m.ended);
    }

    #[test]
    fn byte_ranges_continue_from_the_previous_segment() {
        let m = media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"700@0\"\n\
             #EXTINF:4,\n#EXT-X-BYTERANGE:1000@700\nall.mp4\n\
             #EXTINF:4,\n#EXT-X-BYTERANGE:500\nall.mp4\n\
             #EXTINF:4,\n#EXT-X-BYTERANGE:20@10\nother.mp4\n#EXT-X-ENDLIST\n",
        );
        let ranges: Vec<_> = m.pieces.iter().map(|p| p.range.clone()).collect();
        assert_eq!(
            ranges,
            [
                Some(0..700),
                Some(700..1700),
                Some(1700..2200),
                Some(10..30)
            ]
        );
        assert!(m.pieces[0].init);
        assert!(!m.pieces[1].init);

        let error = parse("#EXTM3U\n#EXTINF:4,\n#EXT-X-BYTERANGE:500\na.ts\n", &base())
            .unwrap_err()
            .cause;
        assert_eq!(error, ManifestError::ByteRangeWithoutStart);
    }

    #[test]
    fn init_sections_are_emitted_once_and_again_when_they_change() {
        let m = media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init1.mp4\"\n#EXTINF:4,\na.m4s\n#EXTINF:4,\nb.m4s\n\
             #EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"init2.mp4\"\n#EXTINF:4,\nc.m4s\n#EXT-X-ENDLIST\n",
        );
        let names: Vec<_> = m
            .pieces
            .iter()
            .map(|p| {
                p.url
                    .path_segments()
                    .unwrap()
                    .next_back()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(names, ["init1.mp4", "a.m4s", "b.m4s", "init2.mp4", "c.m4s"]);
    }

    #[test]
    fn keys_ivs_and_media_sequence() {
        let m = media(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n\
             #EXTINF:4,\nclear.ts\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"https://keys.example/k1\"\n\
             #EXTINF:4,\nseq8.ts\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"k2\",IV=0x000102030405060708090A0B0C0D0E0F\n\
             #EXTINF:4,\nexplicit.ts\n\
             #EXT-X-KEY:METHOD=NONE\n\
             #EXTINF:4,\nclear-again.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(m.pieces[0].encryption.is_none());
        let e1 = m.pieces[1].encryption.as_ref().unwrap();
        assert_eq!(e1.key_url.as_str(), "https://keys.example/k1");
        assert_eq!(e1.iv, iv_from_sequence(8));
        assert_eq!(e1.iv[15], 8);
        let e2 = m.pieces[2].encryption.as_ref().unwrap();
        assert_eq!(e2.key_url.as_str(), "https://site.example/video/k2");
        assert_eq!(
            e2.iv,
            0x0001_0203_0405_0607_0809_0a0b_0c0d_0e0f_u128.to_be_bytes()
        );
        assert!(m.pieces[3].encryption.is_none());
        assert!(m.refusal.is_none());
    }

    #[test]
    fn a_key_before_a_map_encrypts_the_init_section_and_a_key_after_does_not() {
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\",IV=0x1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\na.m4s\n#EXT-X-ENDLIST\n",
        );
        assert!(m.pieces[0].init && m.pieces[0].encryption.is_some());
        let m = media(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.m4s\n#EXT-X-ENDLIST\n",
        );
        assert!(m.pieces[0].init && m.pieces[0].encryption.is_none());
        assert!(m.pieces[1].encryption.is_some());
    }

    #[test]
    fn drm_and_sample_aes_are_reported() {
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://abc\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(matches!(
            &m.refusal,
            Some(Refusal::Drm(DrmSystem::FairPlay))
        ));
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(matches!(
            &m.refusal,
            Some(Refusal::Unsupported(Feature::Encryption(method))) if method == "SAMPLE-AES"
        ));
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\",KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(matches!(
            &m.refusal,
            Some(Refusal::Drm(DrmSystem::Widevine))
        ));
        // A plain AES-128 key next to a DRM one: the plain one is used.
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(m.refusal.is_none());
        assert!(m.pieces[0].encryption.is_some());
    }

    #[test]
    fn byte_ranges_that_overflow_are_errors() {
        let max = u64::MAX;
        for text in [
            format!(
                "#EXTM3U
#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"{max}@10\"
#EXTINF:1,
a.ts
"
            ),
            format!(
                "#EXTM3U
#EXTINF:1,
#EXT-X-BYTERANGE:10@{max}
a.ts
"
            ),
            format!(
                "#EXTM3U
#EXTINF:1,
#EXT-X-BYTERANGE:{}@0
a.ts
#EXTINF:1,
#EXT-X-BYTERANGE:2
a.ts
",
                max - 1
            ),
        ] {
            let error = parse(&text, &base()).unwrap_err().cause;
            assert!(
                matches!(error, ManifestError::ByteRangeOverflow { .. }),
                "{error}"
            );
        }
    }

    /// Refused where the limit is passed, not once the whole playlist is read.
    #[test]
    fn a_playlist_is_refused_at_the_first_segment_past_the_limit() {
        let mut text = String::from("#EXTM3U\n");
        for _ in 0..crate::plan::MAX_SEGMENTS + 10 {
            text.push_str("#EXTINF:1,\ns.ts\n");
        }
        let error = parse(&text, &base()).unwrap_err();
        assert_eq!(error.cause, ManifestError::TooManySegments);
        let first_past = usize::try_from(crate::plan::MAX_SEGMENTS).unwrap() + 1;
        // Line 1 is `#EXTM3U`; segment `n` takes lines `2n` and `2n + 1`.
        assert_eq!(error.line, Some(2 * first_past + 1));
    }

    #[test]
    fn ivs_are_parsed_strictly() {
        assert_eq!(parse_iv("0x1").unwrap()[15], 1);
        assert_eq!(parse_iv("0XFF00").unwrap()[14], 0xFF);
        for bad in [
            "",
            "0x",
            "1234",
            "0xZZ",
            "0x000102030405060708090A0B0C0D0E0F00",
        ] {
            assert!(parse_iv(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn broken_playlists_name_what_is_wrong_and_never_panic() {
        let huge = "9".repeat(40);
        for (text, expected) in [
            (String::new(), ManifestError::NotM3u),
            ("hello".into(), ManifestError::NotM3u),
            ("#EXTM3U\n".into(), ManifestError::NoSegments),
            // Cut off after a quality's tag, before its address.
            (
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n".into(),
                ManifestError::NoVariants,
            ),
            (
                "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:x\n#EXTINF:1,\na.ts\n".into(),
                ManifestError::BadMediaSequence("x".into()),
            ),
            (
                format!("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:{huge}\n#EXTINF:1,\na.ts\n"),
                ManifestError::BadMediaSequence(huge.clone()),
            ),
            (
                "#EXTM3U\n#EXT-X-BYTERANGE:abc\na.ts\n".into(),
                ManifestError::BadByteRange("abc".into()),
            ),
            (
                format!("#EXTM3U\n#EXT-X-BYTERANGE:{huge}@0\na.ts\n"),
                ManifestError::BadByteRange(format!("{huge}@0")),
            ),
            (
                "#EXTM3U\n#EXT-X-MAP:BYTERANGE=\"1@0\"\n#EXTINF:1,\na.ts\n".into(),
                ManifestError::MapWithoutUri,
            ),
            (
                "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\",IV=0xZZ\n#EXTINF:1,\na.ts\n".into(),
                ManifestError::BadIv("0xZZ".into()),
            ),
            (
                "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"skd://k\"\n#EXTINF:1,\na.ts\n".into(),
                ManifestError::KeyWithoutAddress,
            ),
            (
                "#EXTM3U\n#EXTINF:1,\nhttp://[::1\n".into(),
                ManifestError::BadLink("http://[::1".into()),
            ),
        ] {
            assert_eq!(
                parse(&text, &base()).unwrap_err().cause,
                expected,
                "{text:?}"
            );
        }
        // Odd but harmless input parses, and an absurd duration counts as unknown.
        assert!(parse("\u{feff}#EXTM3U\n#EXTINF:abc,\na.ts\n", &base()).is_ok());
        let m = media("#EXTM3U\n#EXTINF:1e300,\na.ts\n#EXT-X-ENDLIST\n");
        assert!(m.duration.abs() < f64::EPSILON);
    }

    #[test]
    fn errors_name_the_line_to_blame() {
        let error = parse(
            "#EXTM3U\n\n#EXTINF:1,\na.ts\n#EXT-X-BYTERANGE:abc\nb.ts\n",
            &base(),
        )
        .unwrap_err();
        assert_eq!(error.line, Some(5));
        let error = parse(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nfile:///etc/passwd\n",
            &base(),
        )
        .unwrap_err();
        assert_eq!(error.line, Some(3));
        // Nothing on one line is to blame for a playlist with no segments.
        assert_eq!(parse("#EXTM3U\n", &base()).unwrap_err().line, None);
    }

    #[test]
    fn links_must_be_web_addresses_of_a_real_length() {
        let long = format!("{}.ts", "a".repeat(crate::plan::MAX_URL_LEN));
        for (text, expected) in [
            (
                "#EXTM3U\n#EXTINF:1,\nfile:///etc/passwd\n".to_string(),
                ManifestError::NotWebAddress("file".into()),
            ),
            (
                "#EXTM3U\n#EXT-X-MAP:URI=\"data:,x\"\n#EXTINF:1,\na.ts\n".into(),
                ManifestError::NotWebAddress("data".into()),
            ),
            (
                format!("#EXTM3U\n#EXTINF:1,\n{long}\n"),
                ManifestError::LinkTooLong(long.len()),
            ),
        ] {
            assert_eq!(
                parse(&text, &base()).unwrap_err().cause,
                expected,
                "{text:?}"
            );
        }
    }

    /// Each short line becomes a whole address, base included: a long base and many lines would otherwise hold gigabytes of addresses from a small playlist.
    #[test]
    fn a_track_whose_addresses_are_too_long_together_is_refused() {
        let folder = "f".repeat(crate::plan::MAX_URL_LEN - 100);
        let base = Url::parse(&format!("https://cdn.example/{folder}/index.m3u8")).unwrap();
        let len = u64::try_from(base.join("s.ts").unwrap().as_str().len()).unwrap();
        let count = crate::plan::MAX_TRACK_URL_BYTES / len + 1;
        let mut text = String::from("#EXTM3U\n");
        for _ in 0..count {
            text.push_str("#EXTINF:1,\ns.ts\n");
        }
        // Not `unwrap_err`, which would print every address on failure.
        let error = parse(&text, &base).err().map(|e| e.cause);
        assert_eq!(error, Some(ManifestError::AddressesTooLong));
    }

    /// A master playlist served from a long address makes every short quality line a long address.
    #[test]
    fn a_master_playlist_whose_addresses_are_too_long_together_is_refused() {
        let folder = "f".repeat(crate::plan::MAX_URL_LEN - 100);
        let base = Url::parse(&format!("https://cdn.example/{folder}/master.m3u8")).unwrap();
        let len = u64::try_from(base.join("a").unwrap().as_str().len()).unwrap();
        let count = crate::plan::MAX_MANIFEST_URL_BYTES / len + 1;
        let mut text = String::from("#EXTM3U\n");
        for _ in 0..count {
            text.push_str("#EXT-X-STREAM-INF:BANDWIDTH=1\na\n");
        }
        let error = parse(&text, &base).err().map(|e| e.cause);
        assert_eq!(error, Some(ManifestError::AddressesTooLong));
    }

    #[test]
    fn a_master_playlist_holds_a_bounded_number_of_qualities() {
        let mut text = String::from("#EXTM3U\n");
        for _ in 0..=crate::plan::MAX_MANIFEST_ENTRIES {
            text.push_str("#EXT-X-STREAM-INF:\na\n");
        }
        let error = parse(&text, &base()).err().map(|e| e.cause);
        assert_eq!(error, Some(ManifestError::TooManyEntries));
    }

    /// A new init section at the limit takes a place too, so it and its segment both have to fit.
    #[test]
    fn the_segment_limit_counts_a_new_init_section() {
        let max = crate::plan::MAX_SEGMENTS;
        let mut text = String::from("#EXTM3U\n");
        for _ in 0..max - 1 {
            text.push_str("#EXTINF:1,\ns.ts\n");
        }
        // One place is left: the segment fits, but not with an init section before it.
        let fits = format!("{text}#EXTINF:1,\nlast.ts\n");
        assert!(parse(&fits, &base()).is_ok());
        let too_many = format!("{text}#EXT-X-MAP:URI=\"i.mp4\"\n#EXTINF:1,\nlast.ts\n");
        assert_eq!(
            parse(&too_many, &base()).unwrap_err().cause,
            ManifestError::TooManySegments
        );
    }

    #[test]
    fn a_media_sequence_that_overflows_is_refused() {
        let text = format!(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:{}\n#EXTINF:1,\na.ts\n#EXTINF:1,\nb.ts\n",
            u64::MAX
        );
        assert_eq!(
            parse(&text, &base()).unwrap_err().cause,
            ManifestError::NumberTooLarge
        );
    }

    #[test]
    fn empty_byte_ranges_are_refused() {
        // No request can ask for no bytes: `bytes=5-4` is invalid, and `0..0` would fetch one byte that is not the segment's.
        for text in [
            "#EXTM3U\n#EXTINF:1,\n#EXT-X-BYTERANGE:0@5\na.ts\n",
            "#EXTM3U\n#EXT-X-MAP:URI=\"i.mp4\",BYTERANGE=\"0@0\"\n#EXTINF:1,\na.ts\n",
        ] {
            assert!(
                matches!(
                    parse(text, &base()).unwrap_err().cause,
                    ManifestError::EmptyByteRange { .. }
                ),
                "{text:?}"
            );
        }
    }
}
