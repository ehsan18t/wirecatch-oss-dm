//! The [`Fetcher`]: single requests that follow the same rules as a download.

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Response, StatusCode};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use url::Url;

use crate::error::{Chain, Error, IoAction, IoContext, Result};
use crate::http::{self, Clients, Failure, RequestRange, Session};
use crate::limiter::{Paced, RateLimiter, pace};
use crate::net_guard::{PrivateNetwork, private_guard};
use crate::numeric::clamp_to_len;
use crate::options::DownloadOptions;
use crate::receive::{Received, receive, skip_start};
use crate::retry::{Attempts, Next, Refusals};
use crate::units::KIB_USIZE;

/// Size of the write buffer used by [`Fetcher::save`].
const SAVE_BUFFER: usize = 256 * KIB_USIZE;

/// Makes requests that share one set of credentials, limits and rules.
///
/// [`download`](fn@crate::download) handles one large file. Some callers instead
/// need many small requests that belong together: the playlist of a video
/// stream, its decryption keys and its segments. A [`Fetcher`] makes those
/// requests with everything a download already does:
///
/// - Credentials in [`DownloadOptions::headers`] (and a `user:password@` in the
///   URL it was created with) are sent only to the origin (scheme, host and
///   port) of that URL. Requests to any other origin, such as a CDN that serves
///   the segments, carry only headers that hold no credentials, exactly as a
///   download that redirects to another origin does.
/// - A redirect from `https` to `http` is refused unless
///   [`DownloadOptions::allow_insecure_redirects`] is set.
/// - Network errors, timeouts and busy answers (`408`, `425`, `429`, `500`,
///   `502`, `503`, `504`) are retried with growing pauses, up to
///   [`DownloadOptions::max_retries`] attempts in a row without progress. A
///   `Retry-After` on a `429` or `503` is honored. An interrupted transfer
///   continues from the byte it reached instead of starting over.
/// - A server that stops sending for [`DownloadOptions::stall_timeout`] counts
///   as a failed attempt, and cancelling the token stops every request at once.
/// - Every byte counts against [`DownloadOptions::rate_limiters`].
/// - When the URL it was created with is on the internet, requests to the
///   local network (loopback, private, link-local, shared CGNAT and other
///   non-public addresses, whether written as an IP, also inside an IPv6
///   address, or reached through a name or a redirect) are
///   refused with [`Error::PrivateNetwork`]: a playlist from a website must
///   not make the program reach the router or other devices at home. See
///   [`PrivateNetwork`].
///
/// ```no_run
/// # async fn example() -> wirecatch_engine::Result<()> {
/// use wirecatch_engine::{CancellationToken, DownloadOptions, Fetcher, Url};
/// use wirecatch_engine::units::MIB;
///
/// let playlist = Url::parse("https://video.example/stream/index.m3u8").unwrap();
/// let fetcher = Fetcher::new(&playlist, &DownloadOptions::default(), CancellationToken::new()).await?;
///
/// // A small resource, read into memory, refused if larger than 1 MiB.
/// let text = fetcher.get(fetcher.url(), None, MIB).await?;
///
/// // A large one, streamed to disk; here only its first 1000 bytes.
/// let segment = text.url.join("segment1.ts").unwrap();
/// fetcher
///     .save(&segment, Some(0..1000), "segment1.ts".as_ref(), |bytes| println!("+{bytes}"))
///     .await?;
/// # Ok(())
/// # }
/// ```
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
        let protection = private_guard(
            &url,
            private_network,
            options.stall_timeout,
            clients.environment(),
        )
        .await;
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
            Sink::File { .. } => Bytes::new(),
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
    /// Cancelling (by the token or by dropping this future) keeps what arrived
    /// in the file; once the future has been polled, the file may already have
    /// been created or truncated even if no byte arrived.
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
        let file = tokio::fs::File::create(path)
            .await
            .context(IoAction::CreateFile, path)?;
        let mut sink = Sink::File {
            file: BufWriter::with_capacity(SAVE_BUFFER, file),
            path: path.to_path_buf(),
        };
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
        let mut attempts = Attempts::new(Refusals::AsFailures, self.inner.max_retries);
        loop {
            let before = position.written;
            let attempt = self
                .attempt(client, url, &mut position, sink, on_bytes)
                .await;
            let f = match attempt {
                Ok(answer) => return Ok(answer),
                Err(_) if self.inner.cancel.is_cancelled() => return Err(Error::Cancelled),
                Err(f) => f,
            };
            let progressed = position.written > before;
            let delay = match attempts.after_failure(&f.error, f.retry_after, progressed) {
                Next::GiveUp => return Err(f.error),
                Next::Retry(delay) | Next::StepBackOrRetry(delay) => delay,
            };
            warn!(error = %Chain(&f.error), %url, retry_in = ?delay, "request failed, retrying");
            tokio::select! {
                () = self.inner.cancel.cancelled() => return Err(Error::Cancelled),
                () = tokio::time::sleep(delay) => {}
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
        let mut response = response;
        loop {
            let at = position.base + position.written;
            if position.end.is_some_and(|end| at >= end) {
                break;
            }
            let next = async { response.chunk().await.map_err(Error::network) };
            let chunk = match receive(&self.inner.cancel, stall, next).await? {
                Received::Data(chunk) => chunk,
                Received::End => break,
                Received::Cancelled => return Err(Error::Cancelled.into()),
            };
            let mut data = skip_start(&chunk, &mut skip);
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
            if pace(&self.inner.rate_limiters, n, &self.inner.cancel).await == Paced::Cancelled {
                return Err(Error::Cancelled.into());
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
        let range = RequestRange::up_to(from, end)
            .ok_or_else(|| http::empty_range(from, end.unwrap_or(from)))?;
        let range = (!range.is_whole()).then_some(range);
        let response = http::send(client, url, &self.inner.session, range).await?;
        match (response.status(), range) {
            (StatusCode::OK, None) => Ok((response, 0)),
            // The server ignores ranges and sends everything.
            (StatusCode::OK, Some(_)) => Ok((response, from)),
            (StatusCode::PARTIAL_CONTENT, Some(_)) => {
                http::check_partial(response.headers(), from, None)?;
                Ok((response, 0))
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
    File {
        file: BufWriter<tokio::fs::File>,
        /// Where it is, for errors.
        path: PathBuf,
    },
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
            Sink::File { file, path } => file.write_all(data).await.context(IoAction::Write, path),
        }
    }

    async fn finish(&mut self) -> Result<()> {
        if let Sink::File { file, path } = self {
            file.flush().await.context(IoAction::Write, path)?;
        }
        Ok(())
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
