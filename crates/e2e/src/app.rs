//! The app side: a private folder and endpoint per test, and a real manager
//! serving the protocol on that endpoint the way the desktop app does.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::{broadcast::error::RecvError, watch};
use tokio::task::{JoinHandle, JoinSet};
use wirecatch_core::{KnownFolders, Manager, ManagerConfig, serve_connection};
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_protocol::app::DownloadSummary;
use wirecatch_protocol::extension::NewDownload;

use crate::PATIENCE;

/// How long the test app waits for a captured download's server before giving
/// up. Shorter than the default so a test that hits it does not take long, and
/// longer than any wait a passing test needs.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);

/// How long the test app lets a download take to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// A temporary folder for one test: the app's data folder, a home folder whose
/// `Downloads` receives the files, and a private endpoint nobody else uses.
pub struct Sandbox {
    root: TempDir,
    /// Where the test app listens and the bridge connects.
    pub endpoint: Endpoint,
}

impl Sandbox {
    /// A new sandbox with a fresh endpoint.
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = tempfile::Builder::new()
            .prefix("wirecatch-e2e-")
            .tempdir()
            .expect("could not make a temporary folder");
        let tag = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let endpoint = private_endpoint(root.path(), &tag);
        Self { root, endpoint }
    }

    /// The sandbox folder.
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// The app's data folder (database).
    pub fn data_dir(&self) -> PathBuf {
        self.path().join("data")
    }

    /// The user's standard folders, all under the sandbox.
    pub fn folders(&self) -> KnownFolders {
        KnownFolders::under_home(self.path().join("home"))
    }

    /// Every partial download file (`.wcpart`) under the home folder.
    pub fn part_files(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut dirs = vec![self.path().join("home")];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.to_string_lossy().ends_with(".wcpart") {
                    found.push(path);
                }
            }
        }
        found
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
fn private_endpoint(_root: &Path, tag: &str) -> Endpoint {
    Endpoint::from_raw(format!(r"\\.\pipe\wirecatch-e2e-{tag}"))
}

#[cfg(unix)]
fn private_endpoint(root: &Path, _tag: &str) -> Endpoint {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    // The listener refuses a folder anyone else can enter.
    let dir = root.join("ipc");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .expect("could not make the endpoint folder");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .expect("could not make the endpoint folder private");
    Endpoint::from_raw(dir.join("app.sock"))
}

/// The app: a manager serving every connection on an endpoint, each in its own
/// task, the way the desktop app hosts `wirecatch-core` (D13).
pub struct App {
    /// The manager, for checking and changing things the way the app's UI would.
    pub manager: Manager,
    accepted: Arc<AtomicUsize>,
    ended: watch::Receiver<usize>,
    acceptor: JoinHandle<()>,
}

impl App {
    /// Starts the app on the sandbox's endpoint.
    pub async fn start(sandbox: &Sandbox) -> Self {
        Self::start_at(&sandbox.endpoint, &sandbox.data_dir(), sandbox.folders())
            .await
            .expect("could not start the test app")
    }

    /// Opens a manager on `data_dir` and serves it on `endpoint`.
    pub async fn start_at(
        endpoint: &Endpoint,
        data_dir: &Path,
        folders: KnownFolders,
    ) -> anyhow::Result<Self> {
        let config = ManagerConfig::new(data_dir, folders)
            .with_capture_timeout(CAPTURE_TIMEOUT)
            .with_stop_timeout(STOP_TIMEOUT);
        let manager = Manager::open(config).await?;
        let listener = Listener::bind(endpoint)?;
        let accepted = Arc::new(AtomicUsize::new(0));
        let (ended_tx, ended) = watch::channel(0);
        let acceptor = tokio::spawn(accept_loop(
            listener,
            manager.clone(),
            accepted.clone(),
            ended_tx,
        ));
        Ok(Self {
            manager,
            accepted,
            ended,
            acceptor,
        })
    }

    /// How many connections the app has accepted.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Counts connections that have ended.
    pub fn ended(&self) -> watch::Receiver<usize> {
        self.ended.clone()
    }

    /// Quits the app the way a process exit does: stops listening, drops every
    /// connection at once, then shuts the manager down.
    pub async fn quit(self) {
        self.acceptor.abort();
        // Wait for the task to be dropped, which closes the listener and every
        // connection it was serving.
        let _ = self.acceptor.await;
        self.manager.shutdown().await;
    }
}

async fn accept_loop(
    mut listener: Listener,
    manager: Manager,
    accepted: Arc<AtomicUsize>,
    ended: watch::Sender<usize>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            conn = listener.accept() => match conn {
                Ok(conn) => {
                    accepted.fetch_add(1, Ordering::SeqCst);
                    connections.spawn(serve_connection(manager.clone(), conn));
                }
                Err(e) => eprintln!("test app: a connection attempt failed: {e}"),
            },
            Some(_) = connections.join_next() => ended.send_modify(|n| *n += 1),
        }
    }
}

/// A capture as the extension sends it for `url`, with a cookie and the given
/// handover id.
pub fn capture(url: &str, handover_id: &str) -> NewDownload {
    NewDownload {
        handover_id: handover_id.to_string(),
        url: url.to_string(),
        referrer: Some("http://127.0.0.1/page".into()),
        page_url: Some("http://127.0.0.1/page".into()),
        suggested_name: None,
        mime: Some("application/octet-stream".into()),
        size_hint: None,
        cookies: Some("session=e2e-cookie".into()),
        user_agent: Some("WirecatchE2E/1.0".into()),
        extra_headers: Vec::new(),
        source: wirecatch_protocol::extension::CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

/// Waits until a download satisfies `check` and returns its row. Reacts to the
/// manager's events instead of polling.
pub async fn wait_for_row(
    manager: &Manager,
    what: &str,
    check: impl Fn(&DownloadSummary) -> bool,
) -> DownloadSummary {
    let mut events = manager.subscribe();
    let wait = async {
        loop {
            if let Some(row) = manager.list_downloads().into_iter().find(|r| check(r)) {
                return row;
            }
            match events.recv().await {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => panic!("the manager closed while waiting for {what}"),
            }
        }
    };
    tokio::time::timeout(PATIENCE, wait)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for {what}; downloads: {:#?}",
                manager.list_downloads()
            )
        })
}
