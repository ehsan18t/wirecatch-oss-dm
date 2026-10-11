//! What can go wrong with a stream.

use std::io;
use std::path::PathBuf;

/// Errors from [`inspect`](crate::inspect) and [`download`](crate::download).
///
/// Every message says what happened and, where there is one, how to fix it,
/// so it can be shown to a user as is. New variants may be added in minor
/// releases, so a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A request failed: the server refused it, the network stayed down beyond
    /// the retries, and the like. The engine's error says which.
    #[error("{0}")]
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

    /// The playlist or manifest is broken; the text says how.
    #[error(
        "The stream's playlist could not be read: {0}. The link may be broken or expired; open the video page again to get a fresh one."
    )]
    Manifest(String),

    /// The video is encrypted with DRM; the text names the kind.
    #[error(
        "This video is protected with DRM ({0}), which only licensed players can open, so Wirecatch cannot download it."
    )]
    Drm(String),

    /// The stream uses a feature this version does not handle; the text names it.
    #[error("This stream uses {0}, which Wirecatch cannot download yet.")]
    Unsupported(String),

    /// The track chosen in the request is not in the stream (any more).
    #[error(
        "The chosen {kind} track \"{id}\" is not in this stream. Inspect the stream again and pick one of the options it lists."
    )]
    NoSuchTrack {
        /// `video` or `audio`.
        kind: &'static str,
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

    /// ffmpeg ran but failed; the text is what it reported. The downloaded
    /// segments are kept, so trying again does not download them again.
    #[error(
        "ffmpeg could not join the video: {0}. The downloaded parts are kept; try again, or reinstall the ffmpeg add-on if it keeps failing."
    )]
    Ffmpeg(String),

    /// The decryption key could not be used.
    #[error(
        "The decryption key at {url} is not usable: {reason}. The link may have expired; open the video page again to get a fresh one."
    )]
    Key {
        /// Where the key came from.
        url: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A segment kept failing after the retries.
    #[error("Part {number} of {total} of the video could not be downloaded: {source}")]
    Segment {
        /// The segment's position, counting from 1.
        number: usize,
        /// How many segments the download has.
        total: usize,
        /// What went wrong.
        #[source]
        source: wirecatch_engine::Error,
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
    Io(#[from] io::Error),

    /// Cancelled before the list of segments was known, so there is nothing
    /// to resume yet. Cancelling later returns
    /// [`Outcome::Paused`](crate::Outcome::Paused) instead.
    #[error("Cancelled before the download started.")]
    Cancelled,
}

impl From<wirecatch_engine::Error> for Error {
    fn from(error: wirecatch_engine::Error) -> Self {
        match error {
            wirecatch_engine::Error::Cancelled => Error::Cancelled,
            wirecatch_engine::Error::PrivateNetwork { .. } => Error::PrivateNetwork,
            other => Error::Request(other),
        }
    }
}

impl Error {
    /// For a background task that panicked: a bug, reported as such.
    pub(crate) fn task(error: tokio::task::JoinError) -> Self {
        Error::Request(wirecatch_engine::Error::Internal(format!(
            "a background task failed: {error}"
        )))
    }
}

/// A `Result` whose error defaults to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
