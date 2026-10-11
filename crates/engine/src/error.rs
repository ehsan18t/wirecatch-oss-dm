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
    /// The URL could not be used, for example a scheme the engine does not
    /// download from, or a redirect to an address that is not `http` or
    /// `https`. The text says which URL and why.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// The connection failed: DNS, refused or reset connections, TLS errors,
    /// timeouts while connecting. Used for every protocol (HTTP, HTTP/3 and
    /// FTP).
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

    /// A [`Fetcher`](crate::Fetcher) answer was larger than the size limit the
    /// caller set for it, so it was not read to the end.
    #[error(
        "{url} sent more than {limit} bytes, more than expected for it; the link may not point to what was expected"
    )]
    TooLarge {
        /// The URL that answered.
        url: String,
        /// The limit in bytes.
        limit: u64,
    },

    /// A [`Fetcher`](crate::Fetcher) refused to send a request to an address
    /// on the local network (loopback, private, link-local or unspecified),
    /// because a resource from the internet pointed there; see
    /// [`PrivateNetwork`](crate::fetch::PrivateNetwork).
    #[error("{url} points to a device on your own network, which Wirecatch blocks for safety")]
    PrivateNetwork {
        /// The address that was refused.
        url: String,
    },

    /// The FTP server answered a command with an error, for example `550` for
    /// a file that does not exist. Codes from 400 to 499 mean the problem is
    /// temporary, so they are retried (see [`Error::is_retryable`]); others
    /// are not.
    #[error("the FTP server answered \"{code} {message}\" for {url}{}", ftp_reply_hint(*code))]
    #[non_exhaustive]
    FtpReply {
        /// The FTP reply code, such as `550`.
        code: u16,
        /// The text the server sent with the code.
        message: String,
        /// The address being downloaded, without any password.
        url: String,
    },

    /// The FTP server did not accept the user name or password. The text the
    /// server sent is included; the password never is.
    #[error("{}", login_refused_text(host, *code, message, *anonymous))]
    #[non_exhaustive]
    FtpLoginRefused {
        /// The server's host name or address.
        host: String,
        /// The FTP reply code, usually `530`.
        code: u16,
        /// The text the server sent with the code.
        message: String,
        /// Whether the login was the anonymous one (no user name in the
        /// link), which many servers do not allow.
        anonymous: bool,
    },

    /// An `ftps` or `ftpes` link asked for encryption, but the server does not
    /// offer it. The download stops rather than continue unencrypted.
    #[error(
        "the FTP server {host} does not offer encryption, so the secure download was stopped. If downloading without encryption is acceptable, use an ftp:// link instead"
    )]
    #[non_exhaustive]
    FtpTlsUnavailable {
        /// The server's host name or address.
        host: String,
    },

    /// A proxy is set in the environment (for example `ALL_PROXY` or
    /// `FTP_PROXY`), but FTP downloads cannot go through a proxy yet, and
    /// going around it without asking could expose what it is there to hide.
    #[error(
        "FTP downloads cannot go through a proxy yet, and the {variable} environment variable sets one. To download this link directly instead, remove {variable} from the environment"
    )]
    #[non_exhaustive]
    FtpProxyUnsupported {
        /// The environment variable that sets the proxy.
        variable: String,
    },

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
            // FTP's 4xx replies are "transient negative completion" (RFC 959):
            // too many connections, data connection failed, file busy.
            Error::FtpReply { code, .. } => (400..500).contains(code),
            _ => false,
        }
    }

    pub(crate) fn network(error: reqwest::Error) -> Self {
        Error::Network(NetworkError(Inner::Http(error)))
    }

    /// A socket error while talking to a server (not a disk error). `what`
    /// says what was being done; `connecting` whether the connection was still
    /// being opened.
    pub(crate) fn socket(what: impl Into<String>, error: io::Error, connecting: bool) -> Self {
        Error::Network(NetworkError(Inner::Io {
            what: what.into(),
            error,
            connecting,
        }))
    }

    /// A failure of another transport library (QUIC, HTTP/3).
    #[cfg(feature = "http3")]
    pub(crate) fn transport(
        what: impl Into<String>,
        error: impl std::error::Error + Send + Sync + 'static,
        connecting: bool,
    ) -> Self {
        Error::Network(NetworkError(Inner::Other {
            what: what.into(),
            error: Box::new(error),
            connecting,
        }))
    }

    /// For a background task that panicked or was aborted.
    pub(crate) fn task(error: tokio::task::JoinError) -> Self {
        Error::Internal(format!("a background task failed: {error}"))
    }
}

/// What to add to an FTP error reply to say how to fix it.
fn ftp_reply_hint(code: u16) -> &'static str {
    match code {
        550 => {
            ". Check that the file exists. The path in the link is relative to the folder the server logs in to; to give a path from the top folder, start it with %2F, as in ftp://host/%2Fpub/file.iso"
        }
        _ => "",
    }
}

fn login_refused_text(host: &str, code: u16, message: &str, anonymous: bool) -> String {
    if anonymous {
        format!(
            "the FTP server {host} did not accept an anonymous login (\"{code} {message}\"); it may not allow anonymous access. Add a user name and password to the link, as in ftp://user:password@{host}/path"
        )
    } else {
        format!(
            "the FTP server {host} did not accept the login (\"{code} {message}\"). Check the user name and password in the link"
        )
    }
}

/// A failed connection or request. The message says what failed, including
/// the underlying cause (for example "connection refused" or a certificate
/// problem); [`std::error::Error::source`] gives the chain.
pub struct NetworkError(Inner);

enum Inner {
    /// From the HTTP client.
    Http(reqwest::Error),
    /// A socket error, for example on an FTP connection.
    Io {
        what: String,
        error: io::Error,
        connecting: bool,
    },
    /// From another transport library, such as QUIC for HTTP/3.
    #[cfg(feature = "http3")]
    Other {
        what: String,
        error: Box<dyn std::error::Error + Send + Sync>,
        connecting: bool,
    },
}

impl NetworkError {
    /// Whether a time limit ran out (connecting, or waiting for the server).
    pub fn is_timeout(&self) -> bool {
        match &self.0 {
            Inner::Http(e) => e.is_timeout(),
            Inner::Io { error, .. } => error.kind() == io::ErrorKind::TimedOut,
            #[cfg(feature = "http3")]
            Inner::Other { .. } => false,
        }
    }

    /// Whether the connection could not be opened at all (name lookup, refused
    /// connection, TLS handshake).
    pub fn is_connect(&self) -> bool {
        match &self.0 {
            Inner::Http(e) => e.is_connect(),
            Inner::Io { connecting, .. } => *connecting,
            #[cfg(feature = "http3")]
            Inner::Other { connecting, .. } => *connecting,
        }
    }

    /// The first error in the chain below the one this error displays itself.
    fn first_cause(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            Inner::Http(e) => std::error::Error::source(e),
            Inner::Io { error, .. } => error.get_ref().and_then(|e| e.source()),
            #[cfg(feature = "http3")]
            Inner::Other { error, .. } => error.source(),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Inner::Http(e) => write!(f, "{e}")?,
            Inner::Io { what, error, .. } => write!(f, "{what}: {error}")?,
            #[cfg(feature = "http3")]
            Inner::Other { what, error, .. } => write!(f, "{what}: {error}")?,
        }
        // The top message is often just "error sending request"; the deepest
        // cause says what actually happened.
        let mut cause = self.first_cause();
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
        match &self.0 {
            Inner::Http(e) => fmt::Debug::fmt(e, f),
            Inner::Io { what, error, .. } => write!(f, "{what}: {error:?}"),
            #[cfg(feature = "http3")]
            Inner::Other { what, error, .. } => write!(f, "{what}: {error:?}"),
        }
    }
}

impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            // The HTTP client's error is displayed as this one, so its own
            // source comes next.
            Inner::Http(e) => std::error::Error::source(e),
            Inner::Io { error, .. } => Some(error),
            #[cfg(feature = "http3")]
            Inner::Other { error, .. } => Some(error.as_ref()),
        }
    }
}

/// A `Result` whose error defaults to [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
