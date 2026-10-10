//! Manager tests that need runs to misbehave in ways a real server cannot cause
//! reliably: runs that ignore their stop request, or a server answer held back
//! until the test lets it through.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use wirecatch_protocol::app::AddRequest;
use wirecatch_protocol::extension::{CaptureSource, NewDownload};

use super::Manager;
use super::fake::{EngineFuture, FakeEngine, FakeRun};
use crate::error::CoreError;
use crate::{DEFAULT_CAPTURE_TIMEOUT, DEFAULT_QUEUE_ID, KnownFolders, ManagerConfig};

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

fn add_request(url: &str) -> AddRequest {
    AddRequest {
        url: url.into(),
        queue: None,
        folder: None,
        file_name: None,
        start: true,
    }
}

/// While a removal waits for a run that ignores its stop request, nothing may
/// bring the download back or start it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_being_removed_cannot_be_brought_back() {
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = runs.clone();
    let engine = move |run: FakeRun| {
        counted.fetch_add(1, Ordering::SeqCst);
        deaf(run)
    };
    let stop_timeout = Duration::from_secs(1);
    let (manager, dir) = manager_with(engine, stop_timeout, DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/gone.bin"))
        .await
        .unwrap();
    assert!(manager.lock().downloads[&id].is_running());

    let removing = manager.clone();
    let removal = tokio::spawn(async move { removing.remove(&[id], false).await });
    wait_until("the removal to begin", || {
        manager.list_downloads().is_empty()
    })
    .await;
    assert!(
        manager.lock().downloads[&id].is_running(),
        "the run stopped too early to test anything"
    );

    let gone = |result: crate::Result<()>| matches!(result, Err(CoreError::DownloadNotFound(_)));
    assert!(gone(manager.resume(id)));
    assert!(gone(manager.retry(id)));
    assert!(gone(manager.pause(id)));
    assert!(gone(manager.cancel(id)));
    assert!(gone(
        manager.refresh_link(id, "https://example.com/again.bin")
    ));
    assert!(gone(manager.move_to_queue(&[id], DEFAULT_QUEUE_ID)));
    let elsewhere = dir.path().join("elsewhere").to_string_lossy().into_owned();
    assert!(gone(manager.set_download_folder(id, &elsewhere).await));
    manager.resume_all().unwrap();
    assert!(manager.list_downloads().is_empty());

    removal.await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(manager.lock().downloads.is_empty());
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "a removed download ran again"
    );
}
