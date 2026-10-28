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
use wirecatch_test_support::PATIENCE;

use super::fake::{EngineFuture, FakeEngine, FakeRun};
use super::settle::ServerAnswer;
use super::{LinkFrom, Manager};
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
        // A page on the person's network (D39), so no name is looked up first.
        page_url: Some("http://intranet.test/".into()),
        frame_url: None,
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
    let reopened = Manager::open(ManagerConfig::new(dir.path().join("data"), folders))
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
