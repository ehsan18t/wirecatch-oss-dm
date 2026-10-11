//! A small local HTTP server for stream tests, in the style of the engine's
//! test server: it serves a set of files by path (with byte ranges), can make
//! a path fail a number of times or answer slowly, and logs every request
//! with its headers. Tests never reach the internet.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use wirecatch_engine::Url;

#[derive(Clone, Default)]
struct File {
    body: Bytes,
    content_type: Option<String>,
    /// The next this many requests get `fail_status` instead.
    failures: usize,
    fail_status: u16,
    delay: Duration,
}

/// One request as the server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
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
struct Inner {
    files: Mutex<HashMap<String, File>>,
    log: Mutex<Vec<Seen>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

pub struct Server {
    pub base: String,
    inner: Arc<Inner>,
}

impl Server {
    pub async fn start() -> Self {
        let inner = Arc::new(Inner::default());
        let app = Router::new()
            .route("/{*path}", get(serve))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            inner,
        }
    }

    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}/{}", self.base, path.trim_start_matches('/'))).unwrap()
    }

    pub fn put(&self, path: &str, body: impl Into<Bytes>) {
        self.put_typed(path, body, None);
    }

    pub fn put_typed(&self, path: &str, body: impl Into<Bytes>, content_type: Option<&str>) {
        self.inner.files.lock().unwrap().insert(
            format!("/{}", path.trim_start_matches('/')),
            File {
                body: body.into(),
                content_type: content_type.map(str::to_string),
                ..File::default()
            },
        );
    }

    /// The next `times` requests for `path` are answered with `status`.
    pub fn fail(&self, path: &str, times: usize, status: u16) {
        let mut files = self.inner.files.lock().unwrap();
        let file = files
            .get_mut(&format!("/{}", path.trim_start_matches('/')))
            .expect("put the file first");
        file.failures = times;
        file.fail_status = status;
    }

    /// Every request for `path` waits this long before answering.
    pub fn delay(&self, path: &str, delay: Duration) {
        let mut files = self.inner.files.lock().unwrap();
        files
            .get_mut(&format!("/{}", path.trim_start_matches('/')))
            .expect("put the file first")
            .delay = delay;
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.inner.log.lock().unwrap().clone()
    }

    /// How many requests asked for `path`.
    pub fn requests(&self, path: &str) -> usize {
        let path = format!("/{}", path.trim_start_matches('/'));
        self.seen().iter().filter(|s| s.path == path).count()
    }

    pub fn max_active(&self) -> usize {
        self.inner.max_active.load(Ordering::SeqCst)
    }
}

struct Active(Arc<Inner>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn serve(State(inner): State<Arc<Inner>>, uri: Uri, headers: HeaderMap) -> Response {
    let now = inner.active.fetch_add(1, Ordering::SeqCst) + 1;
    inner.max_active.fetch_max(now, Ordering::SeqCst);
    let _active = Active(inner.clone());
    let path = uri.path().to_string();
    let file = {
        let mut files = inner.files.lock().unwrap();
        files.get_mut(&path).map(|f| {
            let failing = f.failures > 0;
            if failing {
                f.failures -= 1;
            }
            (f.clone(), failing)
        })
    };
    let log = |status: StatusCode| {
        inner.log.lock().unwrap().push(Seen {
            path: path.clone(),
            headers: headers.clone(),
            status: status.as_u16(),
        })
    };
    let Some((file, failing)) = file else {
        log(StatusCode::NOT_FOUND);
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    if !file.delay.is_zero() {
        tokio::time::sleep(file.delay).await;
    }
    if failing {
        let status = StatusCode::from_u16(file.fail_status).unwrap();
        log(status);
        return Response::builder()
            .status(status)
            .body(Body::empty())
            .unwrap();
    }

    let len = file.body.len() as u64;
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(a, b)| {
            let start: u64 = a.parse().ok()?;
            let end = if b.is_empty() {
                len
            } else {
                b.parse::<u64>().ok()?.saturating_add(1).min(len)
            };
            Some((start, end))
        });
    let mut builder = Response::builder().header(header::ACCEPT_RANGES, "bytes");
    if let Some(content_type) = &file.content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let body = match range {
        Some((start, _)) if start >= len => {
            log(StatusCode::RANGE_NOT_SATISFIABLE);
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .unwrap();
        }
        Some((start, end)) => {
            log(StatusCode::PARTIAL_CONTENT);
            builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{len}", end - 1),
            );
            file.body.slice(start as usize..end as usize)
        }
        None => {
            log(StatusCode::OK);
            file.body.clone()
        }
    };
    builder
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .unwrap()
}

/// Deterministic pseudo-random bytes, different for each `seed`, starting
/// with the MPEG-TS sync byte so a joined track is recognized as `.ts`.
pub fn segment(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut data: Vec<u8> = (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    if let Some(first) = data.first_mut() {
        *first = 0x47;
    }
    data
}

/// A fresh, empty folder for one test.
pub fn temp_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-streams-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
