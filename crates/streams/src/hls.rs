//! HLS playlists (RFC 8216): master playlists with their variants and
//! renditions, and media playlists turned into the list of pieces to fetch.
//!
//! Tags are read in order, so the rules that depend on order hold: an
//! `EXT-X-KEY` applies to the segments and `EXT-X-MAP` sections after it, an
//! `EXT-X-BYTERANGE` without an offset continues where the previous segment
//! of the same file ended, and a segment without an explicit IV uses its media
//! sequence number.

use std::ops::Range;

use url::Url;

use crate::plan::{Encryption, Piece, Refusal};

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
        self.resolution.is_none()
            && self
                .codecs
                .as_deref()
                .is_some_and(|c| c.split(',').all(|c| crate::plan::is_audio_codec(c.trim())))
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

/// Reads a playlist; relative addresses in it are resolved against `base`
/// (the playlist's own address after redirects).
pub(crate) fn parse(text: &str, base: &Url) -> Result<Playlist, String> {
    let mut lines = text
        .lines()
        .map(|l| l.trim_start_matches('\u{feff}').trim())
        .filter(|l| !l.is_empty());
    if lines.next() != Some("#EXTM3U") {
        return Err("it does not start with #EXTM3U".into());
    }
    let lines: Vec<&str> = lines.collect();
    if lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF")) {
        parse_master(&lines, base).map(Playlist::Master)
    } else {
        parse_media(&lines, base).map(Playlist::Media)
    }
}

fn parse_master(lines: &[&str], base: &Url) -> Result<Master, String> {
    let mut master = Master::default();
    let mut pending: Option<Vec<(String, String)>> = None;
    for &line in lines {
        if let Some((name, value)) = tag(line) {
            match name {
                "#EXT-X-STREAM-INF" => pending = Some(attributes(value)),
                "#EXT-X-MEDIA" => {
                    if let Some(rendition) = rendition(&attributes(value), base)? {
                        master.renditions.push(rendition);
                    }
                }
                _ => {}
            }
        } else if line.starts_with('#') {
            continue;
        } else if let Some(attrs) = pending.take() {
            master.variants.push(Variant {
                url: join(base, line)?,
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
    }
    if master.variants.is_empty() {
        return Err("its list of qualities is empty".into());
    }
    Ok(master)
}

fn rendition(attrs: &[(String, String)], base: &Url) -> Result<Option<Rendition>, String> {
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

/// One `EXT-X-KEY` in force.
#[derive(Debug, Clone)]
struct Key {
    method: String,
    format: String,
    url: Option<Url>,
    iv: Option<[u8; 16]>,
}

/// An `EXT-X-MAP` in force, with the key that applied where it appeared.
#[derive(Debug, Clone)]
struct Map {
    url: Url,
    range: Option<Range<u64>>,
    key: Option<Key>,
}

fn parse_media(lines: &[&str], base: &Url) -> Result<Media, String> {
    let mut media = Media::default();
    let mut media_sequence = 0u64;
    let mut index = 0u64;
    let mut keys: Vec<Key> = Vec::new();
    let mut map: Option<Map> = None;
    let mut emitted_map: Option<(Url, Option<Range<u64>>)> = None;
    let mut pending_range: Option<(u64, Option<u64>)> = None;
    let mut previous_end: Option<(Url, u64)> = None;
    let mut pending_duration = 0.0;

    for &line in lines {
        if let Some((name, value)) = tag(line) {
            match name {
                "#EXT-X-MEDIA-SEQUENCE" => {
                    media_sequence = value.trim().parse().map_err(|_| {
                        format!("its media sequence number {value:?} is not a number")
                    })?;
                }
                "#EXTINF" => {
                    pending_duration = value
                        .split(',')
                        .next()
                        .and_then(|d| d.trim().parse::<f64>().ok())
                        .filter(|d| d.is_finite() && *d >= 0.0)
                        .unwrap_or(0.0);
                }
                "#EXT-X-BYTERANGE" => pending_range = Some(byte_range(value)?),
                "#EXT-X-KEY" => update_keys(&mut keys, &attributes(value), base)?,
                "#EXT-X-MAP" => {
                    let attrs = attributes(value);
                    let url = attr(&attrs, "URI")
                        .ok_or("an EXT-X-MAP has no URI")
                        .and_then(|u| join(base, u).map_err(|_| "an EXT-X-MAP has a bad URI"))?;
                    let range = attr(&attrs, "BYTERANGE").map(byte_range).transpose()?.map(
                        |(len, offset)| {
                            let start = offset.unwrap_or(0);
                            start..start + len
                        },
                    );
                    map = Some(Map {
                        url,
                        range,
                        key: usable_key(&keys).cloned(),
                    });
                }
                "#EXT-X-ENDLIST" => media.ended = true,
                "#EXT-X-PLAYLIST-TYPE" if value.trim() == "VOD" => media.ended = true,
                _ => {}
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let url = join(base, line)?;
        let sequence = media_sequence.saturating_add(index);
        if let Some(map) = &map {
            let this = (map.url.clone(), map.range.clone());
            if emitted_map.as_ref() != Some(&this) {
                media.pieces.push(Piece {
                    url: map.url.clone(),
                    range: map.range.clone(),
                    encryption: map
                        .key
                        .as_ref()
                        .map(|k| encryption(k, sequence))
                        .transpose()?,
                    init: true,
                });
                emitted_map = Some(this);
            }
        }
        let range = match pending_range.take() {
            None => None,
            Some((len, Some(offset))) => Some(offset..offset.saturating_add(len)),
            Some((len, None)) => match &previous_end {
                Some((previous, end)) if *previous == url => Some(*end..end.saturating_add(len)),
                _ => {
                    return Err(
                        "a byte range without an offset does not follow a part of the same file"
                            .into(),
                    );
                }
            },
        };
        previous_end = range.as_ref().map(|r| (url.clone(), r.end));
        let encryption = match usable_key(&keys) {
            Some(key) => Some(encryption(key, sequence)?),
            None => {
                if let Some(key) = keys.first()
                    && media.refusal.is_none()
                {
                    media.refusal = Some(refusal(key));
                }
                None
            }
        };
        media.pieces.push(Piece {
            url,
            range,
            encryption,
            init: false,
        });
        media.duration += pending_duration;
        pending_duration = 0.0;
        index += 1;
    }
    if index == 0 {
        return Err("it lists no segments".into());
    }
    Ok(media)
}

fn update_keys(keys: &mut Vec<Key>, attrs: &[(String, String)], base: &Url) -> Result<(), String> {
    let method = attr(attrs, "METHOD").unwrap_or("NONE");
    if method == "NONE" {
        keys.clear();
        return Ok(());
    }
    let format = attr(attrs, "KEYFORMAT").unwrap_or("identity").to_string();
    // A key address that is not a URL at all (such as `skd://…` for FairPlay)
    // is kept as missing: it is never fetched, only reported.
    let url = attr(attrs, "URI").and_then(|u| base.join(u).ok());
    let iv = attr(attrs, "IV").map(parse_iv).transpose()?;
    keys.retain(|k| k.format != format);
    keys.push(Key {
        method: method.to_string(),
        format,
        url,
        iv,
    });
    Ok(())
}

/// The key this crate can decrypt with: AES-128 in the plain `identity` format.
fn usable_key(keys: &[Key]) -> Option<&Key> {
    keys.iter()
        .find(|k| k.method == "AES-128" && k.format == "identity")
}

fn encryption(key: &Key, sequence: u64) -> Result<Encryption, String> {
    let key_url = key
        .url
        .clone()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or("an AES-128 key has no http or https address")?;
    Ok(Encryption {
        key_url,
        iv: key.iv.unwrap_or_else(|| iv_from_sequence(sequence)),
    })
}

/// Why an encrypted segment cannot be downloaded.
fn refusal(key: &Key) -> Refusal {
    let format = key.format.to_ascii_lowercase();
    let drm = if format.contains("streamingkeydelivery") {
        Some("Apple FairPlay")
    } else if format.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed") {
        Some("Google Widevine")
    } else if format.contains("playready")
        || format.contains("9a04f079-9840-4286-ab92-e65be0885f95")
    {
        Some("Microsoft PlayReady")
    } else if format != "identity" {
        Some("an unknown key system")
    } else {
        None
    };
    match drm {
        Some(name) => Refusal::Drm(name.to_string()),
        None => Refusal::Unsupported(format!("{} encryption", key.method)),
    }
}

/// The IV of a segment without an explicit one: its media sequence number as
/// a 128-bit big-endian number.
pub(crate) fn iv_from_sequence(sequence: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// Reads `0x` followed by up to 32 hex digits.
pub(crate) fn parse_iv(value: &str) -> Result<[u8; 16], String> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .filter(|h| !h.is_empty() && h.len() <= 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("the IV {value:?} is not a hexadecimal number"))?;
    let padded = format!("{hex:0>32}");
    let mut iv = [0; 16];
    for (i, byte) in iv.iter_mut().enumerate() {
        // Every character is a hex digit, checked above.
        *byte = u8::from_str_radix(&padded[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    Ok(iv)
}

/// `<length>[@<offset>]`.
fn byte_range(value: &str) -> Result<(u64, Option<u64>), String> {
    let bad = || format!("the byte range {value:?} is not valid");
    let value = value.trim();
    let (len, offset) = match value.split_once('@') {
        Some((len, offset)) => (len, Some(offset.trim().parse().map_err(|_| bad())?)),
        None => (value, None),
    };
    Ok((len.trim().parse().map_err(|_| bad())?, offset))
}

/// Splits `#EXT-X-NAME:value` into its name and value.
fn tag(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with("#EXT") {
        return None;
    }
    Some(line.split_once(':').unwrap_or((line, "")))
}

/// Reads an attribute list such as `BANDWIDTH=1280000,CODECS="avc1,mp4a"`.
/// Quoted values may contain commas; quotes are removed.
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

fn join(base: &Url, reference: &str) -> Result<Url, String> {
    base.join(reference)
        .map_err(|_| format!("it links to {reference:?}, which is not a valid address"))
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
        assert!(attributes("").is_empty());
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

        let error =
            parse("#EXTM3U\n#EXTINF:4,\n#EXT-X-BYTERANGE:500\na.ts\n", &base()).unwrap_err();
        assert!(error.contains("byte range"), "{error}");
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
        assert_eq!(e2.iv, core::array::from_fn(|i| i as u8));
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
        assert!(matches!(&m.refusal, Some(Refusal::Drm(name)) if name == "Apple FairPlay"));
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(matches!(&m.refusal, Some(Refusal::Unsupported(_))));
        // A plain AES-128 key next to a DRM one: the plain one is used.
        let m = media(
            "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n",
        );
        assert!(m.refusal.is_none());
        assert!(m.pieces[0].encryption.is_some());
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
    fn broken_playlists_are_errors_not_panics() {
        for text in [
            "",
            "hello",
            "#EXTM3U\n",
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n",
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:x\n#EXTINF:1,\na.ts\n",
            "#EXTM3U\n#EXT-X-BYTERANGE:abc\na.ts\n",
            "#EXTM3U\n#EXT-X-MAP:BYTERANGE=\"1@0\"\n#EXTINF:1,\na.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\",IV=0xZZ\n#EXTINF:1,\na.ts\n",
        ] {
            assert!(parse(text, &base()).is_err(), "{text:?}");
        }
        // Odd but harmless input parses.
        assert!(parse("\u{feff}#EXTM3U\n#EXTINF:abc,\na.ts\n", &base()).is_ok());
    }
}
