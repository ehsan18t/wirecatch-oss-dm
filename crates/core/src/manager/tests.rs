//! Manager tests that need runs to misbehave in ways a real server cannot cause
//! reliably: runs that ignore their stop request, or a server answer held back
//! until the test lets it through.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wirecatch_protocol::extension::{CaptureSource, NewDownload};

use super::Manager;
use super::fake::{EngineFuture, FakeEngine, FakeRun};
use crate::error::CoreError;
use crate::{KnownFolders, ManagerConfig};

/// How long any wait in these tests may take before it fails.
const PATIENCE: Duration = Duration::from_secs(20);

/// A manager in a temporary folder whose runs are played by `engine`.
async fn manager_with(
    engine: impl Fn(FakeRun) -> EngineFuture + Send + Sync + 'static,
    stop_timeout: Duration,
    capture_timeout: Duration,
) -> (Manager, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let folders = KnownFolders::under_home(dir.path().join("home"));
    let config = ManagerConfig::new(dir.path().join("data"), folders)
        .with_stop_timeout(stop_timeout)
        .with_capture_timeout(capture_timeout);
    let manager = Manager::open(config).await.unwrap();
    let engine: FakeEngine = Arc::new(engine);
    manager.use_fake_engine(engine);
    (manager, dir)
}

/// A run that never hears from its server and ignores being asked to stop, so
/// only the stop timeout ends it.
fn deaf(_run: FakeRun) -> EngineFuture {
    Box::pin(std::future::pending())
}

fn capture(url: &str) -> NewDownload {
    NewDownload {
        handover_id: format!("handover-{url}"),
        url: url.into(),
        referrer: None,
        page_url: None,
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: None,
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

/// Polls `check` until it holds.
async fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The extension waits about ten seconds for an answer, so a failed capture must
/// be answered before its attempt has finished stopping, not after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_capture_is_answered_before_its_attempt_is_cleaned_up() {
    let stop_timeout = Duration::from_secs(3);
    let (manager, _dir) = manager_with(deaf, stop_timeout, Duration::from_millis(200)).await;
    let asked = Instant::now();
    let err = manager
        .add_capture(capture("https://example.com/report.pdf"))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::NotStarted(_)), "{err}");
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "answered only after {:?}",
        asked.elapsed()
    );
    assert!(
        manager.list_downloads().is_empty(),
        "the attempt is still shown"
    );
    // The attempt is cleaned up in the background once its run gives up.
    wait_until("the attempt to be removed", || {
        manager.lock().downloads.is_empty()
    })
    .await;
}
