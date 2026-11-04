//! Manager tests that need runs to misbehave in ways a real server cannot cause
//! reliably: runs that ignore their stop request, or a server answer held back
//! until the test lets it through.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use wirecatch_engine::{Error as EngineError, PathRejected, Url};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AddRequest, CaptureConfirmation, DownloadStatus};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};
use wirecatch_test_support::{PATIENCE, QUIET};

use super::fake::{EngineFuture, FakeEngine, FakeRun};
use super::settle::ServerAnswer;
use super::{LinkFrom, Manager, RemoveFiles};
use crate::error::CoreError;
use crate::naming;
use crate::{DEFAULT_CAPTURE_TIMEOUT, DEFAULT_QUEUE_ID, KnownFolders, ManagerConfig};
use wirecatch_engine::{PrivateNetwork, ProxyChoice};

/// A media download without a browser's agent names the app, never the engine
/// library's default; with one, it is the browser's.
#[test]
fn media_requests_name_the_app_or_the_browser() {
    let mut origin = crate::headers::Origin::default();
    let agent =
        super::media::media_options(&origin, PrivateNetwork::Block, ProxyChoice::System).user_agent;
    assert_eq!(
        agent,
        crate::headers::download_options(PrivateNetwork::Block, ProxyChoice::System).user_agent
    );
    assert_ne!(
        agent,
        wirecatch_engine::DownloadOptions::default().user_agent
    );
    origin.user_agent = Some("TestBrowser/1".into());
    assert_eq!(
        super::media::media_options(&origin, PrivateNetwork::Block, ProxyChoice::System).user_agent,
        "TestBrowser/1"
    );
}

/// A manager in a temporary folder whose runs are played by `engine`.
async fn manager_with(
    engine: impl Fn(FakeRun) -> EngineFuture + Send + Sync + 'static,
    stop_timeout: Duration,
    capture_timeout: Duration,
) -> (Manager, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let folders = KnownFolders::under_home(dir.path().join("home"));
    let config = ManagerConfig::for_tests(dir.path().join("data"), folders)
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
        // A page on the person's network (D39), so no name is looked up first.
        page_url: Some("http://intranet.test/".into()),
        frame_url: None,
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: None,
        cookies: None,
        cookie_jar: None,
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
    // The run ignores being asked to stop until the test lets it go, and the stop
    // timeout outlasts the test: the attempt stops only when the test says.
    let let_go = Arc::new(Notify::new());
    let held = {
        let let_go = let_go.clone();
        move |_run: FakeRun| -> EngineFuture {
            let let_go = let_go.clone();
            Box::pin(async move {
                let_go.notified().await;
                Err(EngineError::Cancelled)
            })
        }
    };
    let (manager, _dir) = manager_with(held, 2 * PATIENCE, Duration::from_millis(200)).await;
    let err = tokio::time::timeout(
        PATIENCE,
        manager.add_capture(capture("https://example.com/report.pdf")),
    )
    .await
    .expect("answered while the attempt was still stopping")
    .unwrap_err();
    assert!(matches!(err, CoreError::NotStarted(_)), "{err}");
    assert!(
        !manager.lock().downloads.is_empty(),
        "the attempt stopped before the answer"
    );
    assert!(
        manager.list_downloads().is_empty(),
        "the attempt is still shown"
    );
    // The attempt is cleaned up in the background once its run gives up.
    let_go.notify_one();
    wait_until("the attempt to be removed", || {
        manager.lock().downloads.is_empty()
    })
    .await;
}

/// The capture timeout counts from when the capture arrived, not from when the
/// wait for its server begins: whatever the check of the link took first is
/// already spent. Here all of it was, so the answer comes without waiting; a
/// wait counted from its own start would last twice `PATIENCE`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_captures_answer_comes_within_the_timeout_of_its_arrival() {
    let capture_timeout = 2 * PATIENCE;
    let (manager, _dir) = manager_with(deaf, Duration::from_secs(1), capture_timeout).await;
    let arrived = tokio::time::Instant::now()
        .checked_sub(capture_timeout)
        .expect("the machine has been running for longer than the capture timeout");
    let answer = tokio::time::timeout(
        PATIENCE,
        manager.capture_arrived(capture("https://example.com/late.pdf"), arrived),
    )
    .await
    .expect("the wait for the server counted from its own start, not the capture's arrival");
    let err = answer.unwrap_err();
    assert!(matches!(err, CoreError::NotStarted(_)), "{err}");
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
    let removal = tokio::spawn(async move { removing.remove(&[id], RemoveFiles::Keep).await });
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
    assert!(gone(manager.refresh_link(
        id,
        "https://example.com/again.bin",
        LinkFrom::Person
    )));
    assert!(gone(manager.move_to_queue(&[id], DEFAULT_QUEUE_ID)));
    let elsewhere = dir.path().join("elsewhere").to_string_lossy().into_owned();
    assert!(gone(manager.set_download_folder(id, &elsewhere).await));
    manager.resume_all().unwrap();
    assert_eq!(
        manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );

    removal.await.unwrap().unwrap();
    tokio::time::sleep(QUIET).await;
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
            let path = settle(&answer(name))
                .map_err(|reason| EngineError::TargetRejected(PathRejected::new(reason)))?;
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
        // One check under the lock is the failure; looking again would only wait out another `PATIENCE`.
        if under_lock.load(Ordering::SeqCst) == 0 && !lock_is_free_to_others(&manager) {
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

#[tokio::test]
async fn a_path_another_download_took_is_not_replaced_kept_or_asked_about() {
    use super::state::{Found, OnExisting};
    use wirecatch_protocol::app::WhenFileExists;

    let (manager, dir) = manager_with(
        |_| Box::pin(std::future::pending()),
        Duration::from_secs(1),
        DEFAULT_CAPTURE_TIMEOUT,
    )
    .await;
    let holder = manager
        .add_download(add_request("https://example.com/a.bin"))
        .await
        .unwrap();
    let late = manager
        .add_download(add_request("https://example.com/b.bin"))
        .await
        .unwrap();
    let mut guard = manager.lock();
    let state = &mut *guard;
    let path = dir.path().join("taken.bin");
    assert!(state.claimed.reserve(&path, holder));
    let late_path = state.downloads[&late].path().to_path_buf();

    for then in [
        WhenFileExists::Replace,
        WhenFileExists::KeepExisting,
        WhenFileExists::Ask,
    ] {
        let found = Found {
            path: path.clone(),
            size: 1,
            saved_at: None,
            then,
            same: true,
        };
        manager.on_found(state, late, &found);
        let entry = &state.downloads[&late];
        assert_eq!(entry.path(), late_path, "{then:?}");
        assert_eq!(entry.on_existing, Some(OnExisting::KeepBoth), "{then:?}");
        assert_eq!(entry.row.status, DownloadStatus::Queued, "{then:?}");
        assert_eq!(state.claimed.holder(&path), Some(holder), "{then:?}");
        assert!(state.file_exists.is_empty(), "{then:?}");
    }
}

/// A folder that is slow to answer holds only its own download: other work on the runtime goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn names_are_checked_on_disk_off_the_async_threads() {
    let gate = Arc::new(Notify::new());
    let engine = answers_on_cue(gate.clone(), "report.pdf");
    let (manager, _dir) =
        manager_with(engine, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/get?id=1"))
        .await
        .unwrap();
    let run_id = manager.lock().downloads[&id].run.as_ref().unwrap().id;

    let (told, entered) = tokio::sync::oneshot::channel::<()>();
    let told = std::sync::Mutex::new(Some(told));
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    let settling = {
        let manager = manager.clone();
        tokio::spawn(async move {
            let free_on_disk = move |_: &Path| {
                if let Some(told) = told.lock().unwrap().take() {
                    told.send(()).unwrap();
                }
                // The disk answers only once the runtime's one worker has run the task below.
                held.lock()
                    .unwrap()
                    .recv_timeout(PATIENCE)
                    .expect("the runtime stalled while the disk was checked");
                true
            };
            manager.settle_path_checking(id, run_id, &answer("report.pdf"), &free_on_disk)
        })
    };
    entered.await.unwrap();
    let other_work = tokio::spawn(async move { release.send(()) });
    let path = settling.await.unwrap();
    other_work.await.unwrap().unwrap();
    assert!(
        path.is_ok(),
        "the runtime stalled while the disk was checked"
    );
}

/// A client sending a list longer than the list may hold is answered while the manager's lock is held elsewhere: the list is refused before the lock is taken, so such a request never waits for it nor holds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_of_sites_too_long_is_refused_without_the_lock() {
    use wirecatch_protocol::app::AppSettingsPatch;
    use wirecatch_protocol::extension::{ExtensionSettingsPatch, MAX_IGNORED_SITES};

    let (manager, _dir) = manager_with(deaf, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let too_long = || ExtensionSettingsPatch {
        add_ignored_sites: Some(
            (0..=MAX_IGNORED_SITES)
                .map(|i| format!("s{i}.example"))
                .collect(),
        ),
        ..ExtensionSettingsPatch::default()
    };
    let held = manager.lock();
    let (answered, answers) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let from_browser = manager.update_extension_settings(too_long()).is_err();
            let from_window = manager
                .change_settings(AppSettingsPatch {
                    extension: Some(too_long()),
                    ..AppSettingsPatch::default()
                })
                .is_err();
            answered.send((from_browser, from_window)).unwrap();
        });
        let refused = answers.recv_timeout(PATIENCE);
        drop(held);
        assert_eq!(
            refused,
            Ok((true, true)),
            "the list was not refused while the lock was held elsewhere"
        );
    });
}

/// Whether another thread can take the manager's lock while the caller waits. A download's own task takes it for a moment now and then, so a bare `try_lock` fails at times without the caller holding it; a lock the caller holds keeps the other thread out until [`PATIENCE`] has passed.
fn lock_is_free_to_others(manager: &Manager) -> bool {
    let (taken, took) = std::sync::mpsc::channel();
    let inner = Arc::clone(&manager.inner);
    std::thread::spawn(move || {
        drop(inner.state.lock());
        checker_may_have_given_up(taken.send(()));
    });
    took.recv_timeout(PATIENCE).is_ok()
}

/// Takes the outcome of telling the check the lock was taken: after [`PATIENCE`] the check has given up and is no longer listening, which changes nothing.
fn checker_may_have_given_up(told: Result<(), std::sync::mpsc::SendError<()>>) {
    let (Ok(()) | Err(_)) = told;
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
    assert!(state.claimed.reserve(&path, DownloadId(1)));
    state.assert_consistent();
    state.claimed.release(&path, DownloadId(1));
    state.assert_consistent();
    state.claimed.insert(&path, DownloadId(1));
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

    {
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

    refuses_to_restart_split(&manager, id, &dir.path().join("other")).await;

    // The same after a restart: the split is saved with the download.
    manager.shutdown().await;
    drop(manager);
    let folders = KnownFolders::under_home(dir.path().join("home"));
    let reopened = Manager::open(ManagerConfig::for_tests(dir.path().join("data"), folders))
        .await
        .unwrap();
    reopened.use_fake_engine(Arc::new(obedient));
    refuses_to_restart_split(&reopened, id, &dir.path().join("other")).await;
    let stranded = chosen.join(old_path.file_name().unwrap());
    assert!(
        reopened.lock().claimed.contains(&stranded),
        "the name in the new folder was given up on restart"
    );
}

/// Moving download `id` to `elsewhere`, or giving it a new link, would start it
/// on split files: both are refused, and it stays failed and stopped.
async fn refuses_to_restart_split(manager: &Manager, id: DownloadId, elsewhere: &Path) {
    let again = manager
        .set_download_folder(id, &elsewhere.to_string_lossy())
        .await
        .unwrap_err();
    assert!(matches!(again, CoreError::WrongState(_)), "{again}");
    let relinked = manager
        .refresh_link(id, "https://example.com/big.iso?fresh", LinkFrom::Person)
        .unwrap_err();
    assert!(matches!(relinked, CoreError::WrongState(_)), "{relinked}");
    let state = manager.lock();
    let entry = &state.downloads[&id];
    assert!(entry.run.is_none(), "the download started again");
    assert_eq!(entry.row.status, DownloadStatus::Failed);
}

/// Every way a link arrives runs under its own rule for the local network: a
/// website's, confirmed in the window, under `Block`; the same link refreshed
/// by the person under `Auto`; and one from a page on the person's network
/// under `Auto` (D34.9, D39).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_way_a_link_arrives_runs_under_its_own_rule() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    let (manager, _dir) = manager_with(
        move |run| {
            record.lock().unwrap().push(run.private_network);
            Box::pin(async move {
                run.cancel.cancelled().await;
                Err(EngineError::Cancelled)
            })
        },
        PATIENCE,
        PATIENCE,
    )
    .await;
    let mut settings = manager.get_settings();
    settings.extension.ask_before_download = true;
    manager.update_settings(settings).unwrap();
    let rules = |n: usize| {
        let seen = seen.clone();
        async move {
            wait_until("the run starts", || seen.lock().unwrap().len() == n).await;
            seen.lock().unwrap().clone()
        }
    };
    let confirmed = |page: &str, link: &str| {
        let mut capture = capture(link);
        capture.page_url = Some(page.into());
        let manager = manager.clone();
        async move {
            manager.add_capture(capture).await.unwrap();
            let asked = manager.pending_confirmations().remove(0);
            manager
                .confirm_capture(asked.id, CaptureConfirmation::default())
                .await
                .unwrap()
        }
    };

    let id = confirmed("https://example.com/page", "http://93.184.215.14/a.pdf").await;
    assert_eq!(rules(1).await, [PrivateNetwork::Block]);
    // A page that shows what the website chose keeps its rule (D43.5).
    manager
        .refresh_link(id, "http://93.184.215.14/a.pdf", LinkFrom::SharedPage)
        .unwrap();
    assert_eq!(rules(2).await[1], PrivateNetwork::Block);
    manager
        .refresh_link(id, "http://93.184.215.14/a.pdf", LinkFrom::Person)
        .unwrap();
    assert_eq!(rules(3).await[2], PrivateNetwork::Auto);
    confirmed("http://nas/files", "http://93.184.215.15/b.pdf").await;
    assert_eq!(rules(4).await[3], PrivateNetwork::Auto);
    manager.shutdown().await;
}

#[tokio::test]
async fn a_download_that_fails_to_start_gives_its_slot_to_the_next() {
    let (manager, _dir) = manager_with(deaf, PATIENCE, DEFAULT_CAPTURE_TIMEOUT).await;
    let mut settings = manager.get_settings();
    settings.max_concurrent_downloads = 1;
    manager.update_settings(settings).unwrap();
    manager.set_queue_paused(DEFAULT_QUEUE_ID, true).unwrap();
    let mut ids = Vec::new();
    for name in ["broken", "second", "third"] {
        let link = format!("https://example.com/{name}.bin");
        ids.push(manager.add_download(add_request(&link)).await.unwrap());
    }
    let (broken, second, third) = (ids[0], ids[1], ids[2]);
    manager.lock().downloads.get_mut(&broken).unwrap().row.url = "not a link".into();

    manager.set_queue_paused(DEFAULT_QUEUE_ID, false).unwrap();

    let state = manager.lock();
    assert_eq!(state.downloads[&broken].row.status, DownloadStatus::Failed);
    assert!(state.downloads[&second].is_running());
    assert!(!state.downloads[&third].is_running());
}

#[tokio::test]
async fn a_download_runs_under_the_apps_limits_and_its_queues() {
    const CONNECTIONS: u8 = 3;
    const GONE: wirecatch_protocol::QueueId = wirecatch_protocol::QueueId(99);
    let (manager, _dir) = manager_with(deaf, PATIENCE, DEFAULT_CAPTURE_TIMEOUT).await;
    let mut settings = manager.get_settings();
    settings.connections_per_download = CONNECTIONS;
    manager.update_settings(settings).unwrap();
    let state = manager.lock();
    let global = &manager.inner.global_limiter;
    let queue = &state.slot(DEFAULT_QUEUE_ID).unwrap().limiter;

    let policy = manager.run_policy(&state, DEFAULT_QUEUE_ID);
    assert_eq!(policy.connections, usize::from(CONNECTIONS));
    assert_eq!(policy.proxy, state.app_proxy());
    assert!(
        matches!(&policy.limiters[..], [a, b] if Arc::ptr_eq(a, global) && Arc::ptr_eq(b, queue))
    );

    for app_only in [manager.run_policy(&state, GONE), manager.app_policy(&state)] {
        assert_eq!(app_only.connections, usize::from(CONNECTIONS));
        assert!(matches!(&app_only.limiters[..], [a] if Arc::ptr_eq(a, global)));
    }
}

#[tokio::test]
async fn a_queue_with_its_own_connections_runs_its_downloads_with_them() {
    const OWN: u8 = 5;
    let (manager, _dir) = manager_with(deaf, PATIENCE, DEFAULT_CAPTURE_TIMEOUT).await;
    let mut main = manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == DEFAULT_QUEUE_ID)
        .unwrap();
    main.settings.connections = wirecatch_protocol::app::Setting::Own(OWN);
    manager
        .update_queue(DEFAULT_QUEUE_ID, main.settings)
        .unwrap();
    let state = manager.lock();
    assert_eq!(
        manager.run_policy(&state, DEFAULT_QUEUE_ID).connections,
        usize::from(OWN)
    );
    assert_ne!(manager.app_policy(&state).connections, usize::from(OWN));
}

/// A run that ignores being asked to stop until `let_go` is notified, then ends as cancelled.
fn held_until(let_go: &Arc<Notify>) -> impl Fn(FakeRun) -> EngineFuture + use<> {
    let let_go = let_go.clone();
    move |_run: FakeRun| -> EngineFuture {
        let let_go = let_go.clone();
        Box::pin(async move {
            let_go.notified().await;
            Err(EngineError::Cancelled)
        })
    }
}

/// Polls `work` once, then drops it, as a caller that stops waiting (a timeout, a `select!`) does.
async fn give_up_on<F: Future>(work: F) {
    let mut work = std::pin::pin!(work);
    let first = std::future::poll_fn(|cx| std::task::Poll::Ready(work.as_mut().poll(cx))).await;
    assert!(first.is_pending(), "finished before the caller gave up");
}

/// A held run and a manager whose stop timeout outlasts the test: the run stops only when the test lets it go.
async fn running_download(let_go: &Arc<Notify>) -> (Manager, DownloadId, tempfile::TempDir) {
    let (manager, dir) =
        manager_with(held_until(let_go), 2 * PATIENCE, DEFAULT_CAPTURE_TIMEOUT).await;
    let id = manager
        .add_download(add_request("https://example.com/big.iso"))
        .await
        .unwrap();
    wait_until("the download to run", || {
        manager.lock().downloads[&id].run.is_some()
    })
    .await;
    (manager, id, dir)
}

/// A removal whose caller stops waiting still removes: the download is not left hidden for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removal_finishes_after_its_caller_gives_up() {
    let let_go = Arc::new(Notify::new());
    let (manager, id, _dir) = running_download(&let_go).await;
    give_up_on(manager.remove(&[id], RemoveFiles::Keep)).await;
    let_go.notify_one();
    wait_until("the download to be removed", || {
        !manager.lock().downloads.contains_key(&id)
    })
    .await;
}

/// A move whose caller stops waiting still ends: the download is not left marked as moving, which would keep it from ever starting again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_finishes_after_its_caller_gives_up() {
    let let_go = Arc::new(Notify::new());
    let (manager, id, dir) = running_download(&let_go).await;
    let elsewhere = dir.path().join("elsewhere");
    give_up_on(manager.set_download_folder(id, &elsewhere.to_string_lossy())).await;
    let_go.notify_one();
    wait_until("the move to end", || {
        manager.lock().downloads[&id].hold.is_free()
    })
    .await;
}

/// A repeat of a handover that was still being added waits for that add's outcome: joining the download would answer it early and make the first capture's download the app's (D89.2).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_handover_never_joins_its_own_download() {
    let (manager, _dir) = manager_with(deaf, Duration::from_secs(1), 2 * PATIENCE).await;
    let first = capture("https://example.com/held.pdf");
    let url = Url::parse(&first.url).unwrap();
    let adding = tokio::spawn({
        let manager = manager.clone();
        let first = first.clone();
        async move { manager.add_capture(first).await }
    });
    wait_until("the first capture to be added", || {
        !manager.lock().downloads.is_empty()
    })
    .await;
    // As a repeat does that checked for its handover before the first add registered it.
    assert!(manager.join_unfinished(&first, &url).unwrap().is_none());
    let handovers = &manager.lock().handovers;
    let target = &handovers.get(&first.handover_id).unwrap().target;
    assert!(
        matches!(target, super::handovers::Target::Download(_)),
        "{target:?}"
    );
    adding.abort();
}

/// A run that stops only when `leave` lets it, however soon it is asked to.
fn leaves_on_cue(leave: Arc<Notify>) -> impl Fn(FakeRun) -> EngineFuture {
    move |_run: FakeRun| {
        let leave = leave.clone();
        Box::pin(async move {
            leave.notified().await;
            Err(EngineError::Cancelled)
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_all_never_keeps_a_download_being_cancelled() {
    let leave = Arc::new(Notify::new());
    let (manager, _dir) = manager_with(
        leaves_on_cue(leave.clone()),
        PATIENCE,
        DEFAULT_CAPTURE_TIMEOUT,
    )
    .await;
    let id = manager
        .add_download(add_request("https://example.com/big.iso"))
        .await
        .unwrap();

    manager.cancel(id).unwrap();
    manager.pause_all().unwrap();
    leave.notify_one();

    wait_until("the run's end", || {
        !manager.lock().downloads[&id].is_running()
    })
    .await;
    assert_eq!(
        manager.get_download(id).unwrap().status,
        DownloadStatus::Cancelled
    );
}

/// A run whose server answers with `report.pdf` when `gate` lets it, ends when `leave` lets it, and says so on `answered`.
fn offers_a_file_on_cue(
    gate: Arc<Notify>,
    answered: Arc<Notify>,
    leave: Arc<Notify>,
) -> impl Fn(FakeRun) -> EngineFuture {
    move |run: FakeRun| {
        let (gate, answered, leave) = (gate.clone(), answered.clone(), leave.clone());
        Box::pin(async move {
            gate.notified().await;
            let settle = run.settle.expect("the path is not settled yet");
            let refused = settle(&answer("report.pdf")).unwrap_err();
            answered.notify_one();
            leave.notified().await;
            Err(EngineError::TargetRejected(PathRejected::new(refused)))
        })
    }
}

/// A person's stop in the moment after the duplicate was found decides, not the setting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_found_duplicate_does_not_override_a_stop_the_person_chose() {
    for (stop, ended) in [
        (
            Manager::pause as fn(&Manager, DownloadId) -> crate::error::Result<()>,
            DownloadStatus::Paused,
        ),
        (Manager::cancel, DownloadStatus::Cancelled),
    ] {
        let (gate, answered, leave) = (
            Arc::new(Notify::new()),
            Arc::new(Notify::new()),
            Arc::new(Notify::new()),
        );
        let (manager, dir) = manager_with(
            offers_a_file_on_cue(gate.clone(), answered.clone(), leave.clone()),
            PATIENCE,
            DEFAULT_CAPTURE_TIMEOUT,
        )
        .await;
        let folder = dir.path().join("saved");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("report.pdf"), b"older").unwrap();
        let mut request = add_request("https://example.com/get?id=1");
        request.folder = Some(folder.to_string_lossy().into_owned());
        let id = manager.add_download(request).await.unwrap();

        gate.notify_one();
        tokio::time::timeout(PATIENCE, answered.notified())
            .await
            .expect("the server never answered");
        stop(&manager, id).unwrap();
        leave.notify_one();

        wait_until("the run's end", || {
            !manager.lock().downloads[&id].is_running()
        })
        .await;
        assert_eq!(manager.get_download(id).unwrap().status, ended);
        assert_eq!(manager.pending_file_exists(), vec![]);
    }
}

/// A new download never takes a name whose old files are still being deleted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removal_keeps_the_path_until_its_files_are_deleted() {
    let (manager, _dir) =
        manager_with(obedient, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let mut request = add_request("https://example.com/big.iso");
    request.start = false;
    let id = manager.add_download(request).await.unwrap();
    let path = PathBuf::from(&manager.lock().downloads[&id].row.path);

    let entered = Arc::new(Notify::new());
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    let told = entered.clone();
    manager.use_before_deleting(Arc::new(move || {
        told.notify_one();
        assert!(
            held.lock().unwrap().recv().is_err(),
            "ends when the sender is dropped"
        );
    }));
    let removing = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.remove(&[id], RemoveFiles::Keep).await })
    };
    tokio::time::timeout(PATIENCE, entered.notified())
        .await
        .expect("the deletion never began");
    assert!(manager.lock().claimed.contains(&path));
    drop(release);
    tokio::time::timeout(PATIENCE, removing)
        .await
        .expect("the removal never ended")
        .unwrap()
        .unwrap();
    assert!(!manager.lock().claimed.contains(&path));
    manager.lock().assert_consistent();
}

/// Opens a download's move to `chosen` with the mover held, quits, lets the mover go, then reopens the
/// list. Returns the path the reopened list names.
async fn path_after_a_quit_during_a_move(
    manager: Manager,
    id: DownloadId,
    data: &Path,
    home: &Path,
    chosen: &Path,
) -> PathBuf {
    let entered = Arc::new(Notify::new());
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    let told = entered.clone();
    manager.use_fake_mover(Arc::new(move |from, to| {
        told.notify_one();
        assert!(
            held.lock().unwrap().recv().is_err(),
            "ends when the sender is dropped"
        );
        std::fs::rename(from, to)
    }));
    let moving = {
        let (manager, chosen) = (manager.clone(), chosen.to_string_lossy().into_owned());
        tokio::spawn(async move { manager.set_download_folder(id, &chosen).await })
    };
    tokio::time::timeout(PATIENCE, entered.notified())
        .await
        .expect("the move never began");
    let mut quit = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.shutdown().await })
    };
    let returned_first = tokio::time::timeout(QUIET, &mut quit).await.is_ok();
    assert!(!returned_first, "shutdown returned while the move was held");
    drop(release);
    tokio::time::timeout(PATIENCE, moving)
        .await
        .expect("the move never ended")
        .unwrap()
        .unwrap();
    quit.await.unwrap();
    drop(manager);
    let reopened = Manager::open(ManagerConfig::for_tests(
        data.to_path_buf(),
        KnownFolders::under_home(home.to_path_buf()),
    ))
    .await
    .unwrap();
    let path = PathBuf::from(&reopened.lock().downloads[&id].row.path);
    reopened.shutdown().await;
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quit_during_a_move_of_a_finished_file_keeps_it_listed() {
    let (manager, dir) =
        manager_with(obedient, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let mut request = add_request("https://example.com/big.iso");
    request.start = false;
    let id = manager.add_download(request).await.unwrap();
    let old_path = PathBuf::from(&manager.lock().downloads[&id].row.path);
    std::fs::create_dir_all(old_path.parent().unwrap()).unwrap();
    std::fs::write(&old_path, b"finished").unwrap();
    {
        let mut state = manager.lock();
        let entry = state.downloads.get_mut(&id).unwrap();
        super::run::mark_completed(entry, 8, wirecatch_protocol::UnixMillis(1));
        manager.save(&mut state, id);
    }
    let chosen = dir.path().join("chosen");
    let path = path_after_a_quit_during_a_move(
        manager,
        id,
        &dir.path().join("data"),
        &dir.path().join("home"),
        &chosen,
    )
    .await;
    assert!(
        path.is_file(),
        "the finished row names {}, where no file is; the file is at {}",
        path.display(),
        chosen.join(old_path.file_name().unwrap()).display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quit_during_a_move_of_a_partial_download_keeps_its_progress() {
    let (manager, dir) =
        manager_with(obedient, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let mut request = add_request("https://example.com/big.iso");
    request.start = false;
    let id = manager.add_download(request).await.unwrap();
    let old_path = PathBuf::from(&manager.lock().downloads[&id].row.path);
    std::fs::create_dir_all(old_path.parent().unwrap()).unwrap();
    for file in naming::partial_files(&old_path) {
        std::fs::write(file, b"partial").unwrap();
    }
    let chosen = dir.path().join("chosen");
    let path = path_after_a_quit_during_a_move(
        manager,
        id,
        &dir.path().join("data"),
        &dir.path().join("home"),
        &chosen,
    )
    .await;
    assert!(
        naming::partial_files(&path)[0].is_file(),
        "the row names {}, with no part file; the part file is at {}",
        path.display(),
        naming::partial_files(&chosen.join(old_path.file_name().unwrap()))[0].display()
    );
}

/// A run that fails after its server answered, before any data, leaves the older file standing as what it is: the row must not read it as its own after a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replace_that_failed_at_its_start_is_not_completed_by_a_restart() {
    let engine = |run: FakeRun| -> EngineFuture {
        Box::pin(async move {
            let settle = run.settle.expect("nothing was written yet");
            let path = settle(&answer("report.pdf"))
                .map_err(|reason| EngineError::TargetRejected(PathRejected::new(reason)))?;
            let folder = path.parent().unwrap().display().to_string();
            Err(EngineError::TargetRejected(PathRejected::new(format!(
                "{folder} refused the part file"
            ))))
        })
    };
    let (manager, dir) =
        manager_with(engine, Duration::from_secs(5), DEFAULT_CAPTURE_TIMEOUT).await;
    let mut request = add_request("https://example.com/report.pdf");
    request.start = false;
    let id = manager.add_download(request).await.unwrap();
    let path = PathBuf::from(&manager.lock().downloads[&id].row.path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"the older file").unwrap();
    {
        let mut state = manager.lock();
        let entry = state.downloads.get_mut(&id).unwrap();
        entry.naming.settled = true;
        entry.on_existing = Some(super::state::OnExisting::Replace);
    }
    manager.resume(id).unwrap();
    wait_until("the run's failure", || {
        manager.lock().downloads[&id].row.status == DownloadStatus::Failed
    })
    .await;
    manager.shutdown().await;
    drop(manager);

    let reopened = Manager::open(ManagerConfig::for_tests(
        dir.path().join("data"),
        KnownFolders::under_home(dir.path().join("home")),
    ))
    .await
    .unwrap();
    assert_ne!(
        reopened.lock().downloads[&id].row.status,
        DownloadStatus::Completed
    );
    reopened.shutdown().await;
}
