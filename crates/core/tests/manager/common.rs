//! Shared helpers: a manager running in a temporary folder with a manual
//! clock, and the local servers it downloads from (from the test-support
//! crate).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{NaiveDate, NaiveDateTime};
use tempfile::TempDir;
use wirecatch_core::{
    ComputerProxy, DEFAULT_CAPTURE_TIMEOUT, DEFAULT_STOP_TIMEOUT, KnownFolders, Manager,
    ManagerConfig, ManualClock,
};
use wirecatch_engine::Backoff;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AddRequest, AppSettings, DownloadStatus, DownloadSummary};
pub use wirecatch_test_support::http::{Config, Seen, TestServer};
pub use wirecatch_test_support::silent::SilentServer;
pub use wirecatch_test_support::{PATIENCE, QUIET, TestResult, test_data};

/// A ranged server with an `ETag` for `len` bytes of [`test_data`], sending
/// `bps` bytes per second per connection.
pub async fn throttled_server(len: u64, bps: u64) -> TestResult<TestServer> {
    Ok(TestServer::start(
        test_data(usize::try_from(len)?),
        Config {
            ranges: true,
            per_connection_bps: Some(bps),
            etag: Some("\"v1\"".into()),
            ..Config::default()
        },
    )
    .await)
}

/// A ranged server with an `ETag` for `len` bytes of [`test_data`], at full
/// speed.
pub async fn fast_server(len: u64) -> TestResult<TestServer> {
    Ok(TestServer::start(
        test_data(usize::try_from(len)?),
        Config {
            ranges: true,
            etag: Some("\"v1\"".into()),
            ..Config::default()
        },
    )
    .await)
}

/// Monday, 12 October 2026, at the given local time.
pub fn monday(hour: u32, minute: u32) -> TestResult<NaiveDateTime> {
    NaiveDate::from_ymd_opt(2026, 10, 12)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
        .ok_or_else(|| format!("{hour}:{minute} is not a time of day").into())
}

/// Pauses between failed attempts short enough that a download that must fail
/// fails within a second, and still growing as the engine's own do.
const QUICK_RETRIES: Backoff = Backoff::new(Duration::from_millis(10), Duration::from_millis(100));

/// Changes the manager's configuration, for example to install the media
/// add-on from a local server.
pub type Tweak = Arc<dyn Fn(ManagerConfig) -> ManagerConfig + Send + Sync>;

/// A manager in a temporary folder, with its own home folder and a manual clock.
pub struct Harness {
    /// The manager under test.
    pub manager: Manager,
    /// Its clock, which only moves when the test moves it.
    pub clock: Arc<ManualClock>,
    /// Its known folders, under `root`.
    pub folders: KnownFolders,
    /// The temporary folder that holds its data and its home folder.
    pub root: TempDir,
    timeouts: (Duration, Duration),
    tweak: Option<Tweak>,
}

impl Harness {
    /// A manager at noon on [`monday`].
    pub async fn new() -> TestResult<Self> {
        Self::at(monday(12, 0)?).await
    }

    /// A manager whose clock starts at `now`.
    pub async fn at(now: NaiveDateTime) -> TestResult<Self> {
        Self::build(now, DEFAULT_STOP_TIMEOUT, DEFAULT_CAPTURE_TIMEOUT).await
    }

    /// A manager that gives up on a download that does not stop within `timeout`.
    pub async fn with_stop_timeout(timeout: Duration) -> TestResult<Self> {
        Self::build(monday(12, 0)?, timeout, DEFAULT_CAPTURE_TIMEOUT).await
    }

    /// A manager that waits at most `timeout` for a captured download's server.
    pub async fn with_capture_timeout(timeout: Duration) -> TestResult<Self> {
        Self::build(monday(12, 0)?, Duration::from_millis(300), timeout).await
    }

    /// A manager whose configuration `tweak` changes, on every start.
    pub async fn configured(tweak: Tweak) -> TestResult<Self> {
        Self::build_with(
            monday(12, 0)?,
            DEFAULT_STOP_TIMEOUT,
            DEFAULT_CAPTURE_TIMEOUT,
            Some(tweak),
        )
        .await
    }

    async fn build(
        now: NaiveDateTime,
        stop_timeout: Duration,
        capture_timeout: Duration,
    ) -> TestResult<Self> {
        Self::build_with(now, stop_timeout, capture_timeout, None).await
    }

    async fn build_with(
        now: NaiveDateTime,
        stop_timeout: Duration,
        capture_timeout: Duration,
        tweak: Option<Tweak>,
    ) -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let folders = KnownFolders::under_home(root.path().join("home"));
        let clock = Arc::new(ManualClock::new(now));
        let timeouts = (stop_timeout, capture_timeout);
        let manager = open(&root, &folders, &clock, timeouts, tweak.as_ref()).await?;
        // One connection per download keeps request counts easy to reason about. Browser
        // downloads start without a window first (D23 turns it on by default); the tests of
        // the window turn it back on.
        tune(&manager, |s| {
            s.connections_per_download = 1;
            s.extension.ask_before_download = false;
        })?;
        Ok(Self {
            manager,
            clock,
            folders,
            root,
            timeouts,
            tweak,
        })
    }

    /// The configuration change used on the next [`restart`](Self::restart).
    pub fn tweak_mut(&mut self) -> &mut Option<Tweak> {
        &mut self.tweak
    }

    /// Shuts the manager down and opens a new one on the same data, like an app restart.
    pub async fn restart(&mut self) -> TestResult {
        self.shutdown().await;
        self.reopen().await
    }

    /// The first half of a [`restart`](Self::restart), so a test can look at the files the manager left before [`reopen`](Self::reopen).
    pub async fn shutdown(&self) {
        self.manager.shutdown().await;
    }

    /// Opens a new manager on the same data after [`shutdown`](Self::shutdown).
    pub async fn reopen(&mut self) -> TestResult {
        self.manager = open(
            &self.root,
            &self.folders,
            &self.clock,
            self.timeouts,
            self.tweak.as_ref(),
        )
        .await?;
        Ok(())
    }

    /// Adds a download of `url` that starts at once, and returns its row.
    pub async fn add(&self, url: impl AsRef<str>) -> TestResult<DownloadSummary> {
        self.add_with(url, |_| {}).await
    }

    /// Adds a download of `url` as `change` sets it up, and returns its row.
    pub async fn add_with(
        &self,
        url: impl AsRef<str>,
        change: impl FnOnce(&mut AddRequest),
    ) -> TestResult<DownloadSummary> {
        let mut request = AddRequest {
            url: url.as_ref().into(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        };
        change(&mut request);
        let id = self.manager.add_download(request).await?;
        Ok(self.row(id))
    }

    /// The row of download `id`.
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

    /// Waits until download `id` has `status` and returns its row.
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
) -> TestResult<Manager> {
    let mut config = ManagerConfig::new(root.path().join("data"), folders.clone())
        .with_clock(clock.clone())
        .with_stop_timeout(stop_timeout)
        .with_capture_timeout(capture_timeout)
        .with_retry_backoff(QUICK_RETRIES)
        // The machine's proxy variables and system setting never reach a test.
        .with_computer_proxy(ComputerProxy::Given(None));
    // Tests never reach the internet: no update check unless a test asks.
    config.addon_sources.ytdlp_releases = None;
    if let Some(tweak) = tweak {
        config = tweak(config);
    }
    Ok(Manager::open(config).await?)
}

/// The row of download `id`; the test fails if it is gone.
pub fn row(manager: &Manager, id: DownloadId) -> DownloadSummary {
    manager
        .list_downloads()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("download {id:?} is gone"))
}

/// Changes the manager's settings as `change` says.
pub fn tune(manager: &Manager, change: impl FnOnce(&mut AppSettings)) -> TestResult {
    let mut settings = manager.get_settings();
    change(&mut settings);
    manager.update_settings(settings)?;
    Ok(())
}

/// Whether the download of `row` is connecting or downloading.
pub fn is_running(row: &DownloadSummary) -> bool {
    matches!(
        row.status,
        DownloadStatus::Connecting | DownloadStatus::Downloading
    )
}

/// The extension of file name `name`, compared as the file system wrote it.
pub fn extension(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|ext| ext.to_str())
}

/// Whether `path` exists.
pub fn exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

/// The partial file and the state file the engine keeps beside `path` while
/// it downloads.
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
