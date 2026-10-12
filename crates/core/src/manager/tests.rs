//! Manager tests that need runs to misbehave in ways a real server cannot cause
//! reliably: runs that ignore their stop request, or a server answer held back
//! until the test lets it through.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use wirecatch_engine::{Error as EngineError, Url};
use wirecatch_protocol::app::{AddRequest, DownloadStatus};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};

use super::Manager;
use super::fake::{EngineFuture, FakeEngine, FakeRun};
use super::settle::ServerAnswer;
use crate::error::CoreError;
use crate::naming;
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
    // These captures start at once, without the window a fresh install shows first (D23).
    let mut settings = manager.get_settings();
    settings.extension.ask_before_download = false;
    manager.update_settings(settings).unwrap();
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
    assert_eq!(
        manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );

    removal.await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(manager.lock().downloads.is_empty());
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "a removed download ran again"
    );
}

/// The server's answer for a file called `name`.
fn answer(name: &str) -> ServerAnswer {
    ServerAnswer {
        suggested_name: name.into(),
        total: Some(10),
        content_type: None,
        final_url: Url::parse("https://example.com/get?id=1").unwrap(),
    }
}

/// A run whose server answers only when `gate` lets it, with a file named
/// `name`. Like the engine inside its resolver, it does not watch for a stop
/// request while waiting; once the path is chosen it writes a part file there
/// and then waits to be stopped.
fn answers_on_cue(gate: Arc<Notify>, name: &'static str) -> impl Fn(FakeRun) -> EngineFuture {
    move |run: FakeRun| {
        let gate = gate.clone();
        Box::pin(async move {
            gate.notified().await;
            let settle = run.settle.expect("the path is not settled yet");
            let path = settle(&answer(name)).map_err(EngineError::TargetRejected)?;
            let [part, _] = naming::partial_files(&path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(part, b"part").unwrap();
            run.cancel.cancelled().await;
            Err(EngineError::Cancelled)
        })
    }
}

/// Every part file under `dir`.
fn part_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(part_files(&path));
        } else if path.to_string_lossy().ends_with(wirecatch_engine::PART_EXT) {
            found.push(path);
        }
    }
    found
}

/// A server answer that arrives while the download is being moved to another
/// folder must not settle a path: the move would then leave the new part file
/// behind in the old place, with nothing pointing at it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_answer_during_a_move_settles_nothing() {
    let gate = Arc::new(Notify::new());
    let engine = answers_on_cue(gate.clone(), "report.pdf");
    let (manager, dir) =
        manager_with(engine, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/get?id=1"))
        .await
        .unwrap();

    let chosen = dir.path().join("chosen");
    let moving = manager.clone();
    let target = chosen.to_string_lossy().into_owned();
    let moved = tokio::spawn(async move { moving.set_download_folder(id, &target).await });
    wait_until("the move to ask the run to stop", || {
        manager.lock().downloads[&id]
            .run
            .as_ref()
            .is_some_and(|run| run.stop.is_some())
    })
    .await;
    // The server answers now, while the run is being stopped for the move.
    gate.notify_one();
    moved.await.unwrap().unwrap();

    let row = manager.lock().downloads[&id].row.clone();
    assert!(
        Path::new(&row.path).starts_with(&chosen),
        "moved to {}",
        row.path
    );
    assert_eq!(
        part_files(dir.path()),
        Vec::<PathBuf>::new(),
        "a part file was left behind"
    );
}

/// Checking whether a name is free can take long (a network folder that does not
/// answer); it must not hold the lock every command needs meanwhile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_are_checked_on_disk_without_holding_the_lock() {
    let (manager, _dir) = manager_with(deaf, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/get?id=1"))
        .await
        .unwrap();
    let run_id = manager.lock().downloads[&id].run.as_ref().unwrap().id;

    let (checks, under_lock) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let free_on_disk = |path: &Path| {
        checks.fetch_add(1, Ordering::SeqCst);
        if manager.inner.state.try_lock().is_err() {
            under_lock.fetch_add(1, Ordering::SeqCst);
        }
        // Someone else's file already has the server's name.
        !path.ends_with("report.pdf")
    };
    let path = manager
        .settle_path_checking(id, run_id, &answer("report.pdf"), &free_on_disk)
        .unwrap();
    assert!(path.ends_with("report (1).pdf"), "{}", path.display());
    assert_eq!(checks.load(Ordering::SeqCst), 2);
    assert_eq!(
        under_lock.load(Ordering::SeqCst),
        0,
        "the disk was checked with the lock held"
    );
    let state = manager.lock();
    assert!(state.claimed.contains(&path));
    assert_eq!(Path::new(&state.downloads[&id].row.path), path);
    state.assert_consistent();
}

/// A path held for a moment does not upset the consistency check, but a claim
/// that belongs to no unfinished download does.
#[tokio::test]
async fn only_reservations_may_exceed_the_downloads_paths() {
    let (manager, dir) = manager_with(
        |_| Box::pin(std::future::pending()),
        Duration::from_secs(1),
        DEFAULT_CAPTURE_TIMEOUT,
    )
    .await;
    let mut state = manager.lock();
    let path = dir.path().join("moment.bin");
    state.claimed.reserve(&path);
    state.assert_consistent();
    state.claimed.release(&path);
    state.assert_consistent();
    state.claimed.insert(&path);
    let leaked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.assert_consistent();
    }));
    assert!(leaked.is_err(), "a claim of no download went unnoticed");
}

/// A run that waits until it is asked to stop.
fn obedient(run: FakeRun) -> EngineFuture {
    Box::pin(async move {
        run.cancel.cancelled().await;
        Err(EngineError::Cancelled)
    })
}

/// A move that fails halfway and cannot be undone leaves the download's files in
/// two folders: it must not start again on its own (it would resume from a
/// mismatched part and state), it says what happened, and the name in the new
/// folder stays taken while files of it are there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_that_cannot_be_undone_stops_the_download() {
    let (manager, dir) =
        manager_with(obedient, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/big.iso"))
        .await
        .unwrap();
    let old_path = PathBuf::from(&manager.lock().downloads[&id].row.path);
    std::fs::create_dir_all(old_path.parent().unwrap()).unwrap();
    for file in naming::partial_files(&old_path) {
        std::fs::write(file, b"partial").unwrap();
    }
    // The first file moves; the second, and putting the first back, fail.
    let moves = Arc::new(AtomicUsize::new(0));
    let counted = moves.clone();
    manager.use_fake_mover(Arc::new(move |from, to| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            std::fs::rename(from, to)
        } else {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }));

    let chosen = dir.path().join("chosen");
    let error = manager
        .set_download_folder(id, &chosen.to_string_lossy())
        .await
        .unwrap_err();
    assert!(matches!(error, CoreError::FilesSplit { .. }), "{error}");

    let state = manager.lock();
    let entry = &state.downloads[&id];
    assert!(entry.run.is_none(), "the download started again");
    assert_eq!(entry.row.status, DownloadStatus::Failed);
    let shown = entry.row.error.as_ref().map(|e| e.message.clone());
    assert_eq!(shown, Some(error.to_string()));
    let stranded = chosen.join(old_path.file_name().unwrap());
    assert!(
        state.claimed.contains(&stranded),
        "the name in the new folder was given up"
    );
}
