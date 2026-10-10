//! A small HTTP server for tests: serves a byte buffer with optional range
//! support, per-connection throttling, injected disconnects, custom headers,
//! redirects, busy answers with `Retry-After` and a connection limit. Every
//! request is logged with its headers so tests can check what was sent.

#![allow(dead_code)]

use std::ops::Range;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;

#[derive(Clone, Default)]
pub struct Config {
    pub ranges: bool,
    /// Bytes per second for each connection.
    pub per_connection_bps: Option<u64>,
    /// The first `n` responses break off after `after` bytes.
    pub drop_first: Option<(usize, u64)>,
    /// The first response (a range request) sends at most this many bytes, with
    /// a matching `Content-Range`, as servers may. It ends cleanly, so the client
    /// must ask again for the rest, with no reset that could race the headers.
    pub short_first: Option<u64>,
    pub content_disposition: Option<String>,
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
    pub content_type: Option<String>,
}

/// Answers a range of requests with a busy status instead of the file.
#[derive(Clone)]
pub struct Busy {
    pub requests: Range<usize>,
    pub status: u16,
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
    pub at: Instant,
    pub wall: SystemTime,
    pub path: String,
    pub headers: HeaderMap,
    pub status: u16,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

#[derive(Default)]
pub struct Stats {
    pub requests: AtomicUsize,
    pub bytes_sent: AtomicU64,
    pub max_concurrent: AtomicUsize,
    /// Requests turned away by `max_connections`.
    pub refused: AtomicUsize,
    pub log: Mutex<Vec<Seen>>,
    current: AtomicUsize,
}

impl Stats {
    pub fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }
}

struct Inner {
    data: Bytes,
    config: Mutex<Config>,
    stats: Arc<Stats>,
}

pub struct TestServer {
    pub base: String,
    pub data: Bytes,
    pub stats: Arc<Stats>,
    inner: Arc<Inner>,
}

impl TestServer {
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
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            data,
            stats,
            inner,
        }
    }

    pub fn url(&self, path: &str) -> wirecatch_engine::Url {
        wirecatch_engine::Url::parse(&format!("{}/{}", self.base, path)).unwrap()
    }

    pub fn configure(&self, change: impl FnOnce(&mut Config)) {
        change(&mut self.inner.config.lock().unwrap());
    }
}

/// Deterministic pseudo-random bytes, so a misplaced range is always detected.
pub fn test_data(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn parse_range(headers: &HeaderMap, len: u64) -> Option<(u64, u64)> {
    let value = headers.get(header::RANGE)?.to_str().ok()?;
    let spec = value.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.parse().ok()?;
    let end = if b.is_empty() {
        len
    } else {
        b.parse::<u64>().ok()?.saturating_add(1).min(len)
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

fn log(inner: &Inner, uri: &Uri, headers: &HeaderMap, status: StatusCode) {
    inner.stats.log.lock().unwrap().push(Seen {
        at: Instant::now(),
        wall: SystemTime::now(),
        path: uri.path().to_string(),
        headers: headers.clone(),
        status: status.as_u16(),
    });
}

async fn serve(State(inner): State<Arc<Inner>>, uri: Uri, headers: HeaderMap) -> Response {
    let config = inner.config.lock().unwrap().clone();
    let len = inner.data.len() as u64;
    let index = inner.stats.requests.fetch_add(1, Ordering::SeqCst);

    if let Some((path, location)) = &config.redirect
        && uri.path() == path
    {
        log(&inner, &uri, &headers, StatusCode::FOUND);
        return Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, location)
            .body(Body::empty())
            .unwrap();
    }
    if let Some(hang) = &config.hang
        && hang.contains(&index)
    {
        log(&inner, &uri, &headers, StatusCode::REQUEST_TIMEOUT);
        std::future::pending::<()>().await;
    }
    if let Some(busy) = &config.busy
        && busy.requests.contains(&index)
    {
        let status = StatusCode::from_u16(busy.status).unwrap();
        log(&inner, &uri, &headers, status);
        let mut builder = Response::builder().status(status);
        if let Some(retry_after) = &busy.retry_after {
            builder = builder.header(header::RETRY_AFTER, retry_after);
        }
        return builder.body(Body::empty()).unwrap();
    }

    let range = if config.ranges {
        parse_range(&headers, len)
    } else {
        None
    };
    let mut builder = Response::builder();
    if config.ranges {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(etag) = &config.etag {
        builder = builder.header(header::ETAG, etag);
    }
    if let Some(cd) = &config.content_disposition {
        builder = builder.header(header::CONTENT_DISPOSITION, cd);
    }
    if let Some(content_type) = &config.content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }

    let mut status = StatusCode::OK;
    let (start, end) = match range {
        Some((start, _)) if start >= len => {
            log(&inner, &uri, &headers, StatusCode::RANGE_NOT_SATISFIABLE);
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .unwrap();
        }
        Some((start, end)) => {
            let end = match config.short_first {
                Some(cap) if index == 0 => end.min(start + cap),
                _ => end,
            };
            status = StatusCode::PARTIAL_CONTENT;
            builder = builder.status(status).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{len}", end - 1),
            );
            (start, end)
        }
        None => (0, len),
    };
    if !config.omit_length {
        builder = builder.header(header::CONTENT_LENGTH, end - start);
    }

    let stats = inner.stats.clone();
    let limit = config.max_connections.map(|(n, _)| n).unwrap_or(usize::MAX);
    let now = match admit(&stats.current, limit) {
        Some(now) => now,
        None => {
            stats.refused.fetch_add(1, Ordering::SeqCst);
            let refuse = config.max_connections.map(|(_, r)| r);
            if let Some(Refuse::Status(code)) = refuse {
                let busy = StatusCode::from_u16(code).unwrap();
                log(&inner, &uri, &headers, busy);
                let mut refusal = Response::builder().status(busy);
                if let Some(retry_after) = &config.refuse_retry_after {
                    refusal = refusal.header(header::RETRY_AFTER, retry_after);
                }
                return refusal.body(Body::empty()).unwrap();
            }
            // Headers go out, then the body fails at once: hyper resets the connection.
            log(&inner, &uri, &headers, status);
            let reset = futures_util::stream::once(async {
                Err::<Bytes, _>(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "connection limit reached",
                ))
            });
            return builder.body(Body::from_stream(reset)).unwrap();
        }
    };
    stats.max_concurrent.fetch_max(now, Ordering::SeqCst);
    let live = Live(stats.clone());
    log(&inner, &uri, &headers, status);

    let abort_after = config
        .drop_first
        .and_then(|(n, after)| (index < n).then_some(after));
    let chunk = config
        .per_connection_bps
        .map(|bps| (bps / 20).max(1024) as usize)
        .unwrap_or(64 * 1024);
    let pause = config
        .per_connection_bps
        .map(|bps| Duration::from_secs_f64(chunk as f64 / bps as f64));
    let data = inner.data.clone();

    let body = futures_util::stream::unfold((start, 0u64, live), move |(pos, sent, live)| {
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
            let bytes = data.slice(pos as usize..next as usize);
            stats
                .bytes_sent
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Some((
                Ok::<_, std::io::Error>(bytes),
                (next, sent + (next - pos), live),
            ))
        }
    });

    builder.body(Body::from_stream(body)).unwrap()
}
