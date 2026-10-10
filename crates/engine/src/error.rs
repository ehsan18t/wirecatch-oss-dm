//! What can go wrong in a download.

use std::fmt;
use std::io;

/// Errors a download can end with.
///
/// [`Error::is_retryable`] tells whether trying the same download again later
/// can reasonably succeed. The engine already retries those errors itself
/// before giving up, so seeing one here means the retries ran out.
///
/// New variants may be added in minor releases, so a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The URL could not be used, for example a redirect to an address that is
    /// not `http` or `https`. The text says which URL and why.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// The connection failed: DNS, refused or reset connections, TLS errors,
    /// timeouts while connecting.
    #[error("network error: {0}")]
    Network(NetworkError),

    /// The server answered with a status that is not a success, such as `404`.
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

    /// Nothing arrived for [`DownloadOptions::stall_timeout`](crate::DownloadOptions::stall_timeout),
    /// neither an answer to a request nor data; the number is that timeout in seconds.
    #[error("no data received for {0} seconds")]
    Stalled(u64),

    /// Reading or writing the file or its resume file failed, for example a
    /// full disk or a folder without write permission.
    #[error("disk error: {0}")]
    Io(#[from] io::Error),

    /// The download was cancelled before the server answered its first request,
    /// so there is nothing to resume yet. Cancelling once the server has
    /// answered returns [`Outcome::Paused`](crate::Outcome::Paused) instead.
    #[error("cancelled before the download started")]
    Cancelled,

    /// The resume file exists but could not be read.
    #[error("resume state is unreadable: {0}")]
    State(String),

    /// The link kept redirecting, more than 20 times, usually because it
    /// redirects in a loop.
    #[error(
        "{url} redirected more than 20 times, probably in a loop. Open the link in a browser to check it, or get a fresh link"
    )]
    TooManyRedirects {
        /// The URL that was asked for.
        url: String,
    },

    /// The server answered with a redirect but did not say where to.
    #[error(
        "{url} answered with redirect {status} but did not say where to; the link may be broken"
    )]
    RedirectWithoutLocation {
        /// The redirect status code.
        status: u16,
        /// The URL that answered.
        url: String,
    },

    /// A secure (`https`) link redirected to an insecure (`http`) one, which
    /// would let anyone on the network read or change the file. Refused unless
    /// [`DownloadOptions::allow_insecure_redirects`](crate::DownloadOptions::allow_insecure_redirects)
    /// is set.
    #[error(
        "the secure link {from} redirects to {to}, which is not encrypted, so the download was stopped. Allow insecure redirects to download it anyway"
    )]
    InsecureRedirect {
        /// The `https` URL that redirected.
        from: String,
        /// The `http` URL it redirected to.
        to: String,
    },

    /// [`DownloadRequest::resolve_path`](crate::DownloadRequest::resolve_path)
    /// refused the file; the text is its reason. Nothing was written.
    #[error("the download was not saved: {0}")]
    TargetRejected(String),

    /// Something inside the engine failed that should not, such as a task that
    /// panicked. This is a bug in Wirecatch; please report it with the text.
    #[error("internal error: {0}. This is a bug in Wirecatch; please report it")]
    Internal(String),
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

    pub(crate) fn network(error: reqwest::Error) -> Self {
        Error::Network(NetworkError(error))
    }

    /// For a background task that panicked or was aborted.
    pub(crate) fn task(error: tokio::task::JoinError) -> Self {
        Error::Internal(format!("a background task failed: {error}"))
    }
}

/// A failed connection or request. The message says what failed, including
/// the underlying cause (for example "connection refused" or a certificate
/// problem); [`std::error::Error::source`] gives the chain.
pub struct NetworkError(reqwest::Error);

impl NetworkError {
    /// Whether a time limit ran out (connecting, or waiting for the server).
    pub fn is_timeout(&self) -> bool {
        self.0.is_timeout()
    }

    /// Whether the connection could not be opened at all (name lookup, refused
    /// connection, TLS handshake).
    pub fn is_connect(&self) -> bool {
        self.0.is_connect()
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        // The top message is often just "error sending request"; the deepest
        // cause says what actually happened.
        let mut cause = std::error::Error::source(&self.0);
        let mut deepest = None;
        while let Some(c) = cause {
            deepest = Some(c);
            cause = c.source();
        }
        if let Some(deepest) = deepest {
            write!(f, ": {deepest}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.0)
    }
}

/// A `Result` whose error defaults to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
