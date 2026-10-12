//! A small HTTP server for tests: serves a byte buffer with optional range
//! support, per-connection throttling, injected disconnects, custom headers,
//! redirects, busy answers with `Retry-After`, a connection limit and a limit on
//! how much one request may ask for. Every request is logged with its headers
//! and the connection it came on, so tests can check what was sent.

use std::net::SocketAddr;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, Uri, header, response};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use url::Url;

/// How much of the body goes out at once when the speed is not limited.
const UNTHROTTLED_CHUNK: usize = 64 * 1024;
/// The smallest piece of body a throttled connection sends at once.
const MIN_THROTTLED_CHUNK: u64 = 1024;
/// A throttled connection sends its speed in this many pieces a second.
const THROTTLE_STEPS_PER_SECOND: u64 = 20;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// How the server behaves. The default serves the whole file with a `200` to
/// every request and ignores `Range`.
#[derive(Clone, Default)]
pub struct Config {
    /// Answers range requests with `206` and advertises `Accept-Ranges`.
    pub ranges: bool,
    /// Bytes per second for each connection.
    pub per_connection_bps: Option<u64>,
    /// The first `n` responses break off after `after` bytes.
    pub drop_first: Option<(usize, u64)>,
    /// The first response (a range request) sends at most this many bytes, with
    /// a matching `Content-Range`, as servers may. It ends cleanly, so the client
    /// must ask again for the rest, with no reset that could race the headers.
    pub short_first: Option<u64>,
    /// The `Content-Disposition` header to send.
    pub content_disposition: Option<String>,
    /// The `ETag` header to send.
    pub etag: Option<String>,
    /// Send no Content-Length (chunked body of unknown size).
    pub omit_length: bool,
    /// Requests for this path get a `302` to this location.
    pub redirect: Option<(String, String)>,
    /// Requests with these sequence numbers (0 is the first) are turned away.
    pub busy: Option<Busy>,
    /// At most this many bodies are served at once; requests beyond it are refused.
    pub max_connections: Option<(usize, Refuse)>,
    /// `Retry-After` sent with a `Refuse::Status` refusal.
    pub refuse_retry_after: Option<String>,
    /// Requests with these sequence numbers are accepted but never answered.
    pub hang: Option<Range<usize>>,
    /// The `Content-Type` header to send.
    pub content_type: Option<String>,
    /// Requests that ask for more than this many bytes, ask for a range with
    /// no end, or ask for no range at all are refused with `403`, the way some
    /// video servers turn away (or slow down) large requests.
    pub max_range: Option<u64>,
    /// Range answers give no size (`Content-Range: bytes 0-99/*`).
    pub unknown_total: bool,
}

/// Answers a range of requests with a busy status instead of the file.
#[derive(Clone)]
pub struct Busy {
    /// Sequence numbers of the requests to turn away (0 is the first).
    pub requests: Range<usize>,
    /// The status to answer with, such as `503`.
    pub status: u16,
    /// The `Retry-After` header to send with it.
    pub retry_after: Option<String>,
}

/// How a request over the connection limit is turned away.
#[derive(Clone, Copy, Debug)]
pub enum Refuse {
    /// Answer with this status (and no body).
    Status(u16),
    /// Send the `206` headers, then reset the connection before any body byte.
    Reset,
}

/// One request as the server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    /// When it arrived, for measuring the time between requests.
    pub at: Instant,
    /// When it arrived, by the wall clock, for comparing with dates in headers.
    pub wall: SystemTime,
    /// The path asked for.
    pub path: String,
    /// The request headers.
    pub headers: HeaderMap,
    /// The status the server answered with.
    pub status: u16,
    /// The client's end of the TCP connection the request came on: requests
    /// with the same one shared a connection.
    pub peer: SocketAddr,
}

impl Seen {
    /// The value of header `name`, if it was sent and is text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The range asked for in the `Range` header, `[start, end)`; `end` is
    /// `None` for an open range such as `bytes=100-`.
    #[must_use]
    pub fn range(&self) -> Option<(u64, Option<u64>)> {
        let (first, last) = self
            .header("range")?
            .strip_prefix("bytes=")?
            .split_once('-')?;
        let start = first.parse().ok()?;
        let end = if last.is_empty() {
            None
        } else {
            Some(last.parse::<u64>().ok()?.checked_add(1)?)
        };
        Some((start, end))
    }
}

/// What the server has done so far.
#[derive(Default)]
pub struct Stats {
    /// Requests received.
    pub requests: AtomicUsize,
    /// Body bytes sent.
    pub bytes_sent: AtomicU64,
    /// Most bodies served at the same time.
    pub max_concurrent: AtomicUsize,
    /// Requests turned away by `max_connections`.
    pub refused: AtomicUsize,
    /// Every request, in the order they arrived.
    pub log: Mutex<Vec<Seen>>,
    current: AtomicUsize,
}

impl Stats {
    /// A copy of the request log.
    #[must_use]
    pub fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }
}

struct Inner {
    data: Bytes,
    config: Mutex<Config>,
    stats: Arc<Stats>,
}

/// A running server; it stops with the test's runtime.
pub struct TestServer {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    /// The bytes every path serves.
    pub data: Bytes,
    /// What the server has done so far.
    pub stats: Arc<Stats>,
    inner: Arc<Inner>,
}

impl TestServer {
    /// Serves `data` at every path of a free port on 127.0.0.1.
    pub async fn start(data: Vec<u8>, config: Config) -> Self {
        let data = Bytes::from(data);
        let stats = Arc::new(Stats::default());
        let inner = Arc::new(Inner {
            data: data.clone(),
            config: Mutex::new(config),
            stats: stats.clone(),
        });
        let app = Router::new()
            .route("/{*path}", get(serve))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            data,
            stats,
            inner,
        }
    }

    /// The URL of `path` on this server.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}/{}", self.base, path)).unwrap()
    }

    /// Changes the configuration for the requests that follow.
    pub fn configure(&self, change: impl FnOnce(&mut Config)) {
        change(&mut self.inner.config.lock().unwrap());
    }
}

/// One request being answered.
struct Request<'a> {
    inner: &'a Inner,
    config: Config,
    /// Its sequence number; 0 is the first.
    index: usize,
    peer: SocketAddr,
    path: String,
    headers: HeaderMap,
}

impl Request<'_> {
    fn log(&self, status: StatusCode) {
        self.inner.stats.log.lock().unwrap().push(Seen {
            at: Instant::now(),
            wall: SystemTime::now(),
            path: self.path.clone(),
            headers: self.headers.clone(),
            status: status.as_u16(),
            peer: self.peer,
        });
    }

    fn len(&self) -> u64 {
        self.inner.data.len() as u64
    }
}

async fn serve(
    State(inner): State<Arc<Inner>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let request = Request {
        config: inner.config.lock().unwrap().clone(),
        index: inner.stats.requests.fetch_add(1, Ordering::SeqCst),
        inner: &inner,
        peer,
        path: uri.path().to_string(),
        headers,
    };
    match turn_away(&request).await {
        Some(response) => response,
        None => serve_file(&request),
    }
}

/// The answer for a request the configuration turns away before the file:
/// too large, redirected, hanging or busy.
async fn turn_away(request: &Request<'_>) -> Option<Response> {
    let config = &request.config;
    if let Some(max) = config.max_range
        && asks_too_much(&request.headers, max)
    {
        request.log(StatusCode::FORBIDDEN);
        return Some(empty(Response::builder().status(StatusCode::FORBIDDEN)));
    }
    if let Some((path, location)) = &config.redirect
        && request.path == *path
    {
        request.log(StatusCode::FOUND);
        let builder = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, location);
        return Some(empty(builder));
    }
    if let Some(hang) = &config.hang
        && hang.contains(&request.index)
    {
        request.log(StatusCode::REQUEST_TIMEOUT);
        std::future::pending::<()>().await;
    }
    if let Some(busy) = &config.busy
        && busy.requests.contains(&request.index)
    {
        let status = StatusCode::from_u16(busy.status).unwrap();
        request.log(status);
        let mut builder = Response::builder().status(status);
        if let Some(retry_after) = &busy.retry_after {
            builder = builder.header(header::RETRY_AFTER, retry_after);
        }
        return Some(empty(builder));
    }
    None
}

/// The file, or the part of it the request asked for.
fn serve_file(request: &Request<'_>) -> Response {
    let config = &request.config;
    let len = request.len();
    let mut builder = file_headers(config);
    let mut status = StatusCode::OK;
    let range = if config.ranges {
        parse_range(&request.headers, len)
    } else {
        None
    };
    let (start, end) = match range {
        Some((start, _)) if start >= len => {
            request.log(StatusCode::RANGE_NOT_SATISFIABLE);
            let builder = builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"));
            return empty(builder);
        }
        Some((start, end)) => {
            let end = match config.short_first {
                Some(cap) if request.index == 0 => end.min(start + cap),
                _ => end,
            };
            status = StatusCode::PARTIAL_CONTENT;
            let total = if config.unknown_total {
                "*".to_string()
            } else {
                len.to_string()
            };
            builder = builder.status(status).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{total}", end - 1),
            );
            (start, end)
        }
        None => (0, len),
    };
    if !config.omit_length {
        builder = builder.header(header::CONTENT_LENGTH, end - start);
    }

    let counters = request.inner.stats.clone();
    let limit = config.max_connections.map_or(usize::MAX, |(n, _)| n);
    let Some(now) = admit(&counters.current, limit) else {
        return refuse(request, builder, status);
    };
    counters.max_concurrent.fetch_max(now, Ordering::SeqCst);
    let live = Live(counters);
    request.log(status);
    builder
        .body(Body::from_stream(body(request, start..end, live)))
        .unwrap()
}

/// The headers every answer with the file carries.
fn file_headers(config: &Config) -> response::Builder {
    let mut builder = Response::builder();
    if config.ranges {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(etag) = &config.etag {
        builder = builder.header(header::ETAG, etag);
    }
    if let Some(disposition) = &config.content_disposition {
        builder = builder.header(header::CONTENT_DISPOSITION, disposition);
    }
    if let Some(content_type) = &config.content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    builder
}

/// Turns away a request over the connection limit, the way `max_connections` says.
fn refuse(request: &Request<'_>, builder: response::Builder, status: StatusCode) -> Response {
    let config = &request.config;
    request.inner.stats.refused.fetch_add(1, Ordering::SeqCst);
    if let Some((_, Refuse::Status(code))) = config.max_connections {
        let busy = StatusCode::from_u16(code).unwrap();
        request.log(busy);
        let mut refusal = Response::builder().status(busy);
        if let Some(retry_after) = &config.refuse_retry_after {
            refusal = refusal.header(header::RETRY_AFTER, retry_after);
        }
        return empty(refusal);
    }
    // Headers go out, then the body fails at once: hyper resets the connection.
    request.log(status);
    let reset = futures_util::stream::once(async {
        Err::<Bytes, _>(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection limit reached",
        ))
    });
    builder.body(Body::from_stream(reset)).unwrap()
}

/// The body for bytes `range` of the file, throttled and broken off as configured.
fn body(
    request: &Request<'_>,
    range: Range<u64>,
    live: Live,
) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + use<> {
    let config = &request.config;
    let abort_after = config
        .drop_first
        .and_then(|(n, after)| (request.index < n).then_some(after));
    let chunk = config.per_connection_bps.map_or(UNTHROTTLED_CHUNK, |bps| {
        usize::try_from((bps / THROTTLE_STEPS_PER_SECOND).max(MIN_THROTTLED_CHUNK)).unwrap()
    });
    let pause = config
        .per_connection_bps
        .map(|bps| time_to_send(chunk as u64, bps));
    let data = request.inner.data.clone();
    let stats = request.inner.stats.clone();
    let end = range.end;
    futures_util::stream::unfold((range.start, 0u64, live), move |(pos, sent, live)| {
        let data = data.clone();
        let stats = stats.clone();
        async move {
            if pos >= end {
                return None;
            }
            if let Some(after) = abort_after
                && sent >= after
            {
                let err =
                    std::io::Error::new(std::io::ErrorKind::ConnectionReset, "injected disconnect");
                return Some((Err(err), (end, sent, live)));
            }
            if let Some(pause) = pause {
                tokio::time::sleep(pause).await;
            }
            let next = (pos + chunk as u64).min(end);
            let bytes = data.slice(index(pos)..index(next));
            stats
                .bytes_sent
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Some((Ok(bytes), (next, sent + (next - pos), live)))
        }
    })
}

/// How long `bytes` take at `bps` bytes per second.
pub(crate) fn time_to_send(bytes: u64, bps: u64) -> Duration {
    let nanos = u128::from(bytes) * NANOS_PER_SECOND / u128::from(bps.max(1));
    Duration::from_nanos(u64::try_from(nanos).unwrap())
}

/// A position in the served buffer as an index into it.
pub(crate) fn index(position: u64) -> usize {
    usize::try_from(position).unwrap()
}

fn empty(builder: response::Builder) -> Response {
    builder.body(Body::empty()).unwrap()
}

/// `bytes=a-b` or `bytes=a-` as a half-open range within `len`.
pub(crate) fn parse_range(headers: &HeaderMap, len: u64) -> Option<(u64, u64)> {
    let value = headers.get(header::RANGE)?.to_str().ok()?;
    let spec = value.strip_prefix("bytes=")?;
    let (first, last) = spec.split_once('-')?;
    let start: u64 = first.parse().ok()?;
    let end = if last.is_empty() {
        len
    } else {
        last.parse::<u64>().ok()?.saturating_add(1).min(len)
    };
    Some((start, end))
}

/// Decrements the concurrency gauge when the response body is dropped.
struct Live(Arc<Stats>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.current.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Counts one more body being served unless `limit` are already being served.
/// Returns the new count.
fn admit(current: &AtomicUsize, limit: usize) -> Option<usize> {
    let mut now = current.load(Ordering::SeqCst);
    loop {
        if now >= limit {
            return None;
        }
        match current.compare_exchange(now, now + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(now + 1),
            Err(actual) => now = actual,
        }
    }
}

/// Whether a request asks for more than `max` bytes, for a range with no end,
/// or for no range at all.
fn asks_too_much(headers: &HeaderMap, max: u64) -> bool {
    let asked = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(first, last)| Some((first.parse::<u64>().ok()?, last.parse::<u64>().ok()?)));
    match asked {
        Some((start, last)) => last < start || last - start + 1 > max,
        None => true,
    }
}
