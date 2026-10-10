//! What can go wrong in a download.

use std::io;

/// Errors a download can end with.
///
/// [`Error::is_retryable`] tells whether trying the same download again later
/// can reasonably succeed. The engine already retries those errors itself
/// before giving up, so seeing one here means the retries ran out.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The URL could not be used, for example a redirect to an address that is
    /// not `http` or `https`. The text says which URL and why.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// The connection failed: DNS, refused or reset connections, TLS errors,
    /// timeouts while connecting.
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    /// The server answered with a status that is not a success, such as `404`,
    /// or a `3xx` without a usable `Location` or after too many redirects.
    #[error("server answered {status} for {url}")]
    HttpStatus {
        /// The HTTP status code.
        status: u16,
        /// The URL that answered (after redirects).
        url: String,
    },

    /// The server accepted byte ranges at first but then answered a range
    /// request with the wrong part of the file, or with the whole file.
    #[error("server stopped honoring byte ranges (asked for offset {expected}, got {got:?})")]
    RangeNotHonored {
        /// The offset that was asked for.
        expected: u64,
        /// The offset the server sent instead, if it said.
        got: Option<u64>,
    },

    /// The server now reports a different size for the file, so the bytes
    /// downloaded so far belong to another version of it.
    #[error("the file on the server changed while downloading")]
    RemoteChanged,

    /// A connection ended before its part of the file was complete.
    #[error("connection closed early at byte {at} of a part ending at {end}")]
    PrematureEof {
        /// Offset reached when the connection ended.
        at: u64,
        /// Offset the part should have reached.
        end: u64,
    },

    /// No data arrived for [`DownloadOptions::stall_timeout`](crate::DownloadOptions::stall_timeout);
    /// the number is that timeout in seconds.
    #[error("no data received for {0} seconds")]
    Stalled(u64),

    /// Reading or writing the file or its resume file failed, for example a
    /// full disk or a folder without write permission.
    #[error("disk error: {0}")]
    Io(#[from] io::Error),

    /// The download was cancelled while waiting to retry its first request, so
    /// there is nothing to resume yet. Cancelling once data flows returns
    /// [`Outcome::Paused`](crate::Outcome::Paused) instead.
    #[error("cancelled before the download started")]
    Cancelled,

    /// The resume file exists but could not be read.
    #[error("resume state is unreadable: {0}")]
    State(String),
}

impl Error {
    /// Whether retrying the same request later can reasonably succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Network(_) | Error::PrematureEof { .. } | Error::Stalled(_) => true,
            Error::HttpStatus { status, .. } => {
                matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }
}

/// A `Result` whose error defaults to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
