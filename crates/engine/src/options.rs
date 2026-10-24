//! The tuning of a download, and the limits it must stay within.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::error::{Error, Result};
use crate::limiter::RateLimiter;
use crate::net_guard::PrivateNetwork;
use crate::units::{MIB, MIB_USIZE};

/// Most connections one download may open ([`DownloadOptions::connections`]). More only adds load on the server and on this machine.
pub const MAX_CONNECTIONS: usize = 64;
/// Largest buffer one connection may fill before writing ([`DownloadOptions::buffer_size`]): 64 MiB. Each connection holds two, so this bounds memory, and a larger one would gain nothing.
pub const MAX_BUFFER_SIZE: usize = 64 * MIB_USIZE;

/// Tuning for a download. The defaults suit most connections.
///
/// Start from [`DownloadOptions::default`] and change what you need:
///
/// ```
/// # use wirecatch_engine::DownloadOptions;
/// let mut options = DownloadOptions::default();
/// options.connections = 16;
/// ```
///
/// New options may be added in minor releases, which is why the struct cannot
/// be built field by field outside this crate.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DownloadOptions {
    /// Most parallel connections to open (default 8). Fewer are used when the
    /// file is too small to split that often, when the server does not accept
    /// byte ranges, or when the server refuses extra connections.
    pub connections: usize,
    /// A range is only split if both halves would be at least this big, in
    /// bytes (default 1 MiB).
    pub min_split: u64,
    /// Bytes collected per connection before each disk write (default 1 MiB).
    pub buffer_size: usize,
    /// Consecutive failed attempts without progress before a connection gives
    /// up (default 6).
    pub max_retries: u32,
    /// How long to wait for the server, either for the answer to a request or
    /// for more data while receiving, before the attempt counts as failed and
    /// is retried (default 30 s).
    pub stall_timeout: Duration,
    /// How long opening a connection may take, TLS handshake included
    /// (default 15 s).
    pub connect_timeout: Duration,
    /// The `User-Agent` sent with every request (default `wirecatch-engine/<version>`, the
    /// version of this crate; an application sets its own).
    /// A `User-Agent` in [`headers`](Self::headers) takes precedence.
    pub user_agent: String,
    /// Extra request headers, such as the cookies, referer and user agent a
    /// browser hands over. Only used for `http` and `https` links; FTP has no
    /// headers (its login comes from the URL).
    ///
    /// They are sent in full only to the origin (scheme, host and port) of
    /// [`DownloadRequest::url`](crate::DownloadRequest::url). If that URL redirects to another origin, only
    /// headers that carry no credentials follow: `User-Agent`, `Accept`,
    /// `Accept-Language`, `Cache-Control`, `DNT`, `Pragma`, and `Referer` cut
    /// down to its origin (`https://site.example/`), as browsers do. Cookies,
    /// `Authorization` and any other header stay with their origin.
    ///
    /// A `user:password@` in the URL becomes an `Authorization: Basic` header
    /// bound the same way, unless this map already has an `Authorization`.
    ///
    /// Proxy credentials do not belong here: a `Proxy-Authorization` header is
    /// sent to the server, not to a proxy. Put them in the proxy URL instead,
    /// for example `HTTPS_PROXY=http://user:password@proxy.example:8080`.
    pub headers: HeaderMap,
    /// How often the resume file is rewritten while downloading (default 1 s).
    /// A killed process loses at most this much progress.
    pub checkpoint_interval: Duration,
    /// Speed limits this download obeys, for example a global one and its
    /// queue's. Every limiter in the list applies (default none).
    pub rate_limiters: Vec<Arc<RateLimiter>>,
    /// Follow a redirect from a secure `https` address to an insecure `http`
    /// one (default `false`: the download stops with
    /// [`Error::InsecureRedirect`] instead, because anyone on the network could
    /// read or change the file).
    pub allow_insecure_redirects: bool,
    /// Use HTTP/3 (QUIC) when the server offers it. **Experimental**, default
    /// `false`.
    ///
    /// Has an effect only when this crate is built with its `http3` feature;
    /// without it the option is accepted and ignored. When on, the first
    /// request still goes over HTTP/2 or HTTP/1.1. If the server's answer
    /// advertises HTTP/3 on the same host (an `Alt-Svc: h3=":port"` header),
    /// the other connections use HTTP/3, each over a QUIC connection of its
    /// own. Any QUIC failure (blocked UDP, a failed handshake, a broken
    /// stream) quietly switches the download back to HTTP/2 or HTTP/1.1 and
    /// it carries on from where it was. Not used when a proxy is set for
    /// `https` (`HTTPS_PROXY` or `ALL_PROXY` in the environment, or the
    /// system's proxy setting) and its exceptions do not name the server,
    /// since QUIC would go around the proxy.
    ///
    /// Off by default because on fast links several TCP connections are
    /// already as fast, and QUIC costs more processor time per byte.
    pub http3: bool,
    /// Whether an `http` or `https` download may reach the local network
    /// (loopback, private, link-local, shared CGNAT and every other address
    /// that is not [`AddressKind::Public`](crate::AddressKind::Public)). Default
    /// [`PrivateNetwork::Allow`]: a link the user gave may point anywhere.
    ///
    /// With [`PrivateNetwork::Block`], or [`PrivateNetwork::Auto`] when the
    /// link's host is on the internet, the download follows the same rules as a
    /// [`Fetcher`](crate::Fetcher): every request and redirect hop is checked
    /// first (to the link's own origin too, unless its host is written as an
    /// address), and every connection resolves names through a filter that
    /// never hands out a local address, so a name that changes its address
    /// later (DNS rebinding) cannot reach one either (see [`PrivateNetwork`]
    /// on the starting URL). HTTP/3 is not used then, as QUIC connections do not go
    /// through that filter. `ftp` links are not covered.
    pub private_network: PrivateNetwork,
    /// The most bytes one HTTP request may ask for (default `None`: no limit).
    ///
    /// Some servers slow down any request for more than a certain amount; video
    /// sites do this to make players fetch a little at a time (yt-dlp reports
    /// it as a format's `http_chunk_size`). With `Some(n)`, no request over
    /// HTTP/1.1, HTTP/2 or HTTP/3 asks for more than `n` bytes: the first
    /// request asks for `bytes=0-(n-1)` and learns the size from its
    /// `Content-Range`, and each connection fetches a larger range as a series
    /// of requests of at most `n` bytes, one after the other on the same
    /// connection. Splitting, resuming, retries, speed limits and the stall
    /// timeout work exactly as without it.
    ///
    /// A server that does not accept byte ranges is downloaded in one request
    /// as before, and so is a file whose size the server does not say. FTP
    /// ignores this option. `Some(0)` is the same as `None`. Only
    /// [`download`](fn@crate::download) uses it: a [`Fetcher`](crate::Fetcher) sends the requests
    /// its caller asks for.
    pub max_request_bytes: Option<u64>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connections: 8,
            min_split: MIB,
            buffer_size: MIB_USIZE,
            max_retries: 6,
            stall_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(15),
            user_agent: concat!("wirecatch-engine/", env!("CARGO_PKG_VERSION")).to_string(),
            headers: HeaderMap::new(),
            checkpoint_interval: Duration::from_secs(1),
            rate_limiters: Vec::new(),
            allow_insecure_redirects: false,
            http3: false,
            private_network: PrivateNetwork::Allow,
            max_request_bytes: None,
        }
    }
}

impl DownloadOptions {
    /// Checks that every option can work. [`download`](fn@crate::download) and [`Fetcher`](crate::Fetcher) call this before sending anything, so a zero connection count, split size, buffer size, timeout or checkpoint interval is an error instead of a hang or a panic, and so are more than [`MAX_CONNECTIONS`] connections or a buffer over [`MAX_BUFFER_SIZE`], which would exhaust memory.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidOption`] naming the first option that cannot work.
    pub fn validate(&self) -> Result<()> {
        let in_range = [
            (
                "connections",
                (1..=MAX_CONNECTIONS).contains(&self.connections),
            ),
            ("min_split", self.min_split > 0),
            (
                "buffer_size",
                (1..=MAX_BUFFER_SIZE).contains(&self.buffer_size),
            ),
            ("stall_timeout", !self.stall_timeout.is_zero()),
            ("connect_timeout", !self.connect_timeout.is_zero()),
            ("checkpoint_interval", !self.checkpoint_interval.is_zero()),
        ];
        match in_range.into_iter().find(|&(_, ok)| !ok) {
            Some((option, _)) => Err(Error::InvalidOption(option)),
            None => Ok(()),
        }
    }

    /// [`max_request_bytes`](Self::max_request_bytes) as a limit, `None` when
    /// there is none.
    pub(crate) fn request_limit(&self) -> Option<u64> {
        self.max_request_bytes.filter(|&n| n > 0)
    }
}
