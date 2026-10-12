//! Single requests that follow the same rules as a download.
//!
//! [`download`](crate::download) handles one large file. Some callers instead
//! need many small requests that belong together: the playlist of a video
//! stream, its decryption keys and its segments. A [`Fetcher`] makes those
//! requests with everything a download already does:
//!
//! - Credentials in [`DownloadOptions::headers`] (and a `user:password@` in the
//!   URL it was created with) are sent only to the origin (scheme, host and
//!   port) of that URL. Requests to any other origin, such as a CDN that serves
//!   the segments, carry only headers that hold no credentials, exactly as a
//!   download that redirects to another origin does.
//! - A redirect from `https` to `http` is refused unless
//!   [`DownloadOptions::allow_insecure_redirects`] is set.
//! - Network errors, timeouts and busy answers (`408`, `425`, `429`, `500`,
//!   `502`, `503`, `504`) are retried with growing pauses, up to
//!   [`DownloadOptions::max_retries`] attempts in a row without progress. A
//!   `Retry-After` on a `429` or `503` is honored. An interrupted transfer
//!   continues from the byte it reached instead of starting over.
//! - A server that stops sending for [`DownloadOptions::stall_timeout`] counts
//!   as a failed attempt, and cancelling the token stops every request at once.
//! - Every byte counts against [`DownloadOptions::rate_limiters`].
//! - When the URL it was created with is on the internet, requests to the
//!   local network (loopback, private, link-local, shared CGNAT and other
//!   non-public addresses, whether written as an IP, also inside an IPv6
//!   address, or reached through a name or a redirect) are
//!   refused with [`Error::PrivateNetwork`]: a playlist from a website must
//!   not make Wirecatch reach the router or other devices at home. See
//!   [`PrivateNetwork`].
//!
//! ```no_run
//! # async fn example() -> wirecatch_engine::Result<()> {
//! use wirecatch_engine::{CancellationToken, DownloadOptions, Fetcher, Url};
//!
//! let playlist = Url::parse("https://video.example/stream/index.m3u8").unwrap();
//! let fetcher = Fetcher::new(&playlist, &DownloadOptions::default(), CancellationToken::new()).await?;
//!
//! // A small resource, read into memory, refused if larger than 1 MiB.
//! let text = fetcher.get(fetcher.url(), None, 1024 * 1024).await?;
//!
//! // A large one, streamed to disk; here only its first 1000 bytes.
//! let segment = text.url.join("segment1.ts").unwrap();
//! fetcher
//!     .save(&segment, Some(0..1000), "segment1.ts".as_ref(), |bytes| println!("+{bytes}"))
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Response, StatusCode};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use url::{Host, Url};

use crate::download::DownloadOptions;
use crate::error::{Error, Result};
use crate::http::{self, Clients, Failure, Guard, Session};
use crate::limiter::RateLimiter;
use crate::numeric::clamp_to_len;
use crate::units::KIB_USIZE;
use crate::worker;

/// Size of the write buffer used by [`Fetcher::save`].
const SAVE_BUFFER: usize = 256 * KIB_USIZE;

/// Whether a [`Fetcher`] may send requests to the local network. These
/// addresses count as local:
///
/// - loopback (`127.0.0.0/8`, `::1`) and unspecified (`0.0.0.0/8`, `::`);
/// - private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`,
///   and the old site-local `fec0::/10`);
/// - link-local (`169.254.0.0/16`, `fe80::/10`);
/// - shared address space (`100.64.0.0/10`), used by carrier-grade NAT and by
///   VPNs such as Tailscale;
/// - benchmarking (`198.18.0.0/15`), multicast (`224.0.0.0/4`, `ff00::/8`),
///   and reserved and broadcast (`240.0.0.0/4`, so `255.255.255.255` too);
/// - local-use NAT64 (`64:ff9b:1::/48`);
/// - an IPv6 address that only carries an IPv4 one counts as that IPv4
///   address: IPv4-mapped (`::ffff:10.0.0.1`), IPv4-compatible (`::10.0.0.1`),
///   NAT64 (`64:ff9b::10.0.0.1`) and 6to4 (`2002:0a00:0001::`).
///
/// The origin the fetcher was created for is always allowed.
///
/// # With a proxy
///
/// When a proxy is set in the environment (`HTTP_PROXY`, `HTTPS_PROXY` or
/// `ALL_PROXY`, in upper or lower case), requests go through it, and the
/// proxy, not Wirecatch, looks up each name and connects. The guard then
/// still refuses, before every request and redirect hop, a target written as a
/// local address and a target whose name resolves only to local addresses on
/// this computer. It cannot see how the proxy resolves a name, so a name that
/// the proxy resolves to a local address (because it changed after the check,
/// or because only the proxy knows it, such as an intranet host) is not
/// caught. The proxy itself is trusted, since the user or their administrator
/// set it, not a website: its own address may be local.
///
/// Requests are not refused outright while a proxy is set, because on
/// networks that need one, every stream would then fail; such a proxy is the
/// network's own gateway and applies its own rules. Use
/// [`PrivateNetwork::Allow`] if those rules should decide alone, or unset the
/// proxy for full protection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrivateNetwork {
    /// Block when the fetcher's own URL is on the internet (the usual case
    /// for a video from a website); allow when it is itself on the local
    /// network, such as a media server at home.
    #[default]
    Auto,
    /// Always block (except the fetcher's own origin).
    Block,
    /// Never block.
    Allow,
}

/// Makes requests that share one set of credentials, limits and rules; see the
/// [module documentation](self).
///
/// It is cheap to clone, and clones share everything, so one `Fetcher` can be
/// used from many tasks at once. It opens up to
/// [`DownloadOptions::connections`] separate connections, used in turn, so that
/// that many requests in parallel really travel over separate connections.
#[derive(Clone)]
pub struct Fetcher {
    inner: Arc<Inner>,
}

struct Inner {
    session: Session,
    url: Url,
    clients: Vec<Client>,
    next: AtomicUsize,
    max_retries: u32,
    stall_timeout: Duration,
    rate_limiters: Vec<Arc<RateLimiter>>,
    cancel: CancellationToken,
    /// Set when requests to the local network are blocked.
    blocking_private: bool,
}

impl fmt::Debug for Fetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fetcher")
            .field("url", &self.inner.url)
            .field("connections", &self.inner.clients.len())
            .field("blocking_private", &self.inner.blocking_private)
            .finish_non_exhaustive()
    }
}

/// A resource read into memory by [`Fetcher::get`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Fetched {
    /// The bytes that were asked for.
    pub body: Bytes,
    /// The address that answered, after redirects. Relative links inside the
    /// resource (for example in a playlist) are relative to this one.
    pub url: Url,
    /// The `Content-Type` the server sent, if any.
    pub content_type: Option<String>,
}

impl Fetcher {
    /// A fetcher whose credentials belong to the origin of `url`.
    ///
    /// Headers, user agent, timeouts, retries, speed limits and the redirect
    /// rule come from `options`. Cancelling `cancel` stops every request made
    /// through this fetcher, with [`Error::Cancelled`]. Requests to the local
    /// network follow [`PrivateNetwork::Auto`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidUrl`] if `url` is not an `http` or `https` address,
    /// [`Error::InvalidOption`] if an option cannot work (see
    /// [`DownloadOptions::validate`]), or a network error if the HTTP client
    /// cannot be set up.
    pub async fn new(
        url: &Url,
        options: &DownloadOptions,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Self::with_private_network(url, options, cancel, PrivateNetwork::Auto).await
    }

    /// Like [`Fetcher::new`], with an explicit rule for requests to the local
    /// network.
    ///
    /// # Errors
    ///
    /// As [`Fetcher::new`].
    pub async fn with_private_network(
        url: &Url,
        options: &DownloadOptions,
        cancel: CancellationToken,
        private_network: PrivateNetwork,
    ) -> Result<Self> {
        check_scheme(url)?;
        options.validate()?;
        // The first use in a process reads the system certificate store, which
        // blocks, so that happens off the async threads.
        let clients = if Clients::shared_ready() {
            Clients::new(options)?
        } else {
            let options = options.clone();
            tokio::task::spawn_blocking(move || Clients::new(&options))
                .await
                .map_err(|e| Error::task(&e))??
        };
        let (mut session, url) = Session::new(url, options, cancel.clone());
        let protection = private_guard(&url, private_network, options.stall_timeout).await;
        let blocking_private = protection.is_some();
        let resolver = protection.map(|p| {
            session.guard = Some(p.guard);
            p.resolver
        });
        let clients = (0..options.connections.max(1))
            .map(|_| match &resolver {
                // Names are resolved through the filter, so even a name that
                // changes its address after the check cannot reach the local
                // network.
                Some(resolver) => clients
                    .builder()
                    .dns_resolver(resolver.clone())
                    .build()
                    .map_err(Error::network),
                None => clients.build(),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            inner: Arc::new(Inner {
                session,
                url,
                clients,
                next: AtomicUsize::new(0),
                max_retries: options.max_retries,
                stall_timeout: options.stall_timeout,
                rate_limiters: options.rate_limiters.clone(),
                cancel,
                blocking_private,
            }),
        })
    }

    /// The URL this fetcher was created with, without any `user:password@`
    /// (those became an `Authorization` header for its origin).
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.inner.url
    }

    /// Whether requests to the local network are refused (see
    /// [`PrivateNetwork`]).
    #[must_use]
    pub fn blocks_private_network(&self) -> bool {
        self.inner.blocking_private
    }

    /// Reads `url` into memory: the whole resource, or only the bytes in
    /// `range` (start inclusive, end exclusive).
    ///
    /// Meant for small resources such as playlists and keys. An answer larger
    /// than `limit` bytes is refused with [`Error::TooLarge`] without reading
    /// the rest. A server that ignores the range is handled: the bytes before
    /// it are skipped and the transfer stops at its end.
    ///
    /// # Errors
    ///
    /// The same errors as a download: [`Error::HttpStatus`] for an answer such
    /// as `404`, network errors once the retries ran out, [`Error::Cancelled`]
    /// when cancelled, and [`Error::TooLarge`] as described above.
    pub async fn get(&self, url: &Url, range: Option<Range<u64>>, limit: u64) -> Result<Fetched> {
        let url = clean_url(url)?;
        if let Some(range) = &range {
            let len = range.end.saturating_sub(range.start);
            if len > limit {
                return Err(Error::TooLarge {
                    url: url.to_string(),
                    limit,
                });
            }
            if len == 0 {
                return Ok(Fetched {
                    body: Bytes::new(),
                    url,
                    content_type: None,
                });
            }
        }
        let mut sink = Sink::Memory {
            buf: BytesMut::new(),
            limit,
            url: url.to_string(),
        };
        let answer = self.transfer(&url, range, &mut sink, &mut |_| {}).await?;
        let body = match sink {
            Sink::Memory { buf, .. } => buf.freeze(),
            Sink::File(_) => Bytes::new(),
        };
        Ok(Fetched {
            body,
            url: answer.url,
            content_type: answer.content_type,
        })
    }

    /// Streams `url` into the file at `path`: the whole resource, or only the
    /// bytes in `range` (start inclusive, end exclusive). Returns how many
    /// bytes were written.
    ///
    /// The file is created, or truncated if it exists. `on_bytes` is called
    /// with the number of new bytes each time some arrive; bytes are never
    /// counted twice, even when a retry has to ask for the resource again.
    ///
    /// # Errors
    ///
    /// The same as [`Fetcher::get`] (without the size limit), plus
    /// [`Error::Io`] when the file cannot be written. Whatever arrived before
    /// an error or a cancellation stays in the file.
    pub async fn save(
        &self,
        url: &Url,
        range: Option<Range<u64>>,
        path: &Path,
        mut on_bytes: impl FnMut(u64) + Send,
    ) -> Result<u64> {
        let url = clean_url(url)?;
        let file = tokio::fs::File::create(path).await?;
        let mut sink = Sink::File(BufWriter::with_capacity(SAVE_BUFFER, file));
        if range.as_ref().is_some_and(|r| r.end <= r.start) {
            return Ok(0);
        }
        let answer = self.transfer(&url, range, &mut sink, &mut on_bytes).await;
        // Whatever arrived is kept, also after an error.
        let flushed = sink.finish().await;
        let answer = answer?;
        flushed?;
        Ok(answer.written)
    }

    /// The next connection, in turn.
    fn client(&self) -> &Client {
        let index = self.inner.next.fetch_add(1, Ordering::Relaxed) % self.inner.clients.len();
        &self.inner.clients[index]
    }

    /// Moves `[range.start, range.end)` (or the whole resource) into `sink`,
    /// retrying from wherever it got to.
    async fn transfer(
        &self,
        url: &Url,
        range: Option<Range<u64>>,
        sink: &mut Sink,
        on_bytes: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Answer> {
        let client = self.client();
        let mut position = Position {
            base: range.as_ref().map_or(0, |r| r.start),
            end: range.map(|r| r.end),
            written: 0,
        };
        let mut failures = 0u32;
        loop {
            let before = position.written;
            let attempt = self
                .attempt(client, url, &mut position, sink, on_bytes)
                .await;
            match attempt {
                Ok(answer) => return Ok(answer),
                Err(_) if self.inner.cancel.is_cancelled() => return Err(Error::Cancelled),
                Err(f) if f.error.is_retryable() && failures < self.inner.max_retries => {
                    failures = if position.written > before {
                        0
                    } else {
                        failures + 1
                    };
                    let delay = worker::retry_delay(failures.max(1), f.retry_after);
                    warn!(error = %f.error, %url, retry_in = ?delay, "request failed, retrying");
                    tokio::select! {
                        () = self.inner.cancel.cancelled() => return Err(Error::Cancelled),
                        () = tokio::time::sleep(delay) => {}
                    }
                }
                Err(f) => return Err(f.error),
            }
        }
    }

    /// One request, streamed into `sink` until the wanted bytes are in, the
    /// body ends, or something fails.
    async fn attempt(
        &self,
        client: &Client,
        url: &Url,
        position: &mut Position,
        sink: &mut Sink,
        on_bytes: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Answer, Failure> {
        let from = position.base + position.written;
        let (response, mut skip) = self.open(client, url, from, position.end).await?;
        if position.end.is_none() {
            // The size, once known, lets a later retry ask for exactly the rest
            // and tells a complete body from one cut short.
            position.end = known_end(&response, from, skip);
        }
        if let Some(end) = position.end {
            sink.expect(end.saturating_sub(from))?;
        }
        let answer_url = response.url().clone();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let stall = self.inner.stall_timeout;
        let mut body = response.bytes_stream();
        loop {
            let at = position.base + position.written;
            if position.end.is_some_and(|end| at >= end) {
                break;
            }
            let next = tokio::select! {
                biased;
                () = self.inner.cancel.cancelled() => return Err(Error::Cancelled.into()),
                next = tokio::time::timeout(stall, body.next()) => next,
            };
            let chunk = match next {
                Err(_) => return Err(Error::Stalled(stall.as_secs()).into()),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(Error::network(e).into()),
                Ok(Some(Ok(chunk))) => chunk,
            };
            let mut data = &chunk[..];
            if skip > 0 {
                let n = clamp_to_len(skip, data.len());
                data = &data[n..];
                skip -= n as u64;
            }
            if let Some(end) = position.end {
                data = &data[..clamp_to_len(end.saturating_sub(at), data.len())];
            }
            if data.is_empty() {
                continue;
            }
            sink.write(data).await?;
            let n = data.len() as u64;
            position.written += n;
            on_bytes(n);
            for limiter in &self.inner.rate_limiters {
                limiter.consume(n).await;
            }
        }

        let at = position.base + position.written;
        if let Some(end) = position.end
            && at < end
        {
            return Err(Error::PrematureEof { at, end }.into());
        }
        Ok(Answer {
            url: answer_url,
            content_type,
            written: position.written,
        })
    }

    /// Asks for the bytes from `from` up to `end` (exclusive), or to the end of
    /// the resource. Returns the response and how many bytes at its start to
    /// skip: a server that ignores ranges sends the whole resource.
    async fn open(
        &self,
        client: &Client,
        url: &Url,
        from: u64,
        end: Option<u64>,
    ) -> Result<(Response, u64), Failure> {
        let range = match (from, end) {
            (0, None) => None,
            (from, None) => Some(format!("bytes={from}-")),
            (from, Some(end)) => Some(format!("bytes={from}-{}", end.saturating_sub(1))),
        };
        let response = http::send(client, url, &self.inner.session, range.as_deref()).await?;
        match (response.status(), range) {
            (StatusCode::OK, None) => Ok((response, 0)),
            // The server ignores ranges and sends everything.
            (StatusCode::OK, Some(_)) => Ok((response, from)),
            (StatusCode::PARTIAL_CONTENT, Some(_)) => {
                let got = http::content_range(response.headers()).map(|c| c.start);
                if got == Some(from) {
                    Ok((response, 0))
                } else {
                    Err(Error::RangeNotHonored {
                        expected: from,
                        got,
                    }
                    .into())
                }
            }
            _ => Err(Failure::status(&response)),
        }
    }
}

/// Where a transfer stands, in offsets of the remote resource.
struct Position {
    /// First byte asked for.
    base: u64,
    /// One past the last byte wanted, once known.
    end: Option<u64>,
    /// Bytes delivered to the sink so far.
    written: u64,
}

/// What a finished transfer learned.
struct Answer {
    url: Url,
    content_type: Option<String>,
    written: u64,
}

/// The end of the resource as the response tells it, in resource offsets.
fn known_end(response: &Response, from: u64, skip: u64) -> Option<u64> {
    if response.status() == StatusCode::PARTIAL_CONTENT {
        return http::content_range(response.headers()).and_then(|c| c.total);
    }
    // A 200 sends the resource from its start; `skip` is `from` in that case.
    debug_assert!(skip == from || from == 0);
    response.content_length()
}

/// Where a transfer's bytes go.
enum Sink {
    Memory {
        buf: BytesMut,
        limit: u64,
        url: String,
    },
    File(BufWriter<tokio::fs::File>),
}

impl Sink {
    /// Prepares for `len` more bytes; refuses them if they would not fit.
    fn expect(&mut self, len: u64) -> Result<()> {
        if let Sink::Memory { buf, limit, url } = self {
            // `len` comes from the server, so it may be anything up to u64::MAX.
            if (buf.len() as u64).saturating_add(len) > *limit {
                return Err(Error::TooLarge {
                    url: url.clone(),
                    limit: *limit,
                });
            }
            // Only room set aside in advance: a size that does not fit in memory
            // is left to grow as data arrives, and is refused by the limit first.
            if let Ok(len) = usize::try_from(len) {
                buf.reserve(len);
            }
        }
        Ok(())
    }

    async fn write(&mut self, data: &[u8]) -> Result<()> {
        match self {
            Sink::Memory { buf, limit, url } => {
                if (buf.len() + data.len()) as u64 > *limit {
                    return Err(Error::TooLarge {
                        url: url.clone(),
                        limit: *limit,
                    });
                }
                buf.extend_from_slice(data);
                Ok(())
            }
            Sink::File(file) => Ok(file.write_all(data).await?),
        }
    }

    async fn finish(&mut self) -> Result<()> {
        if let Sink::File(file) = self {
            file.flush().await?;
        }
        Ok(())
    }
}

/// Whether an address is on the local network or otherwise not a public
/// internet host; see [`PrivateNetwork`] for the list. An IPv6 address that
/// carries an IPv4 one (mapped, compatible, NAT64 or 6to4) counts as that
/// IPv4 address.
pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 0 // "this network", 0.0.0.0/8
                || v4.is_loopback() // 127.0.0.0/8
                || v4.is_private() // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254.0.0/16
                || (a == 100 && b & 0xc0 == 64) // shared (CGNAT, Tailscale), 100.64.0.0/10
                || (a == 198 && b & 0xfe == 18) // benchmarking, 198.18.0.0/15
                || v4.is_multicast() // 224.0.0.0/4
                || a >= 240 // reserved and broadcast, 240.0.0.0/4
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = embedded_ipv4(v6) {
                return is_private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast() // ff00::/8
                || (first & 0xfe00) == 0xfc00 // unique local, fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local, fe80::/10
                || (first & 0xffc0) == 0xfec0 // old site-local, fec0::/10
                // Local-use NAT64, 64:ff9b:1::/48: translated by a gateway on
                // the local network, to wherever it was set up for.
                || v6.segments()[..3] == [0x64, 0xff9b, 1]
        }
    }
}

/// The IPv4 address inside an IPv6 one that is only a way of writing or
/// reaching it: IPv4-mapped `::ffff:a.b.c.d`, IPv4-compatible `::a.b.c.d`,
/// NAT64 `64:ff9b::a.b.c.d` and 6to4 `2002:aabb:ccdd::/48`.
fn embedded_ipv4(v6: std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    let last = |hi: u16, lo: u16| {
        let [first, second] = hi.to_be_bytes();
        let [third, fourth] = lo.to_be_bytes();
        std::net::Ipv4Addr::new(first, second, third, fourth)
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }
    match v6.segments() {
        // `::` and `::1` are themselves, not IPv4 addresses.
        [0, 0, 0, 0, 0, 0, 0, 0 | 1] => None,
        [0, 0, 0, 0, 0, 0, hi, lo] | [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] | [0x2002, hi, lo, ..] => {
            Some(last(hi, lo))
        }
        _ => None,
    }
}

/// Every address `host` resolves to, or nothing if it does not resolve (the
/// request then fails on its own).
async fn resolve(host: &str, port: u16, timeout: Duration) -> Vec<IpAddr> {
    match tokio::time::timeout(timeout, tokio::net::lookup_host((host, port))).await {
        Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
        _ => Vec::new(),
    }
}

/// The addresses of the host of `url`: the address itself, or what the name
/// resolves to (nothing if it does not resolve).
async fn host_addresses(url: &Url, timeout: Duration) -> Vec<IpAddr> {
    match url.host() {
        Some(Host::Ipv4(ip)) => vec![IpAddr::V4(ip)],
        Some(Host::Ipv6(ip)) => vec![IpAddr::V6(ip)],
        Some(Host::Domain(name)) => {
            let port = url.port_or_known_default().unwrap_or(80);
            resolve(name, port, timeout).await
        }
        None => Vec::new(),
    }
}

/// Local addresses the starting host may keep using once the guard is on.
/// Only a host that was entirely local (the user deliberately fetching from
/// their own network) keeps them; a host that mixed public and local answers
/// gets none, so a hostile name server cannot slip a local address in beside a
/// public one.
fn pinned_addresses(start: &[IpAddr]) -> Vec<IpAddr> {
    if all_private(start) {
        start.to_vec()
    } else {
        Vec::new()
    }
}

/// Whether a host with these addresses is on the local network: it has
/// addresses and all of them are local. A host that does not resolve counts as
/// public, which keeps the guard on.
fn all_private(addrs: &[IpAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|ip| is_private(*ip))
}

/// What keeps requests that start at one URL away from the local network: a
/// check before every request and redirect hop, and a name resolver that never
/// hands out a local address (so a name that changes its address after the
/// check, DNS rebinding, cannot reach one either). Used by the [`Fetcher`] and
/// by downloads with [`DownloadOptions::private_network`].
pub(crate) struct PrivateGuard {
    pub guard: Guard,
    pub resolver: Arc<FilteringResolver>,
}

/// The protection `rule` asks for, for requests that start at `url`; `None`
/// when local addresses are allowed.
pub(crate) async fn private_guard(
    url: &Url,
    rule: PrivateNetwork,
    timeout: Duration,
) -> Option<PrivateGuard> {
    if rule == PrivateNetwork::Allow {
        return None;
    }
    // The addresses the starting host has right now. Only these may be on the
    // local network later: a name that switches to a local address after this
    // check (DNS rebinding) is filtered like any other.
    let start_addrs = host_addresses(url, timeout).await;
    if rule == PrivateNetwork::Auto && all_private(&start_addrs) {
        return None;
    }
    Some(guard_from(url, &start_addrs, timeout))
}

/// The protection for requests starting at `url`, whose host had `start_addrs`.
pub(crate) fn guard_from(url: &Url, start_addrs: &[IpAddr], timeout: Duration) -> PrivateGuard {
    let check = Arc::new(NetworkGuard::new(url, timeout));
    PrivateGuard {
        guard: Guard(Arc::new(move |target: &Url| {
            let (check, target) = (check.clone(), target.clone());
            Box::pin(async move { check.check(&target).await })
        })),
        resolver: Arc::new(FilteringResolver {
            exempt: url.host_str().map(str::to_string),
            pinned: pinned_addresses(start_addrs),
            proxies: proxy_hosts(&|name| std::env::var_os(name)),
        }),
    }
}

/// The variables the HTTP client takes a proxy from.
const PROXY_VARIABLES: &[&str] = &[
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
];

/// The host names of the proxies set in the environment, as `env` reads it.
/// The HTTP client connects to these itself, so their names reach the
/// resolver; a proxy given as an IP address needs no entry.
fn proxy_hosts(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Vec<String> {
    let mut hosts = Vec::new();
    for name in PROXY_VARIABLES {
        let Some(value) = env(name) else { continue };
        let value = value.to_string_lossy();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        // Like the HTTP client, a value without a scheme is an http proxy.
        let parsed = if value.contains("://") {
            Url::parse(value)
        } else {
            Url::parse(&format!("http://{value}"))
        };
        if let Ok(url) = parsed
            && let Some(Host::Domain(host)) = url.host()
            && !hosts.iter().any(|h| h == host)
        {
            hosts.push(host.to_string());
        }
    }
    hosts
}

/// Refuses requests to the local network, before they are sent; checked for
/// every request and every redirect hop.
struct NetworkGuard {
    /// The fetcher's own origin, always allowed.
    origin: url::Origin,
    timeout: Duration,
    /// Names already checked, and whether they were refused.
    names: Mutex<HashMap<String, bool>>,
}

impl NetworkGuard {
    fn new(url: &Url, timeout: Duration) -> Self {
        Self {
            origin: url.origin(),
            timeout,
            names: Mutex::new(HashMap::new()),
        }
    }

    /// Locks the names already checked. A lock poisoned by a panic in another
    /// holder is taken over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives; the cache
    /// holds only finished answers, so it is never half written either.
    fn names(&self) -> MutexGuard<'_, HashMap<String, bool>> {
        self.names.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn check(&self, url: &Url) -> Result<()> {
        if url.origin() == self.origin {
            return Ok(());
        }
        let refused = match url.host() {
            Some(Host::Ipv4(ip)) => is_private(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => is_private(IpAddr::V6(ip)),
            Some(Host::Domain(name)) => {
                let known = self.names().get(name).copied();
                if let Some(refused) = known {
                    refused
                } else {
                    let port = url.port_or_known_default().unwrap_or(80);
                    let addrs = resolve(name, port, self.timeout).await;
                    // Refused when no address outside the local network
                    // is left, the same rule the resolver applies.
                    let refused = !addrs.is_empty() && addrs.into_iter().all(is_private);
                    self.names().insert(name.to_string(), refused);
                    refused
                }
            }
            None => false,
        };
        if refused {
            Err(Error::PrivateNetwork {
                url: url.to_string(),
            })
        } else {
            Ok(())
        }
    }
}

/// Resolves names like the system does, minus local network addresses: the
/// connection itself can then never reach one, even if a name's address
/// changes between the check and the connection.
pub(crate) struct FilteringResolver {
    /// The fetcher's own host.
    exempt: Option<String>,
    /// Local addresses the fetcher's own host had when the fetcher was created;
    /// the only local addresses it may use. Empty when it started public, so a
    /// later switch to a local address is filtered (DNS rebinding).
    pinned: Vec<IpAddr>,
    /// The proxies the user set (see [`proxy_hosts`]), resolved without the
    /// filter: the connection to a proxy is the user's choice, and a request
    /// that names one as its target is still checked by the [`NetworkGuard`].
    proxies: Vec<String>,
}

impl Resolve for FilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let trusted = self.proxies.iter().any(|p| p.eq_ignore_ascii_case(&host));
        let pinned = if self.exempt.as_deref() == Some(host.as_str()) {
            self.pinned.clone()
        } else {
            Vec::new()
        };
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| trusted || !is_private(a.ip()) || pinned.contains(&a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!(
                    "{host} points to a device on your own network, which Wirecatch blocks for safety"
                )
                .into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn check_scheme(url: &Url) -> Result<()> {
    if matches!(url.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(Error::InvalidUrl(format!(
            "{url} is not an http or https address"
        )))
    }
}

/// Refuses addresses that are not `http` or `https`, and drops a
/// `user:password@`: the fetcher's own origin already has its credentials as a
/// header, and credentials found in a link are never sent on their own.
fn clean_url(url: &Url) -> Result<Url> {
    check_scheme(url)?;
    Ok(crate::url::without_credentials(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_and_https_are_fetched() {
        for bad in ["file:///etc/passwd", "skd://key", "data:,abc", "ftp://x/y"] {
            let url = Url::parse(bad).unwrap();
            assert!(
                matches!(clean_url(&url), Err(Error::InvalidUrl(_))),
                "{bad}"
            );
        }
        let url = Url::parse("https://user:secret@cdn.example/a.ts").unwrap();
        assert_eq!(
            clean_url(&url).unwrap().as_str(),
            "https://cdn.example/a.ts"
        );
    }

    #[test]
    fn local_network_addresses_are_recognized() {
        for private in [
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.0.1",
            "::ffff:127.0.0.1",
            // Shared address space: carrier-grade NAT, Tailscale.
            "100.64.0.1",
            "100.100.100.100",
            "100.127.255.255",
            // Benchmarking.
            "198.18.0.1",
            "198.19.255.255",
            // Multicast, reserved and broadcast.
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "ff02::1",
            "ff05::1:3",
            // Old site-local IPv6.
            "fec0::1",
            // IPv4 inside IPv6: compatible, NAT64, local-use NAT64, 6to4.
            "::127.0.0.1",
            "::10.0.0.1",
            "::0.0.0.2",
            "64:ff9b::7f00:1",
            "64:ff9b::192.168.1.1",
            "64:ff9b::100.64.0.1",
            "64:ff9b:1::1",
            "64:ff9b:1:abcd::8.8.8.8",
            "2002:7f00:1::",
            "2002:c0a8:0101::1",
            "2002:a9fe:a9fe:1::5",
            "2002:6440:1::",
        ] {
            assert!(is_private(private.parse().unwrap()), "{private}");
        }
        for public in [
            "8.8.8.8",
            "172.32.0.1",
            "192.169.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "198.17.255.255",
            "198.20.0.1",
            "223.255.255.255",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
            "::8.8.8.8",
            "64:ff9b::8.8.8.8",
            "64:ff9b::93.184.215.14",
            "2002:0808:0808::1",
            "fe00::1",
            "2606:4700::1111",
        ] {
            assert!(!is_private(public.parse().unwrap()), "{public}");
        }
    }

    #[tokio::test]
    async fn the_guard_refuses_local_addresses_but_not_its_own_origin() {
        let start = Url::parse("http://127.0.0.1:8000/master.m3u8").unwrap();
        let guard = NetworkGuard::new(&start, Duration::from_secs(5));
        let ok = |u: &str| Url::parse(u).unwrap();
        assert!(
            guard
                .check(&ok("http://127.0.0.1:8000/seg.ts"))
                .await
                .is_ok()
        );
        for refused in [
            "http://127.0.0.1:8001/seg.ts",
            "http://10.0.0.1/seg.ts",
            "http://[::1]:8000/seg.ts",
            "http://169.254.169.254/latest/meta-data",
            "http://[fe80::1]/k",
            "https://192.168.1.1/key",
            "http://localhost:8000/seg.ts",
            "http://100.100.100.100/",
            "http://[64:ff9b::a9fe:a9fe]/latest/meta-data",
            "http://[2002:c0a8:101::1]/admin",
            "http://[::192.168.1.1]/",
            "http://255.255.255.255/",
        ] {
            let error = guard.check(&ok(refused)).await.unwrap_err();
            assert!(matches!(error, Error::PrivateNetwork { .. }), "{refused}");
        }
        assert!(
            guard
                .check(&ok("http://93.184.215.14/seg.ts"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn the_resolver_never_hands_out_local_addresses() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
            exempt: Some("own-host.invalid".into()),
            pinned: Vec::new(),
            proxies: Vec::new(),
        };
        // `localhost` resolves only to loopback addresses, all filtered out.
        let error = match resolver.resolve(Name::from_str("localhost").unwrap()).await {
            Ok(addrs) => panic!("resolved to {:?}", addrs.collect::<Vec<_>>()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blocks for safety"), "{error}");
        // The fetcher's own host keeps the local addresses it had at creation.
        let start = host_addresses(
            &Url::parse("http://localhost/").unwrap(),
            Duration::from_secs(5),
        )
        .await;
        assert!(all_private(&start), "localhost resolved to {start:?}");
        let resolver = FilteringResolver {
            exempt: Some("localhost".into()),
            pinned: start.clone(),
            proxies: Vec::new(),
        };
        let addrs: Vec<_> = resolver
            .resolve(Name::from_str("localhost").unwrap())
            .await
            .unwrap()
            .collect();
        assert!(!addrs.is_empty() && addrs.iter().all(|a| start.contains(&a.ip())));
    }

    /// A download's host looked public when it started; it now resolves to a
    /// local address, as a rebinding name server would make it. No connection
    /// of the download may reach that address.
    #[tokio::test]
    async fn a_download_cannot_be_rebound_to_the_local_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            while let Ok((_socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let url = Url::parse(&format!("http://localhost:{port}/video.mp4")).unwrap();
        // The addresses seen at the start: public (none of them local).
        let guard = guard_from(
            &url,
            &["93.184.215.14".parse().unwrap()],
            Duration::from_secs(5),
        );
        let dir = wirecatch_test_support::private_dir("rebind").unwrap();
        let mut request = crate::DownloadRequest::new(url, dir.path());
        request.options.max_retries = 0;
        request.options.connect_timeout = Duration::from_secs(5);
        let clients = crate::http::Clients::new(&request.options).unwrap();
        let result = crate::download::download_guarded(
            request,
            Arc::new(crate::Progress::default()),
            CancellationToken::new(),
            clients,
            Some(guard),
        )
        .await;
        assert!(result.is_err(), "the download went through");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            0,
            "a connection reached the local address"
        );
    }

    #[test]
    fn a_host_mixing_public_and_local_answers_keeps_no_local_address() {
        let public: IpAddr = "93.184.215.14".parse().unwrap();
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(
            pinned_addresses(&[public, local]),
            [] as [std::net::IpAddr; 0]
        );
        assert_eq!(pinned_addresses(&[public]), [] as [std::net::IpAddr; 0]);
        assert_eq!(pinned_addresses(&[]), [] as [std::net::IpAddr; 0]);
        assert_eq!(pinned_addresses(&[local]), vec![local]);
    }

    #[tokio::test]
    async fn the_own_host_cannot_switch_to_a_local_address_later() {
        use std::str::FromStr;
        // The own host looked public when the fetcher was created (no pinned
        // local addresses); it now resolves to loopback, as a rebinding DNS
        // server would make it. Those addresses must be filtered.
        let resolver = FilteringResolver {
            exempt: Some("localhost".into()),
            pinned: Vec::new(),
            proxies: Vec::new(),
        };
        let error = match resolver.resolve(Name::from_str("localhost").unwrap()).await {
            Ok(addrs) => panic!("rebinding got through: {:?}", addrs.collect::<Vec<_>>()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blocks for safety"), "{error}");
    }

    #[test]
    fn proxy_host_names_come_from_the_environment() {
        let env = |values: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                values
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| std::ffi::OsString::from(v))
            }
        };
        let hosts = proxy_hosts(&env(&[
            ("HTTPS_PROXY", "http://user:secret@Proxy.Corp.example:8080"),
            ("http_proxy", "proxy.corp.example:3128"),
            ("ALL_PROXY", "socks5://socks.lan:1080"),
            ("all_proxy", "http://10.0.0.5:8080"),
            ("HTTP_PROXY", "  "),
            ("https_proxy", "http://[::1]:8080"),
        ]));
        assert_eq!(hosts, ["socks.lan", "proxy.corp.example"]);
        assert_eq!(
            proxy_hosts(&env(&[("NO_PROXY", "localhost")])),
            [] as [std::string::String; 0]
        );
    }

    #[tokio::test]
    async fn the_proxy_the_user_set_may_be_local() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
            exempt: Some("own-host.invalid".into()),
            pinned: Vec::new(),
            proxies: vec!["localhost".into()],
        };
        let addrs: Vec<_> = resolver
            .resolve(Name::from_str("localhost").unwrap())
            .await
            .unwrap()
            .collect();
        assert!(!addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback()));
    }

    #[test]
    fn memory_sink_enforces_its_limit() {
        let mut sink = Sink::Memory {
            buf: BytesMut::new(),
            limit: 4,
            url: "http://x/".into(),
        };
        assert!(sink.expect(4).is_ok());
        assert!(matches!(
            sink.expect(5),
            Err(Error::TooLarge { limit: 4, .. })
        ));
        // A size near the largest number from a hostile server must not overflow.
        assert!(matches!(
            sink.expect(u64::MAX),
            Err(Error::TooLarge { limit: 4, .. })
        ));
    }
}
