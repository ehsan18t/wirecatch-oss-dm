//! What a download fetches: the pieces of each chosen track, and the fingerprint that tells whether segments on disk belong to them.

#![forbid(unsafe_code)]

use std::ops::Range;

use url::Url;
use wirecatch_engine::units::{KIB_USIZE, MIB};

use crate::codec::Codec;
use crate::error::{DrmSystem, Error, Feature, ManifestError, excerpt};
use crate::model::Role;

/// The longest address a playlist may link to. Signed CDN addresses run to a couple of thousand characters; a longer one is a hostile playlist making every piece of a track huge.
pub(crate) const MAX_URL_LEN: usize = 8 * KIB_USIZE;

/// The most address text one track's pieces may hold together, so a short manifest that repeats a long address cannot fill memory.
pub(crate) const MAX_TRACK_URL_BYTES: u64 = 64 * MIB;

/// The most address text one playlist or manifest may resolve to while it is read, over all its tracks. A master playlist's short lines resolve against its own (possibly long) address, so a small playlist could otherwise ask for gigabytes.
pub(crate) const MAX_MANIFEST_URL_BYTES: u64 = 2 * MAX_TRACK_URL_BYTES;

/// The most pieces, qualities and renditions one playlist or manifest may hold while it is read, over all its tracks.
pub(crate) const MAX_MANIFEST_ENTRIES: u64 = 4 * MAX_SEGMENTS;

/// The most representations one DASH manifest may offer. Real ones offer tens; each costs memory and work, however short its text. It also bounds the base addresses a manifest keeps: a representation shares its set's unless it has its own, and an address is at most [`MAX_URL_LEN`], so they come to at most 64 MiB (a representation's own and its set's) whatever the manifest's length.
pub(crate) const MAX_REPRESENTATIONS: usize = 4096;

/// The most name text (ids, codecs, types, languages and labels) one DASH manifest's representations may hold together. A representation keeps the names of its adaptation set, and what reading the manifest reports copies them once more, so one long label on a set of many representations would otherwise be repeated thousands of times. As much as the manifest itself may be long.
pub(crate) const MAX_MANIFEST_NAME_BYTES: u64 = 8 * MIB;

/// `reference` resolved against `base`, refused unless it is a web address (`http` or `https`, never `file:` or another scheme a playlist could aim at the computer) no longer than [`MAX_URL_LEN`].
pub(crate) fn web_address(base: &Url, reference: &str) -> Result<Url, ManifestError> {
    if reference.len() > MAX_URL_LEN {
        return Err(ManifestError::LinkTooLong(reference.len()));
    }
    let url = base
        .join(reference)
        .map_err(|_| ManifestError::BadLink(excerpt(reference)))?;
    check_web_address(&url)?;
    Ok(url)
}

/// Refuses an address that is not a web address or is longer than [`MAX_URL_LEN`].
pub(crate) fn check_web_address(url: &Url) -> Result<(), ManifestError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ManifestError::NotWebAddress(excerpt(url.scheme())));
    }
    if url.as_str().len() > MAX_URL_LEN {
        return Err(ManifestError::LinkTooLong(url.as_str().len()));
    }
    Ok(())
}

/// The address text one track's pieces have used so far, against [`MAX_TRACK_URL_BYTES`].
#[derive(Debug, Default)]
pub(crate) struct AddressBudget(u64);

impl AddressBudget {
    /// Counts `url` for one more piece; refused once the track's addresses pass the limit.
    pub fn spend(&mut self, url: &Url) -> Result<(), ManifestError> {
        let len = u64::try_from(url.as_str().len()).unwrap_or(u64::MAX);
        self.0 = self.0.saturating_add(len);
        if self.0 > MAX_TRACK_URL_BYTES {
            Err(ManifestError::AddressesTooLong)
        } else {
            Ok(())
        }
    }
}

/// What reading one playlist or manifest has kept so far, over all its tracks, against [`MAX_MANIFEST_URL_BYTES`], [`MAX_MANIFEST_ENTRIES`] and [`MAX_MANIFEST_NAME_BYTES`]. What a DASH representation takes from an outer level is shared with that level; only its names, which are copied out once per representation when the manifest is reported, count here.
#[derive(Debug, Default)]
pub(crate) struct ManifestBudget {
    url_bytes: u64,
    entries: u64,
    name_bytes: u64,
}

impl ManifestBudget {
    /// Counts one more entry (a piece, a quality or a rendition) with its address; refused once the manifest passes either limit.
    pub fn spend(&mut self, url: &Url) -> Result<(), ManifestError> {
        self.count(1)?;
        let len = u64::try_from(url.as_str().len()).unwrap_or(u64::MAX);
        self.url_bytes = self.url_bytes.saturating_add(len);
        if self.url_bytes > MAX_MANIFEST_URL_BYTES {
            Err(ManifestError::AddressesTooLong)
        } else {
            Ok(())
        }
    }

    /// Counts `bytes` more of a track's names; refused once the manifest passes [`MAX_MANIFEST_NAME_BYTES`].
    pub fn names(&mut self, bytes: usize) -> Result<(), ManifestError> {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        self.name_bytes = self.name_bytes.saturating_add(bytes);
        if self.name_bytes > MAX_MANIFEST_NAME_BYTES {
            Err(ManifestError::NamesTooLong)
        } else {
            Ok(())
        }
    }

    /// Counts `entries` more entries without an address of their own (a rendition without an address); refused once the manifest passes [`MAX_MANIFEST_ENTRIES`].
    pub fn count(&mut self, entries: u64) -> Result<(), ManifestError> {
        self.entries = self.entries.saturating_add(entries);
        if self.entries > MAX_MANIFEST_ENTRIES {
            Err(ManifestError::TooManyEntries)
        } else {
            Ok(())
        }
    }
}

/// The most pieces one track may have (about 55 hours of 1-second segments), so a broken or hostile playlist cannot exhaust memory.
pub(crate) const MAX_SEGMENTS: u64 = 200_000;

/// Refuses another piece for a track that already has `pieces`, when it would pass [`MAX_SEGMENTS`]. Checked before every piece is added, init sections included, so a huge playlist fails at once instead of filling memory first.
pub(crate) fn check_room(pieces: &[Piece]) -> Result<(), ManifestError> {
    check_room_for(pieces, 1)
}

/// Refuses `more` pieces for a track that already has `pieces`, when the track would then pass [`MAX_SEGMENTS`]: an early exit before a long run of pieces is built one by one.
pub(crate) fn check_room_for(pieces: &[Piece], more: u64) -> Result<(), ManifestError> {
    let total = u64::try_from(pieces.len())
        .ok()
        .and_then(|n| n.checked_add(more));
    if total.is_some_and(|n| n <= MAX_SEGMENTS) {
        Ok(())
    } else {
        Err(ManifestError::TooManySegments)
    }
}

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
    /// Shared by every piece the key encrypts.
    pub key_url: std::sync::Arc<Url>,
    pub iv: [u8; 16],
}

/// Why a track cannot be downloaded.
#[derive(Debug, Clone)]
pub(crate) enum Refusal {
    Drm(DrmSystem),
    Unsupported(Feature),
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Drm(name) => Error::Drm(name),
            Refusal::Unsupported(what) => Error::Unsupported(what),
        }
    }
}

/// One track to download and join.
#[derive(Debug, Clone)]
pub(crate) struct Track {
    pub role: Role,
    /// The codecs the stream names for it, each `None` when it is not one the policy knows; empty when it names none.
    pub codecs: Vec<Option<Codec>>,
    pub pieces: Vec<Piece>,
}

/// Everything to download, in the order the tracks are given to ffmpeg.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub tracks: Vec<Track>,
}

/// Query parameters that sign a link or say when it expires, compared without case. Their values change every time a link is refreshed while the content stays the same, so the fingerprint keeps only their names. Covers CloudFront, Akamai, S3, Google Cloud Storage, nginx `secure_link` and the common generic names.
const SIGNING_PARAMETERS: &[&str] = &[
    "token",
    "__token__",
    "hdnts",
    "hdntl",
    "policy",
    "signature",
    "sig",
    "key-pair-id",
    "expires",
    "exp",
    "md5",
    "hmac",
    "auth",
    "authtoken",
];

/// Prefixes of signing parameters, compared without case: S3 (`X-Amz-Signature`, `X-Amz-Date`) and Google Cloud Storage (`X-Goog-Signature`).
const SIGNING_PREFIXES: &[&str] = &["x-amz-", "x-goog-"];

pub(crate) fn is_signing_parameter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SIGNING_PARAMETERS.contains(&name.as_str())
        || SIGNING_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

impl Plan {
    /// Identifies the plan across runs, to tell whether segments on disk belong to it. Paths count, and so do queries, ranges, keys and the number of pieces. Of a query, every parameter counts except the values of [`SIGNING_PARAMETERS`]: refreshing a signed link must keep the segments already downloaded, while two videos told apart only by a query value (`?video=1`, `?video=2`) must never share them. Hosts are left out, because one video is often served by whichever CDN host is closest at the time. A token carried in the path itself (some Akamai set-ups) still makes a refreshed link start over.
    pub fn fingerprint(&self) -> u64 {
        let mut hash = Fnv::default();
        hash.write(&(self.tracks.len() as u64).to_le_bytes());
        for track in &self.tracks {
            hash.write(&[track.role as u8]);
            hash.write(&(track.pieces.len() as u64).to_le_bytes());
            for piece in &track.pieces {
                hash.write(piece.url.path().as_bytes());
                hash.write(&[0xFE]);
                for (name, value) in piece.url.query_pairs() {
                    hash.write(name.as_bytes());
                    hash.write(&[0xFD]);
                    if !is_signing_parameter(&name) {
                        hash.write(value.as_bytes());
                    }
                    hash.write(&[0xFC]);
                }
                if let Some(range) = &piece.range {
                    hash.write(&range.start.to_le_bytes());
                    hash.write(&range.end.to_le_bytes());
                }
                if let Some(encryption) = &piece.encryption {
                    hash.write(&encryption.iv);
                }
                hash.write(&[u8::from(piece.init), 0xFF]);
            }
        }
        hash.0
    }
}

/// FNV-1a, 64 bits: stable across runs and Rust versions, unlike the standard library's hasher.
pub(crate) struct Fnv(pub(crate) u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub(crate) fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_follow_paths_not_hosts_or_signing_tokens() {
        let piece = |u: &str| Piece::plain(Url::parse(u).unwrap(), None);
        let plan = |urls: &[&str]| Plan {
            tracks: vec![Track {
                role: Role::Video,
                codecs: Vec::new(),
                pieces: urls.iter().map(|u| piece(u)).collect(),
            }],
        };
        let signed = plan(&[
            "https://cdn1.example/s/1.ts?token=a",
            "https://cdn1.example/s/2.ts?token=a",
        ]);
        let refreshed = plan(&[
            "https://cdn2.example/s/1.ts?token=b",
            "https://cdn2.example/s/2.ts?token=b",
        ]);
        let same_on_another_host = plan(&[
            "https://cdn2.example/s/1.ts?token=a",
            "https://cdn2.example/s/2.ts?token=a",
        ]);
        let other_segments = plan(&["https://cdn1.example/s/1.ts", "https://cdn1.example/s/3.ts"]);
        // Segments told apart only by their query.
        let by_query = plan(&[
            "https://cdn1.example/seg?n=1",
            "https://cdn1.example/seg?n=2",
        ]);
        let other_query = plan(&[
            "https://cdn1.example/seg?n=1",
            "https://cdn1.example/seg?n=3",
        ]);
        assert_eq!(signed.fingerprint(), same_on_another_host.fingerprint());
        // A refreshed signed link: same segments, new tokens.
        assert_eq!(signed.fingerprint(), refreshed.fingerprint());
        assert_ne!(signed.fingerprint(), other_segments.fingerprint());
        assert_ne!(by_query.fingerprint(), other_query.fingerprint());
    }

    // Two videos told apart only by a query value got one fingerprint, so a work folder of one (both named master.mp4, say) was reused for the other and their segments were joined.
    #[test]
    fn fingerprints_keep_query_values_that_are_not_signatures() {
        let piece = |u: &str| Piece::plain(Url::parse(u).unwrap(), None);
        let plan = |urls: &[&str]| Plan {
            tracks: vec![Track {
                role: Role::Video,
                codecs: Vec::new(),
                pieces: urls.iter().map(|u| piece(u)).collect(),
            }],
        };
        let video = |id: u8, token: &str| {
            plan(&[
                &format!("https://cdn.example/hls/720/1.ts?video={id}&token={token}"),
                &format!("https://cdn.example/hls/720/2.ts?video={id}&token={token}"),
            ])
        };
        assert_ne!(video(1, "a").fingerprint(), video(2, "a").fingerprint());
        assert_eq!(video(1, "a").fingerprint(), video(1, "b").fingerprint());

        let s3 = |signature: &str| {
            plan(&[&format!(
                "https://bucket.example/v/1.ts?X-Amz-Date=2026&X-Amz-Signature={signature}"
            )])
        };
        assert_eq!(s3("aa").fingerprint(), s3("bb").fingerprint());
    }

    // A byte-range track lists one file many times, so its path was never unique and the whole query, signing token included, was hashed: a refreshed link lost every segment.
    #[test]
    fn a_refreshed_byte_range_link_keeps_its_fingerprint() {
        let piece =
            |u: &str, start: u64| Piece::plain(Url::parse(u).unwrap(), Some(start..start + 100));
        let plan = |token: &str| {
            let url = format!("https://cdn.example/v/main.mp4?hdntl={token}");
            Plan {
                tracks: vec![Track {
                    role: Role::Video,
                    codecs: Vec::new(),
                    pieces: vec![piece(&url, 0), piece(&url, 100)],
                }],
            }
        };
        assert_eq!(plan("a").fingerprint(), plan("b").fingerprint());
    }
}
