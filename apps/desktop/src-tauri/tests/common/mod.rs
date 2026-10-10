//! Shared test helpers: a local HTTP server (the pattern of `crates/engine/tests/common`, cut
//! down to what these tests need) and a manager in a temporary folder.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use tempfile::TempDir;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};
use wirecatch_desktop_lib::api;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AddRequest, DownloadStatus, DownloadSummary};

/// How long any wait may take before the test fails.
pub const PATIENCE: Duration = Duration::from_secs(30);

struct Served {
    data: Bytes,
    /// Bytes per second for each connection; `None` is full speed.
    bps: Option<u64>,
}

/// Serves one file under any path, with range support and an optional per-connection speed.
pub struct TestServer {
    base: String,
    pub data: Bytes,
}

impl TestServer {
    pub async fn start(len: usize, bps: Option<u64>) -> Self {
        let data = Bytes::from(test_data(len));
        let state = Arc::new(Served {
            data: data.clone(),
            bps,
        });
        let app = Router::new()
            .route("/{*path}", get(serve))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            data,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base)
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

fn range(headers: &HeaderMap, len: u64) -> Option<(u64, u64)> {
    let spec = headers
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start = a.parse().ok()?;
    let end = if b.is_empty() {
        len
    } else {
        b.parse::<u64>().ok()?.saturating_add(1).min(len)
    };
    Some((start, end))
}

async fn serve(State(served): State<Arc<Served>>, headers: HeaderMap) -> Response {
    let len = served.data.len() as u64;
    let mut builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, "\"v1\"");
    let (start, end) = match range(&headers, len) {
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
    builder = builder.header(header::CONTENT_LENGTH, end - start);
    let chunk = served
        .bps
        .map(|bps| (bps / 20).max(1024))
        .unwrap_or(64 * 1024);
    let pause = served
        .bps
        .map(|bps| Duration::from_secs_f64(chunk as f64 / bps as f64));
    let data = served.data.clone();
    let body = futures_util::stream::unfold(start, move |pos| {
        let data = data.clone();
        async move {
            if pos >= end {
                return None;
            }
            if let Some(pause) = pause {
                tokio::time::sleep(pause).await;
            }
            let next = (pos + chunk).min(end);
            Some((
                Ok::<_, std::io::Error>(data.slice(pos as usize..next as usize)),
                next,
            ))
        }
    });
    builder.body(Body::from_stream(body)).unwrap()
}

/// A temporary home folder and data folder for a manager.
pub struct Sandbox {
    pub dir: TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn config(&self) -> ManagerConfig {
        let home = self.dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        ManagerConfig::new(
            self.dir.path().join("data"),
            KnownFolders::under_home(&home),
        )
    }

    /// The folder test downloads are saved to.
    pub fn downloads(&self) -> PathBuf {
        let dir = self.dir.path().join("saved");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub async fn open(&self) -> Manager {
        Manager::open(self.config()).await.unwrap()
    }
}

pub fn request(url: String, folder: &Path, start: bool) -> AddRequest {
    AddRequest {
        url,
        queue: None,
        folder: Some(folder.to_string_lossy().into_owned()),
        file_name: None,
        start,
    }
}

pub fn row(m: &Manager, id: DownloadId) -> Option<DownloadSummary> {
    api::list_downloads(m).into_iter().find(|r| r.id == id)
}

/// Waits until the download's row satisfies `ok`, and returns it.
pub async fn wait_for(
    m: &Manager,
    id: DownloadId,
    what: &str,
    ok: impl Fn(&DownloadSummary) -> bool,
) -> DownloadSummary {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(r) = row(m, id)
            && ok(&r)
        {
            return r;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "download {} never became {what}; last seen: {:?}",
            id.0,
            row(m, id).map(|r| (r.status, r.done, r.error))
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub async fn wait_status(m: &Manager, id: DownloadId, status: DownloadStatus) -> DownloadSummary {
    wait_for(m, id, &format!("{status:?}"), |r| r.status == status).await
}

/// Running means bytes are moving, or about to.
pub fn running(r: &DownloadSummary) -> bool {
    matches!(
        r.status,
        DownloadStatus::Connecting | DownloadStatus::Downloading
    )
}
