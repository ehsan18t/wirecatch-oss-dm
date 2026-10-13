//! What can go wrong with a stream.

#![forbid(unsafe_code)]

use std::io;
use std::path::PathBuf;

use crate::model::Role;
use crate::plan::{MAX_REPRESENTATIONS, MAX_SEGMENTS};

/// Errors from [`inspect`](crate::inspect) and [`download`](crate::download).
///
/// Every message says what happened and, where there is one, how to fix it, so it can be shown to a user as is. A variant whose message includes its cause holds the cause as a field rather than as its [`source`](std::error::Error::source), so a report that follows the chain of causes says it once. New variants may be added in minor releases, so a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A request failed: the server refused it, the network stayed down beyond
    /// the retries, and the like. The engine's error says which.
    #[error(transparent)]
    Request(wirecatch_engine::Error),

    /// The stream is a live broadcast (an HLS playlist without an end, or a
    /// dynamic DASH manifest). Downloading those is not supported yet.
    #[error("This video is a live broadcast. Wirecatch can download it after the broadcast ends.")]
    Live,

    /// The link answered with something that is neither an HLS playlist nor a
    /// DASH manifest.
    #[error(
        "This link is not a video stream Wirecatch can read (HLS or DASH). Open the video page again and use the link the browser extension found."
    )]
    NotAStream,

    /// The playlist or manifest is broken; the cause says how. The cause, with its own causes, is part of the message, so it is a field rather than the error's source.
    #[error(
        "The stream's playlist{} could not be read: {}. The link may be broken or expired; open the video page again to get a fresh one.", place(host.as_deref(), *line), Chain(cause)
    )]
    Manifest {
        /// The host the playlist or manifest came from.
        host: Option<String>,
        /// The line of the playlist or manifest to blame, counting from 1, when one is.
        line: Option<usize>,
        /// What is wrong.
        cause: ManifestError,
    },

    /// The playlist or manifest holds far more than any real stream: its addresses or track names together, or its pieces, qualities, renditions or representations. A fresh link does not help.
    #[error(
        "This stream's playlist is far larger than any video's, so Wirecatch does not download it."
    )]
    TooLarge,

    /// The playlist or manifest lists more segments than any video Wirecatch downloads (200,000 per track). A fresh link does not help.
    #[error(
        "This stream lists more than {MAX_SEGMENTS} segments in one track, more than any video Wirecatch can download."
    )]
    TooManySegments,

    /// The video is encrypted with DRM.
    #[error(
        "This video is protected with DRM ({0}), which only licensed players can open, so Wirecatch cannot download it."
    )]
    Drm(DrmSystem),

    /// The stream uses a feature this version does not handle.
    #[error("This stream uses {0}, which Wirecatch cannot download yet.")]
    Unsupported(Feature),

    /// The track chosen in the request is not in the stream (any more).
    #[error(
        "The chosen {} track \"{id}\" is not in this stream. Inspect the stream again and pick one of the options it lists.", .kind.name()
    )]
    NoSuchTrack {
        /// Whether a video or an audio track was asked for.
        kind: Role,
        /// The id that was asked for.
        id: String,
    },

    /// The request left out both video and audio, or the stream has neither.
    #[error("There is nothing to download: choose a video or an audio track.")]
    NothingChosen,

    /// Video and audio come separately and must be joined, which takes ffmpeg,
    /// but no ffmpeg was given. Nothing was downloaded.
    #[error("Joining video and audio needs the ffmpeg add-on. Install it and try again.")]
    NeedsFfmpeg,

    /// The ffmpeg program could not be started from the given path.
    #[error("The ffmpeg add-on was not found at {}. Reinstall it and try again.", .0.display())]
    FfmpegMissing(PathBuf),

    /// A file of the download cannot be given to ffmpeg as an absolute path, so ffmpeg was not started. The downloaded segments are kept.
    #[error(
        "ffmpeg could not join the video: {}. The downloaded parts are kept; try again.", Chain(.0)
    )]
    Path(crate::ffmpeg::FileArgumentError),

    /// ffmpeg could not be started from the given path for another reason, or stopped writing progress and was stopped. The downloaded segments are kept.
    #[error(
        "ffmpeg could not join the video: {}. The downloaded parts are kept; try again, or reinstall the ffmpeg add-on if it keeps failing.", Chain(.0)
    )]
    FfmpegRun(crate::process::RunError),

    /// ffmpeg ran but failed; the text is what it reported. The downloaded
    /// segments are kept, so trying again does not download them again.
    #[error(
        "ffmpeg could not join the video: {0}. The downloaded parts are kept; try again, or reinstall the ffmpeg add-on if it keeps failing."
    )]
    Ffmpeg(String),

    /// A downloaded track is not in a format Wirecatch lets ffmpeg read (see
    /// [`InputFormat`](crate::InputFormat)), so it was not passed to ffmpeg:
    /// ffmpeg would have to guess what it is, and a crafted file could then
    /// make it open other files or addresses. The downloaded parts are kept.
    #[error(
        "The downloaded {} is not in a format Wirecatch can join (MPEG-TS, MP4, Matroska, WebM, AAC, MP3, AC-3 or E-AC-3), so it was not passed to ffmpeg. The link may not point to a real video; open the video page again to get a fresh one.", .role.name()
    )]
    UnknownFormat {
        /// Which track it is.
        role: Role,
    },

    /// The decryption key could not be used.
    #[error(
        "The decryption key at {url} is not usable: {}. The link may have expired; open the video page again to get a fresh one.",
        Chain(problem)
    )]
    Key {
        /// Where the key came from.
        url: String,
        /// What is wrong with it.
        problem: KeyProblem,
    },

    /// A segment kept failing after the retries.
    #[error(
        "Part {number} of {total} of the video could not be downloaded: {}",
        Chain(cause)
    )]
    Segment {
        /// The segment's position, counting from 1.
        number: usize,
        /// How many segments the download has.
        total: usize,
        /// What went wrong.
        cause: wirecatch_engine::Error,
    },

    /// A segment did not decrypt with its key.
    #[error(
        "Part {number} of {total} of the video could not be decrypted, so its key does not fit. The link may have expired; open the video page again to get a fresh one."
    )]
    Decrypt {
        /// The segment's position, counting from 1.
        number: usize,
        /// How many segments the download has.
        total: usize,
    },

    /// The stream points to an address on the local network (a router, a
    /// printer, this computer) although it comes from the internet. Refused
    /// so a website cannot use Wirecatch to reach devices at home; see
    /// [`StreamRequest::private_network`](crate::StreamRequest::private_network).
    #[error(
        "This video points to a device on your own network, which Wirecatch blocks for safety."
    )]
    PrivateNetwork,

    /// Reading or writing files failed.
    #[error(
        "Could not write the video to disk: {0}. Check that the folder is writable and has free space."
    )]
    Io(io::Error),

    /// A task of the download panicked, a bug, or was stopped as the app closed.
    #[error("Part of the download stopped unexpectedly ({0}). Try again.")]
    Task(tokio::task::JoinError),

    /// Cancelled before the list of segments was known, so there is nothing
    /// to resume yet. Cancelling later returns
    /// [`Outcome::Paused`](crate::Outcome::Paused) instead.
    #[error("Cancelled before the download started.")]
    Cancelled,
}

impl From<wirecatch_engine::Error> for Error {
    fn from(error: wirecatch_engine::Error) -> Self {
        map_engine(error, Error::Request)
    }
}

/// An engine error as this crate's: cancelling and the local network rule are the same whatever was being fetched, and every other failure is wrapped by `wrap`, which says what was being fetched.
pub(crate) fn map_engine(
    error: wirecatch_engine::Error,
    wrap: impl FnOnce(wirecatch_engine::Error) -> Error,
) -> Error {
    match error {
        wirecatch_engine::Error::Cancelled => Error::Cancelled,
        wirecatch_engine::Error::PrivateNetwork { .. } => Error::PrivateNetwork,
        other => wrap(other),
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

impl From<crate::ffmpeg::FileArgumentError> for Error {
    fn from(error: crate::ffmpeg::FileArgumentError) -> Self {
        Error::Path(error)
    }
}

impl Error {
    /// A broken playlist or manifest from `url`, as the error for the person.
    pub(crate) fn manifest(url: &url::Url, located: Located) -> Error {
        match located.cause {
            ManifestError::TooManySegments => Error::TooManySegments,
            ManifestError::AddressesTooLong
            | ManifestError::TooManyEntries
            | ManifestError::TooManyRepresentations
            | ManifestError::NamesTooLong => Error::TooLarge,
            cause => Error::Manifest {
                host: url.host_str().map(str::to_string),
                line: located.line,
                cause,
            },
        }
    }
}

/// An error and every cause below it, for a message that holds the error as a field rather than as its source: "it is not valid XML: unexpected end of stream".
struct Chain<'a>(&'a dyn std::error::Error);

impl std::fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            write!(f, ": {error}")?;
            cause = error.source();
        }
        Ok(())
    }
}

/// Where a broken playlist is, in words: ` at <host> (line <n>)`.
fn place(host: Option<&str>, line: Option<usize>) -> String {
    let at = host.map(|host| format!(" at {host}")).unwrap_or_default();
    let line = line
        .map(|line| format!(" (line {line})"))
        .unwrap_or_default();
    format!("{at}{line}")
}

/// A manifest error with the line it was found on, when the format has lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    pub line: Option<usize>,
    pub cause: ManifestError,
}

impl From<ManifestError> for Located {
    fn from(cause: ManifestError) -> Self {
        Located { line: None, cause }
    }
}

impl ManifestError {
    /// This error, found on line `line`.
    pub(crate) fn at(self, line: usize) -> Located {
        Located {
            line: Some(line),
            cause: self,
        }
    }
}

/// The most characters of a playlist's own text an error repeats.
const MAX_EXCERPT_CHARS: usize = 80;

/// `text` from a playlist, cut to [`MAX_EXCERPT_CHARS`], to quote in an error.
pub(crate) fn excerpt(text: &str) -> String {
    let mut chars = text.chars();
    let mut cut: String = chars.by_ref().take(MAX_EXCERPT_CHARS).collect();
    if chars.next().is_some() {
        cut.push('…');
    }
    cut
}

/// What is wrong with an HLS decryption key.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KeyProblem {
    /// It could not be fetched.
    #[error(transparent)]
    Fetch(wirecatch_engine::Error),
    /// It is not 16 bytes long.
    #[error("it is {0} bytes long instead of 16")]
    Length(usize),
}

/// A DRM system, named by the id a playlist or manifest gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrmSystem {
    /// Apple FairPlay.
    FairPlay,
    /// Google Widevine.
    Widevine,
    /// Microsoft PlayReady.
    PlayReady,
    /// W3C Clear Key.
    ClearKey,
    /// MPEG Common Encryption with a system not named.
    CommonEncryption,
    /// A key system this crate does not know.
    Unknown,
}

/// DRM system ids (lower case) and the system each stands for: the UUIDs of the DASH-IF registry, which HLS `KEYFORMAT` and DASH `schemeIdUri` both use, and FairPlay's HLS key format name.
const DRM_SYSTEMS: [(&str, DrmSystem); 5] = [
    ("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed", DrmSystem::Widevine),
    ("9a04f079-9840-4286-ab92-e65be0885f95", DrmSystem::PlayReady),
    ("94ce86fb-07ff-4f43-adb8-93d2fa968ca2", DrmSystem::FairPlay),
    ("1077efec-c0b2-4d02-ace3-3c1e52e2fb4b", DrmSystem::ClearKey),
    ("com.apple.streamingkeydelivery", DrmSystem::FairPlay),
];

impl DrmSystem {
    /// The system an id (a key format or scheme URI, any case) names, if it names one this crate knows.
    pub(crate) fn named_by(id: &str) -> Option<DrmSystem> {
        let id = id.to_ascii_lowercase();
        DRM_SYSTEMS
            .iter()
            .find(|(known, _)| id.contains(known))
            .map(|(_, system)| *system)
            .or_else(|| id.contains("playready").then_some(DrmSystem::PlayReady))
    }
}

impl std::fmt::Display for DrmSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DrmSystem::FairPlay => "Apple FairPlay",
            DrmSystem::Widevine => "Google Widevine",
            DrmSystem::PlayReady => "Microsoft PlayReady",
            DrmSystem::ClearKey => "W3C Clear Key",
            DrmSystem::CommonEncryption => "Common Encryption",
            DrmSystem::Unknown => "an unknown key system",
        })
    }
}

/// A stream feature this crate cannot download.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Feature {
    /// HLS encryption other than AES-128, such as `SAMPLE-AES`; the method as the playlist names it.
    Encryption(String),
    /// A DASH manifest with several periods.
    SeveralPeriods,
}

impl std::fmt::Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Feature::Encryption(method) => write!(f, "{method} encryption"),
            Feature::SeveralPeriods => f.write_str(
                "a DASH manifest made of several periods (often a video with ads joined in)",
            ),
        }
    }
}

/// Why a playlist or manifest cannot be read. Each says what is wrong and, where it helps, the value that is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// An HLS playlist does not start with `#EXTM3U`.
    #[error("it does not start with #EXTM3U")]
    NotM3u,
    /// An HLS master playlist lists no qualities.
    #[error("its list of qualities is empty")]
    NoVariants,
    /// A playlist or a track lists no segments.
    #[error("it lists no segments")]
    NoSegments,
    /// A track has more than 200,000 pieces, more than any video has.
    #[error(
        "it lists more than {MAX_SEGMENTS} segments, more than any video Wirecatch can download"
    )]
    TooManySegments,
    /// `EXT-X-MEDIA-SEQUENCE` is not a number.
    #[error("its media sequence number {0:?} is not a number")]
    BadMediaSequence(String),
    /// A byte range cannot be read.
    #[error("the byte range {0:?} is not valid")]
    BadByteRange(String),
    /// A byte range ends past the largest possible offset.
    #[error("the byte range {length}@{start} ends past any possible file size")]
    ByteRangeOverflow {
        /// Where it starts.
        start: u64,
        /// How long it is.
        length: u64,
    },
    /// A byte range of no bytes, which no request can ask for.
    #[error("the byte range at {start} is empty")]
    EmptyByteRange {
        /// Where it starts.
        start: u64,
    },
    /// An HLS byte range without an offset does not follow a part of the same file.
    #[error("a byte range without an offset does not follow a part of the same file")]
    ByteRangeWithoutStart,
    /// An `IV` is not a hexadecimal number of at most 128 bits.
    #[error("the IV {0:?} is not a hexadecimal number")]
    BadIv(String),
    /// An AES-128 key has no web address to fetch it from.
    #[error("an AES-128 key has no http or https address")]
    KeyWithoutAddress,
    /// An `EXT-X-MAP` has no `URI`.
    #[error("an EXT-X-MAP has no URI")]
    MapWithoutUri,
    /// A link in it is not a valid address.
    #[error("it links to {0:?}, which is not a valid address")]
    BadLink(String),
    /// A link in it is not a web address (`file:` and the like), which no stream may point to.
    #[error("it links to a {0:?} address, which is not a web address")]
    NotWebAddress(String),
    /// A link in it is longer than any real address.
    #[error("it links to an address of {0} characters, longer than any real one")]
    LinkTooLong(usize),
    /// A track's addresses are, together, longer than any real stream's.
    #[error("its addresses are far longer than any video's")]
    AddressesTooLong,
    /// It holds more pieces, qualities and renditions, together, than any real stream's.
    #[error("it lists far more than any video's")]
    TooManyEntries,
    /// A DASH manifest offers more representations than any real stream's.
    #[error("it offers more than {MAX_REPRESENTATIONS} tracks, far more than any video's")]
    TooManyRepresentations,
    /// The names, languages and codecs of a DASH manifest's representations, as each keeps them, are together longer than any real stream's: a long name on an adaptation set is repeated for every representation in it.
    #[error("its track names are far longer than any video's")]
    NamesTooLong,
    /// A number in a segment template or timeline is not one it can use.
    #[error("its {name} {value:?} is not a number it can use")]
    BadNumber {
        /// The attribute, such as `startNumber` or a timeline's `r`.
        name: &'static str,
        /// Its value, cut short.
        value: String,
    },
    /// A segment timeline's times go backwards.
    #[error("its segment times go backwards")]
    TimeGoesBack,
    /// A template's `timescale` is 0.
    #[error("a segment template has a timescale of 0")]
    ZeroTimescale,
    /// A quality's playlist is a master playlist itself.
    #[error("a quality's playlist is itself a list of qualities")]
    NestedMaster,
    /// A DASH manifest is not XML. The error's line says where; the parser's error, its source, says what.
    #[error("it is not valid XML")]
    NotXml(#[source] roxmltree::Error),
    /// The XML is not a DASH manifest.
    #[error("it is not a DASH manifest")]
    NotMpd,
    /// A DASH manifest has no `Period`.
    #[error("it has no periods")]
    NoPeriods,
    /// A `SegmentTemplate` has no `media` address.
    #[error("a segment template has no media address")]
    TemplateWithoutMedia,
    /// A `SegmentTemplate` has neither a `duration` nor a `SegmentTimeline`.
    #[error("a segment template has neither a duration nor a timeline")]
    TemplateWithoutDuration,
    /// A `SegmentTimeline` entry has no duration.
    #[error("a segment timeline entry has no duration")]
    TimelineEntryWithoutDuration,
    /// Segments are counted to the end of the stream, which it does not give.
    #[error("the stream does not say how long it is")]
    UnknownLength,
    /// Segment numbers or times pass the largest number.
    #[error("its segment numbers or times are too large")]
    NumberTooLarge,
    /// A segment address template has a `$` that is not closed.
    #[error("a segment address has an unclosed $")]
    UnclosedTemplate,
    /// A segment address template names an identifier DASH does not have.
    #[error("a segment address uses the unknown ${0}$")]
    UnknownIdentifier(String),
    /// A segment address template uses an identifier where it has no value (`$Time$` in an initialization address).
    #[error("${0}$ is used where it has no value")]
    IdentifierWithoutValue(String),
    /// A segment address template gives an identifier a width it cannot use.
    #[error("${0}$ has a width it cannot use")]
    BadWidth(String),
}

/// A `Result` whose error defaults to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    /// A cause held as a field (its message is part of this error's) still brings its own reason along: what the operating system or the XML parser said.
    #[test]
    fn a_cause_held_in_the_message_keeps_its_own_reason() {
        let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "access is refused here");
        let run = Error::FfmpegRun(crate::process::RunError::Start {
            program: PathBuf::from("ffmpeg"),
            source: denied(),
        });
        let segment = Error::Segment {
            number: 3,
            total: 10,
            cause: wirecatch_engine::Error::Io {
                action: wirecatch_engine::IoAction::Write,
                path: PathBuf::from("part"),
                source: denied(),
            },
        };
        let key = Error::Key {
            url: "https://cdn.example/key".into(),
            problem: KeyProblem::Fetch(wirecatch_engine::Error::Io {
                action: wirecatch_engine::IoAction::Write,
                path: PathBuf::from("key"),
                source: denied(),
            }),
        };
        let xml = roxmltree::Document::parse("<MPD>").unwrap_err();
        let reason = xml.to_string();
        let url = url::Url::parse("https://cdn.example/manifest.mpd").unwrap();
        let manifest = Error::manifest(&url, ManifestError::NotXml(xml).into());
        for error in [run, segment, key] {
            let message = error.to_string();
            assert!(message.contains("access is refused here"), "{message}");
        }
        let message = manifest.to_string();
        assert!(message.contains(&reason), "{message}");
    }

    /// A path ffmpeg cannot be given is not a disk that cannot be written.
    #[test]
    fn a_path_ffmpeg_cannot_be_given_is_its_own_error() {
        let error = Error::from(crate::ffmpeg::FileArgumentError {
            path: PathBuf::new(),
            source: io::Error::from(io::ErrorKind::InvalidInput),
        });
        assert!(matches!(error, Error::Path(_)), "{error:?}");
    }

    #[test]
    fn a_broken_playlist_says_where() {
        let url = url::Url::parse("https://cdn.example/v/index.m3u8").unwrap();
        let error = Error::manifest(&url, ManifestError::NoSegments.at(7));
        let message = error.to_string();
        assert!(
            message.starts_with("The stream's playlist at cdn.example (line 7) could not be read: it lists no segments."),
            "{message}"
        );
        let whole = Error::manifest(&url, ManifestError::NoSegments.into()).to_string();
        assert!(
            whole.starts_with("The stream's playlist at cdn.example could not be read"),
            "{whole}"
        );
        // The limits are their own errors, which a fresh link does not fix.
        assert!(matches!(
            Error::manifest(&url, ManifestError::AddressesTooLong.into()),
            Error::TooLarge
        ));
    }

    #[test]
    fn quoted_playlist_text_is_cut_short() {
        assert_eq!(excerpt("short"), "short");
        let long = "é".repeat(MAX_EXCERPT_CHARS + 1);
        let cut = excerpt(&long);
        assert_eq!(cut.chars().count(), MAX_EXCERPT_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
