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

use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Response, StatusCode};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use url::Url;

use crate::download::DownloadOptions;
use crate::error::{Error, Result};
use crate::http::{self, Clients, Failure, Session};
use crate::limiter::RateLimiter;
use crate::worker;

/// Size of the write buffer used by [`Fetcher::save`].
const SAVE_BUFFER: usize = 256 * 1024;

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
}

impl fmt::Debug for Fetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fetcher")
            .field("url", &self.inner.url)
            .field("connections", &self.inner.clients.len())
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
    /// through this fetcher, with [`Error::Cancelled`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidUrl`] if `url` is not an `http` or `https` address, or a
    /// network error if the HTTP client cannot be set up.
    pub async fn new(
        url: &Url,
        options: &DownloadOptions,
        cancel: CancellationToken,
    ) -> Result<Self> {
        check_scheme(url)?;
        // The first use in a process reads the system certificate store, which
        // blocks, so that happens off the async threads.
        let clients = if Clients::shared_ready() {
            Clients::new(options)
        } else {
            let options = options.clone();
            tokio::task::spawn_blocking(move || Clients::new(&options))
                .await
                .map_err(Error::task)?
        };
        let (session, url) = Session::new(url, options, cancel.clone());
        let clients = (0..options.connections.max(1))
            .map(|_| clients.build())
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
            }),
        })
    }

    /// The URL this fetcher was created with, without any `user:password@`
    /// (those became an `Authorization` header for its origin).
    pub fn url(&self) -> &Url {
        &self.inner.url
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
                        _ = self.inner.cancel.cancelled() => return Err(Error::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
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
                _ = self.inner.cancel.cancelled() => return Err(Error::Cancelled.into()),
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
                let n = skip.min(data.len() as u64) as usize;
                data = &data[n..];
                skip -= n as u64;
            }
            if let Some(end) = position.end {
                let wanted = end.saturating_sub(at).min(data.len() as u64) as usize;
                data = &data[..wanted];
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
            if buf.len() as u64 + len > *limit {
                return Err(Error::TooLarge {
                    url: url.clone(),
                    limit: *limit,
                });
            }
            buf.reserve(len as usize);
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
    let mut url = url.clone();
    if !url.username().is_empty() || url.password().is_some() {
        // Only fails for URLs that cannot have credentials; http and https can.
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    Ok(url)
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
    }
}
