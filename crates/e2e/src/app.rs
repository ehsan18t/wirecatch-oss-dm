//! The app side: a private folder and endpoint per test, and a real manager
//! serving the protocol on that endpoint the way the desktop app does.

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{broadcast::error::RecvError, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};
use wirecatch_server::serve_connection;
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_protocol::app::DownloadSummary;
use wirecatch_protocol::extension::NewDownload;
use wirecatch_test_support::{PrivateDir, private_dir};

use crate::PATIENCE;

/// How long the test app waits for a captured download's server before giving
/// up: longer than any test waits for an answer, so a capture a test holds
/// never ends on its own, however slow the machine.
pub const CAPTURE_TIMEOUT: Duration = PATIENCE.saturating_mul(2);

/// How long the test app lets a download take to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// How long deleting the sandbox keeps trying while Windows still has a file in
/// it open, and how long it waits between tries.
const DELETE_PATIENCE: Duration = Duration::from_secs(2);
const DELETE_RETRY: Duration = Duration::from_millis(20);

/// A private folder for one test: the app's data folder, a home folder whose
/// `Downloads` receives the files, and a private endpoint nobody else uses.
///
/// Clones share the folder. [`run`] deletes it once the test is over.
#[derive(Clone)]
pub struct Sandbox {
    root: Arc<PrivateDir>,
    /// Where the test app listens and the bridge connects.
    pub endpoint: Endpoint,
}

impl Sandbox {
    /// A new sandbox with a fresh endpoint.
    #[must_use]
    pub fn new() -> Self {
        let root = private_dir("e2e").expect("could not make the sandbox folder");
        Self {
            endpoint: Endpoint::from_raw(root.endpoint_name()),
            root: Arc::new(root),
        }
    }

    /// The sandbox folder.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// The app's data folder (database).
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.path().join("data")
    }

    /// The user's standard folders, all under the sandbox.
    #[must_use]
    pub fn folders(&self) -> KnownFolders {
        KnownFolders::under_home(self.path().join("home"))
    }

    /// Deletes the sandbox folder. The manager's database thread closes its file
    /// a moment after the last manager handle is gone, and Windows does not
    /// delete open files, so this tries again for a while instead of leaving the
    /// folder behind.
    async fn delete(&self) {
        let deadline = Instant::now() + DELETE_PATIENCE;
        loop {
            match std::fs::remove_dir_all(self.path()) {
                Ok(()) => return,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(e) if Instant::now() >= deadline => {
                    eprintln!("could not delete {}: {e}", self.path().display());
                    return;
                }
                Err(_) => tokio::time::sleep(DELETE_RETRY).await,
            }
        }
    }

    /// Every partial download file (`.wcpart`) under the home folder.
    #[must_use]
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

/// Runs one end-to-end test on a runtime of its own, with a new sandbox, and
/// deletes the sandbox afterwards, whether the test passed or not.
///
/// The sandbox is deleted only after the test's runtime has stopped: until then
/// the manager's tasks may still hold its database open, and Windows does not
/// delete open files. The deletion runs on a small runtime of its own, so it can
/// wait between tries without blocking a thread.
pub fn run<F, Fut>(test: F)
where
    F: FnOnce(Sandbox) -> Fut,
    Fut: Future<Output = ()>,
{
    let sandbox = Sandbox::new();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("could not start a runtime");
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(test(sandbox.clone()));
    }));
    drop(runtime);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("could not start a runtime")
        .block_on(sandbox.delete());
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
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
    ///
    /// # Errors
    ///
    /// The manager cannot open its data folder or save the test's settings, or
    /// `endpoint` cannot be listened on (another app already serves it, say).
    pub async fn start_at(
        endpoint: &Endpoint,
        data_dir: &Path,
        folders: KnownFolders,
    ) -> anyhow::Result<Self> {
        let config = ManagerConfig::new(data_dir, folders)
            .with_capture_timeout(CAPTURE_TIMEOUT)
            .with_stop_timeout(STOP_TIMEOUT);
        let manager = Manager::open(config).await?;
        // These tests drive captures that start at once; a fresh install shows a window first
        // (D23), so they switch that off. The test of the window switches it back on.
        let mut settings = manager.get_settings();
        if settings.extension.ask_before_download {
            settings.extension.ask_before_download = false;
            manager.update_settings(settings)?;
        }
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
    #[must_use]
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Counts connections that have ended.
    #[must_use]
    pub fn ended(&self) -> watch::Receiver<usize> {
        self.ended.clone()
    }

    /// Quits the app the way a process exit does: stops listening, drops every
    /// connection at once, then shuts the manager down.
    pub async fn quit(self) {
        self.acceptor.abort();
        // Wait for the task to be dropped, which closes the listener and every
        // connection it was serving. Cancelled is the expected end; a panic in the
        // accept loop is the test's failure.
        if let Err(ended) = self.acceptor.await
            && ended.is_panic()
        {
            std::panic::resume_unwind(ended.into_panic());
        }
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
#[must_use]
pub fn capture(url: &str, handover_id: &str) -> NewDownload {
    NewDownload {
        handover_id: handover_id.to_string(),
        url: url.to_string(),
        referrer: Some("http://127.0.0.1/page".into()),
        page_url: Some("http://127.0.0.1/page".into()),
        frame_url: None,
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
