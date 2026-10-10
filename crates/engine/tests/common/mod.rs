//! A small HTTP server for tests: serves a byte buffer with optional range
//! support, per-connection throttling, injected disconnects and custom headers.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
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
    pub content_disposition: Option<String>,
    pub etag: Option<String>,
    /// Send no Content-Length (chunked body of unknown size).
    pub omit_length: bool,
}

#[derive(Default)]
pub struct Stats {
    pub requests: AtomicUsize,
    pub bytes_sent: AtomicU64,
    pub max_concurrent: AtomicUsize,
    current: AtomicUsize,
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

async fn serve(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> Response {
    let config = inner.config.lock().unwrap().clone();
    let len = inner.data.len() as u64;
    let index = inner.stats.requests.fetch_add(1, Ordering::SeqCst);

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

    let (start, end) = match range {
        Some((start, _)) if start >= len => {
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .unwrap();
        }
        Some((start, end)) => {
            builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
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
    let now = stats.current.fetch_add(1, Ordering::SeqCst) + 1;
    stats.max_concurrent.fetch_max(now, Ordering::SeqCst);
    let live = Live(stats.clone());

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
