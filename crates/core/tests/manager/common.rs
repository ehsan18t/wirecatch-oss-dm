//! Shared helpers: a local HTTP server (adapted from the engine's tests) and a
//! manager running in a temporary folder with a manual clock.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use chrono::{NaiveDate, NaiveDateTime};
use tempfile::TempDir;
use wirecatch_core::{
    DEFAULT_CAPTURE_TIMEOUT, DEFAULT_STOP_TIMEOUT, KnownFolders, Manager, ManagerConfig,
    ManualClock,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AddRequest, AppSettings, DownloadStatus, DownloadSummary};

/// How long any wait in a test may take before it fails.
pub const PATIENCE: Duration = Duration::from_secs(20);

#[derive(Clone, Default)]
pub struct Config {
    pub ranges: bool,
    /// Bytes per second for each connection.
    pub per_connection_bps: Option<u64>,
    pub etag: Option<String>,
    /// Paths answered with this status instead of the file.
    pub refuse: Vec<(String, u16)>,
    pub content_disposition: Option<String>,
    pub content_type: Option<String>,
    /// How long to wait before answering at all.
    pub answer_delay: Option<Duration>,
}

#[derive(Default)]
pub struct Stats {
    pub requests: AtomicUsize,
    pub bytes_sent: AtomicU64,
    pub max_concurrent: AtomicUsize,
    current: AtomicUsize,
    /// `Cookie` header of every request.
    pub cookies: Mutex<Vec<String>>,
    /// `Authorization` header of every request.
    pub authorizations: Mutex<Vec<String>>,
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

    /// A ranged server with an ETag, sending `bps` bytes per second per connection.
    pub async fn throttled(len: usize, bps: u64) -> Self {
        Self::start(
            test_data(len),
            Config {
                ranges: true,
                per_connection_bps: Some(bps),
                etag: Some("\"v1\"".into()),
                ..Config::default()
            },
        )
        .await
    }

    /// A ranged server at full speed.
    pub async fn fast(len: usize) -> Self {
        Self::start(
            test_data(len),
            Config {
                ranges: true,
                etag: Some("\"v1\"".into()),
                ..Config::default()
            },
        )
        .await
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path)
    }

    pub fn configure(&self, change: impl FnOnce(&mut Config)) {
        change(&mut self.inner.config.lock().unwrap());
    }

    pub fn bytes_sent(&self) -> u64 {
        self.stats.bytes_sent.load(Ordering::SeqCst)
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

async fn serve(State(inner): State<Arc<Inner>>, uri: Uri, headers: HeaderMap) -> Response {
    let config = inner.config.lock().unwrap().clone();
    let len = inner.data.len() as u64;
    inner.stats.requests.fetch_add(1, Ordering::SeqCst);
    if let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        inner.stats.cookies.lock().unwrap().push(cookie.to_string());
    }
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        inner
            .stats
            .authorizations
            .lock()
            .unwrap()
            .push(auth.to_string());
    }
    if let Some((_, status)) = config.refuse.iter().find(|(path, _)| uri.path() == path) {
        return Response::builder()
            .status(*status)
            .body(Body::empty())
            .unwrap();
    }

    let range = if config.ranges {
        parse_range(&headers, len)
    } else {
        None
    };
    if let Some(delay) = config.answer_delay {
        tokio::time::sleep(delay).await;
    }
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
    builder = builder.header(header::CONTENT_LENGTH, end - start);

    let stats = inner.stats.clone();
    let now = stats.current.fetch_add(1, Ordering::SeqCst) + 1;
    stats.max_concurrent.fetch_max(now, Ordering::SeqCst);
    let live = Live(stats.clone());

    let chunk = config
        .per_connection_bps
        .map(|bps| (bps / 20).max(1024) as usize)
        .unwrap_or(64 * 1024);
    let pause = config
        .per_connection_bps
        .map(|bps| Duration::from_secs_f64(chunk as f64 / bps as f64));
    let data = inner.data.clone();

    let body = futures_util::stream::unfold((start, live), move |(pos, live)| {
        let data = data.clone();
        let stats = stats.clone();
        async move {
            if pos >= end {
                return None;
            }
            if let Some(pause) = pause {
                tokio::time::sleep(pause).await;
            }
            let next = (pos + chunk as u64).min(end);
            let bytes = data.slice(pos as usize..next as usize);
            stats
                .bytes_sent
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            Some((Ok::<_, std::io::Error>(bytes), (next, live)))
        }
    });

    builder.body(Body::from_stream(body)).unwrap()
}

/// Monday, 12 October 2026, at the given local time.
pub fn monday(hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 12)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
        .unwrap()
}

/// Changes the manager's configuration, for example to install the media
/// add-on from a local server.
pub type Tweak = Arc<dyn Fn(ManagerConfig) -> ManagerConfig + Send + Sync>;

/// A manager in a temporary folder, with its own home folder and a manual clock.
pub struct Harness {
    pub manager: Manager,
    pub clock: Arc<ManualClock>,
    pub folders: KnownFolders,
    pub root: TempDir,
    timeouts: (Duration, Duration),
    tweak: Option<Tweak>,
}

impl Harness {
    pub async fn new() -> Self {
        Self::at(monday(12, 0)).await
    }

    pub async fn at(now: NaiveDateTime) -> Self {
        Self::build(now, DEFAULT_STOP_TIMEOUT, DEFAULT_CAPTURE_TIMEOUT).await
    }

    /// A manager that gives up on a download that does not stop within `timeout`.
    pub async fn with_stop_timeout(timeout: Duration) -> Self {
        Self::build(monday(12, 0), timeout, DEFAULT_CAPTURE_TIMEOUT).await
    }

    /// A manager that waits at most `timeout` for a captured download's server.
    pub async fn with_capture_timeout(timeout: Duration) -> Self {
        Self::build(monday(12, 0), Duration::from_millis(300), timeout).await
    }

    /// A manager whose configuration `tweak` changes, on every start.
    pub async fn configured(tweak: Tweak) -> Self {
        Self::build_with(
            monday(12, 0),
            DEFAULT_STOP_TIMEOUT,
            DEFAULT_CAPTURE_TIMEOUT,
            Some(tweak),
        )
        .await
    }

    async fn build(now: NaiveDateTime, stop_timeout: Duration, capture_timeout: Duration) -> Self {
        Self::build_with(now, stop_timeout, capture_timeout, None).await
    }

    async fn build_with(
        now: NaiveDateTime,
        stop_timeout: Duration,
        capture_timeout: Duration,
        tweak: Option<Tweak>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let folders = KnownFolders::under_home(root.path().join("home"));
        let clock = Arc::new(ManualClock::new(now));
        let timeouts = (stop_timeout, capture_timeout);
        let manager = open(&root, &folders, &clock, timeouts, tweak.as_ref()).await;
        // One connection per download keeps request counts easy to reason about. Browser
        // downloads start without a window first (D23 turns it on by default); the tests of
        // the window turn it back on.
        tune(&manager, |s| {
            s.connections_per_download = 1;
            s.extension.ask_before_download = false;
        });
        Self {
            manager,
            clock,
            folders,
            root,
            timeouts,
            tweak,
        }
    }

    /// The configuration change used on the next [`restart`](Self::restart).
    pub fn tweak_mut(&mut self) -> &mut Option<Tweak> {
        &mut self.tweak
    }

    /// Shuts the manager down and opens a new one on the same data, like an app restart.
    pub async fn restart(&mut self) {
        self.manager.shutdown().await;
        self.manager = open(
            &self.root,
            &self.folders,
            &self.clock,
            self.timeouts,
            self.tweak.as_ref(),
        )
        .await;
    }

    pub async fn add(&self, url: &str) -> DownloadSummary {
        self.add_with(url, |_| {}).await
    }

    pub async fn add_with(
        &self,
        url: &str,
        change: impl FnOnce(&mut AddRequest),
    ) -> DownloadSummary {
        let mut request = AddRequest {
            url: url.to_string(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        };
        change(&mut request);
        let id = self.manager.add_download(request).await.unwrap();
        self.row(id)
    }

    pub fn row(&self, id: DownloadId) -> DownloadSummary {
        row(&self.manager, id)
    }

    /// Waits until download `id` satisfies `check` and returns its row.
    pub async fn wait_for(
        &self,
        id: DownloadId,
        what: &str,
        check: impl Fn(&DownloadSummary) -> bool,
    ) -> DownloadSummary {
        wait_until(what, || Some(self.row(id)).filter(|r| check(r))).await
    }

    pub async fn wait_status(&self, id: DownloadId, status: DownloadStatus) -> DownloadSummary {
        self.wait_for(id, &format!("{status:?}"), |r| r.status == status)
            .await
    }
}

async fn open(
    root: &TempDir,
    folders: &KnownFolders,
    clock: &Arc<ManualClock>,
    (stop_timeout, capture_timeout): (Duration, Duration),
    tweak: Option<&Tweak>,
) -> Manager {
    let mut config = ManagerConfig::new(root.path().join("data"), folders.clone())
        .with_clock(clock.clone())
        .with_stop_timeout(stop_timeout)
        .with_capture_timeout(capture_timeout);
    // Tests never reach the internet: no update check unless a test asks.
    config.addon_sources.ytdlp_releases = None;
    if let Some(tweak) = tweak {
        config = tweak(config);
    }
    Manager::open(config).await.unwrap()
}

/// A server that accepts connections and never answers.
pub struct SilentServer {
    pub base: String,
    pub accepted: Arc<AtomicUsize>,
}

impl SilentServer {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                held.push(socket);
            }
        });
        Self {
            base: format!("http://{addr}"),
            accepted,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path)
    }
}

pub fn row(manager: &Manager, id: DownloadId) -> DownloadSummary {
    manager
        .list_downloads()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("download {id:?} is gone"))
}

pub fn tune(manager: &Manager, change: impl FnOnce(&mut AppSettings)) {
    let mut settings = manager.get_settings();
    change(&mut settings);
    manager.update_settings(settings).unwrap();
}

pub fn is_running(row: &DownloadSummary) -> bool {
    matches!(
        row.status,
        DownloadStatus::Connecting | DownloadStatus::Downloading
    )
}

pub fn exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

pub fn partial_files(path: &str) -> [PathBuf; 2] {
    [
        PathBuf::from(format!("{path}{}", wirecatch_engine::PART_EXT)),
        PathBuf::from(format!("{path}{}", wirecatch_engine::STATE_EXT)),
    ]
}

/// Polls `check` until it returns a value. Waiting on a condition rather than a
/// fixed time keeps tests fast and free of timing guesses.
pub async fn wait_until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
