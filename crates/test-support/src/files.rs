//! A local HTTP server for a set of files, each at its own path: what a test
//! of a manifest, a playlist or a page with several parts downloads from.
//!
//! Each file is served with byte ranges and an optional `Content-Type`. A
//! file can be made to fail a set number of times or to answer slowly, and
//! every request is logged with its headers, so a test can check what was
//! asked for, how often and how many requests ran at once.
//!
//! ```no_run
//! # async fn example() {
//! use std::time::Duration;
//! use wirecatch_test_support::files::FileServer;
//!
//! let server = FileServer::start().await;
//! server.put_typed("live/index.m3u8", "#EXTM3U\n", "application/vnd.apple.mpegurl");
//! server.put("live/seg0.ts", vec![0x47; 188]);
//! server.fail("live/seg0.ts", 2, 503);
//! server.delay("live/seg0.ts", Duration::from_millis(50));
//! let playlist = server.url("live/index.m3u8");
//! // ... download `playlist`, then:
//! assert_eq!(server.requests("live/seg0.ts"), 3);
//! # }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use url::Url;

use crate::http::{Seen, index, parse_range};

/// One file and how it is served.
#[derive(Clone, Default)]
struct File {
    body: Bytes,
    content_type: Option<String>,
    /// The next this many requests get `fail_status` instead of the file.
    failures: usize,
    fail_status: u16,
    /// How long every request waits before it is answered.
    delay: Duration,
}

#[derive(Default)]
struct Inner {
    /// The files by path, each starting with `/`.
    files: Mutex<HashMap<String, File>>,
    log: Mutex<Vec<Seen>>,
    /// Requests being answered now.
    active: AtomicUsize,
    /// The most requests answered at the same time.
    max_active: AtomicUsize,
}

/// A running server for a set of files; it stops with the test's runtime.
/// A path with no file is answered with `404`.
pub struct FileServer {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    inner: Arc<Inner>,
}

impl FileServer {
    /// Starts a server with no files on a free port of 127.0.0.1.
    pub async fn start() -> Self {
        let inner = Arc::new(Inner::default());
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
            inner,
        }
    }

    /// The URL of `path` on this server; a leading `/` is optional.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}{}", self.base, key(path))).unwrap()
    }

    /// Serves `body` at `path` with no `Content-Type`, replacing any file
    /// there (and how it was set to fail or wait).
    pub fn put(&self, path: &str, body: impl Into<Bytes>) {
        self.insert(path, body.into(), None);
    }

    /// Serves `body` at `path` with `content_type`, replacing any file there.
    pub fn put_typed(&self, path: &str, body: impl Into<Bytes>, content_type: &str) {
        self.insert(path, body.into(), Some(content_type.to_string()));
    }

    /// The next `times` requests for the file at `path` are answered with
    /// `status` instead.
    ///
    /// # Panics
    ///
    /// If no file was put at `path`, or `status` is not a valid status code.
    pub fn fail(&self, path: &str, times: usize, status: u16) {
        assert!(
            StatusCode::from_u16(status).is_ok(),
            "{status} is not a status code"
        );
        self.change(path, |file| {
            file.failures = times;
            file.fail_status = status;
        });
    }

    /// Every request for the file at `path` waits `delay` before it is
    /// answered, failures included.
    ///
    /// # Panics
    ///
    /// If no file was put at `path`.
    pub fn delay(&self, path: &str, delay: Duration) {
        self.change(path, |file| file.delay = delay);
    }

    /// Every request so far, in the order they arrived.
    #[must_use]
    pub fn seen(&self) -> Vec<Seen> {
        self.inner.log.lock().unwrap().clone()
    }

    /// How many requests asked for `path`.
    #[must_use]
    pub fn requests(&self, path: &str) -> usize {
        let path = key(path);
        self.inner
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| seen.path == path)
            .count()
    }

    /// The most requests that were being answered at the same time.
    #[must_use]
    pub fn max_active(&self) -> usize {
        self.inner.max_active.load(Ordering::SeqCst)
    }

    fn insert(&self, path: &str, body: Bytes, content_type: Option<String>) {
        let file = File {
            body,
            content_type,
            ..File::default()
        };
        self.inner.files.lock().unwrap().insert(key(path), file);
    }

    fn change(&self, path: &str, change: impl FnOnce(&mut File)) {
        let mut files = self.inner.files.lock().unwrap();
        let Some(file) = files.get_mut(&key(path)) else {
            panic!("put a file at {path} before changing how it is served");
        };
        change(file);
    }
}

/// `path` as the server sees it: with exactly one leading `/`.
fn key(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

/// Counts a request as being answered until it is dropped.
struct Active(Arc<Inner>);

impl Active {
    fn begin(inner: &Arc<Inner>) -> Self {
        let now = inner.active.fetch_add(1, Ordering::SeqCst) + 1;
        inner.max_active.fetch_max(now, Ordering::SeqCst);
        Self(inner.clone())
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn serve(
    State(inner): State<Arc<Inner>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let _active = Active::begin(&inner);
    let path = uri.path().to_string();
    // Take the file and use up one failure under the same lock, so parallel
    // requests never share one.
    let file = {
        let mut files = inner.files.lock().unwrap();
        files.get_mut(&path).map(|file| {
            let failing = file.failures > 0;
            if failing {
                file.failures -= 1;
            }
            (file.clone(), failing)
        })
    };
    let (status, response) = match file {
        None => (
            StatusCode::NOT_FOUND,
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty()),
        ),
        Some((file, failing)) => {
            if !file.delay.is_zero() {
                tokio::time::sleep(file.delay).await;
            }
            if failing {
                let status = StatusCode::from_u16(file.fail_status).unwrap();
                (
                    status,
                    Response::builder().status(status).body(Body::empty()),
                )
            } else {
                answer(&file, &headers)
            }
        }
    };
    inner.log.lock().unwrap().push(Seen {
        at: Instant::now(),
        wall: SystemTime::now(),
        path,
        headers,
        status: status.as_u16(),
        peer,
    });
    response.unwrap()
}

/// The file, or the part of it the `Range` header asks for.
fn answer(file: &File, headers: &HeaderMap) -> (StatusCode, axum::http::Result<Response>) {
    let len = file.body.len() as u64;
    let mut builder = Response::builder().header(header::ACCEPT_RANGES, "bytes");
    if let Some(content_type) = &file.content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let (status, body) = match parse_range(headers, len) {
        Some((start, _)) if start >= len => {
            let response = builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty());
            return (StatusCode::RANGE_NOT_SATISFIABLE, response);
        }
        Some((start, end)) => {
            builder = builder.header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{len}", end - 1),
            );
            let part = file.body.slice(index(start)..index(end));
            (StatusCode::PARTIAL_CONTENT, part)
        }
        None => (StatusCode::OK, file.body.clone()),
    };
    let response = builder
        .status(status)
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body));
    (status, response)
}
