//! A local HTTP server for the tests, after the one in
//! `crates/engine/tests/common`: it serves one byte buffer at every path, with
//! range support and an ETag, and logs every request with its headers. A held
//! server takes requests but answers none until [`TestServer::release`].

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use tokio::sync::watch;

use crate::PATIENCE;

/// One request as the server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    /// The request path, for example `/file.bin`.
    pub path: String,
    /// Every request header.
    pub headers: HeaderMap,
}

impl Seen {
    /// The value of header `name`, if present and readable.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

struct Inner {
    data: Bytes,
    /// `true` once answers may go out.
    open: watch::Sender<bool>,
    seen: watch::Sender<Vec<Seen>>,
}

/// The HTTP server, on `127.0.0.1` and a free port.
pub struct TestServer {
    base: String,
    inner: Arc<Inner>,
}

impl TestServer {
    /// A server that answers at once.
    pub async fn start(data: Vec<u8>) -> Self {
        Self::with_gate(data, true).await
    }

    /// A server that takes requests but holds every answer until
    /// [`release`](Self::release).
    pub async fn held(data: Vec<u8>) -> Self {
        Self::with_gate(data, false).await
    }

    async fn with_gate(data: Vec<u8>, open: bool) -> Self {
        let inner = Arc::new(Inner {
            data: Bytes::from(data),
            open: watch::Sender::new(open),
            seen: watch::Sender::new(Vec::new()),
        });
        let app = Router::new()
            .route("/{*path}", get(serve))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("could not bind the test server");
        let addr = listener.local_addr().expect("the test server has no address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base: format!("http://{addr}"),
            inner,
        }
    }

    /// The full URL of `path` (without a leading slash) on this server.
    pub fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base)
    }

    /// The bytes served at every path.
    pub fn data(&self) -> &[u8] {
        &self.inner.data
    }

    /// Lets held answers, and every later one, go out.
    pub fn release(&self) {
        self.inner.open.send_replace(true);
    }

    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<Seen> {
        self.inner.seen.borrow().clone()
    }

    /// The requests for `path` (with its leading slash) so far.
    pub fn requests_for(&self, path: &str) -> Vec<Seen> {
        self.requests()
            .into_iter()
            .filter(|s| s.path == path)
            .collect()
    }

    /// Waits for the first request for `path` (with its leading slash).
    pub async fn wait_for_request(&self, path: &str) -> Seen {
        let mut seen = self.inner.seen.subscribe();
        let found = tokio::time::timeout(PATIENCE, seen.wait_for(|v| v.iter().any(|s| s.path == path)))
            .await
            .unwrap_or_else(|_| panic!("the server never saw a request for {path}"))
            .expect("the test server stopped");
        found
            .iter()
            .find(|s| s.path == path)
            .cloned()
            .expect("the request was just seen")
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

async fn serve(State(inner): State<Arc<Inner>>, uri: Uri, headers: HeaderMap) -> Response {
    inner.seen.send_modify(|seen| {
        seen.push(Seen {
            path: uri.path().to_string(),
            headers: headers.clone(),
        });
    });
    let mut open = inner.open.subscribe();
    if open.wait_for(|open| *open).await.is_err() {
        return status(StatusCode::SERVICE_UNAVAILABLE);
    }

    let len = inner.data.len() as u64;
    let builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, "\"e2e\"")
        .header(header::CONTENT_TYPE, "application/octet-stream");
    let (builder, start, end) = match parse_range(&headers, len) {
        Some((start, _)) if start >= len => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .expect("a valid response");
        }
        Some((start, end)) => (
            builder.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{len}", end - 1),
            ),
            start,
            end,
        ),
        None => (builder.status(StatusCode::OK), 0, len),
    };
    let body = inner.data.slice(start as usize..end as usize);
    builder
        .header(header::CONTENT_LENGTH, end - start)
        .body(Body::from(body))
        .expect("a valid response")
}

fn status(code: StatusCode) -> Response {
    Response::builder()
        .status(code)
        .body(Body::empty())
        .expect("a valid response")
}
