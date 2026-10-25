//! WP6 acceptance 1 and 4: the commands the interface calls drive the real core against a
//! local HTTP server, settings and queues survive a restart, and the tray's actions work.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::Sandbox;
use wirecatch_core::Manager;
use wirecatch_desktop_lib::api::{self, Reveal, ToOpen};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::lifecycle::{quit_core, quit_sequence};
use wirecatch_desktop_lib::tray::{Shell, TrayAction, perform};
use wirecatch_engine::units::{KIB, KIB_USIZE, MIB_USIZE};
use wirecatch_protocol::app::{
    AddRequest, AddonId, AddonState, AppSettingsPatch, CaptureConfirmation, DownloadStatus,
    DownloadSummary, FinishNotice, MediaSelection, Question, Queue, QueueNotify, QueueSettings,
    Theme,
};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};
use wirecatch_protocol::wire::ErrorCode;
use wirecatch_protocol::{DownloadId, QuestionId};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::{PATIENCE, TestResult, test_data};

/// A file big and slow enough to still be running while the test pauses and moves it.
const SLOW_LEN: usize = 4 * MIB_USIZE;
const SLOW_BPS: u64 = 64 * KIB;
/// A file that downloads at once.
const FAST_LEN: usize = 300 * KIB_USIZE;

/// How often a wait looks at the download list again.
const POLL: Duration = Duration::from_millis(20);

/// Serves `len` bytes at every path, with ranges, at `bps` bytes per second for each
/// connection (`None` is full speed).
async fn serve(len: usize, bps: Option<u64>) -> TestServer {
    let config = Config {
        ranges: true,
        etag: Some("\"v1\"".into()),
        per_connection_bps: bps,
        ..Config::default()
    };
    TestServer::start(test_data(len), config).await
}

/// The folder test downloads are saved to.
fn saved_folder(sandbox: &Sandbox) -> TestResult<PathBuf> {
    let dir = sandbox.dir.path().join("saved");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn request(server: &TestServer, path: &str, folder: &Path, start: bool) -> AddRequest {
    AddRequest {
        url: server.url(path).to_string().into(),
        queue: None,
        folder: Some(folder.to_string_lossy().into_owned()),
        file_name: None,
        start,
    }
}

fn row(manager: &Manager, id: DownloadId) -> Option<DownloadSummary> {
    api::list_downloads(manager)
        .into_iter()
        .find(|r| r.id == id)
}

/// Waits until the download's row satisfies `ok`, and returns it.
async fn wait_for(
    manager: &Manager,
    id: DownloadId,
    what: &str,
    ok: impl Fn(&DownloadSummary) -> bool,
) -> DownloadSummary {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(r) = row(manager, id)
            && ok(&r)
        {
            return r;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "download {} never became {what}; last seen: {:?}",
            id.0,
            row(manager, id).map(|r| (r.status, r.done, r.error))
        );
        tokio::time::sleep(POLL).await;
    }
}

async fn wait_status(manager: &Manager, id: DownloadId, status: DownloadStatus) -> DownloadSummary {
    wait_for(manager, id, &format!("{status:?}"), |r| r.status == status).await
}

/// Running means bytes are moving, or about to.
fn running(r: &DownloadSummary) -> bool {
    matches!(
        r.status,
        DownloadStatus::Connecting | DownloadStatus::Downloading
    )
}

/// Changes settings and makes a queue through the same calls the Settings screen makes, and
/// returns the queue.
fn change_settings_and_make_a_queue(manager: &Manager) -> TestResult<Queue> {
    api::change_settings(
        manager,
        AppSettingsPatch {
            theme: Some(Theme::Dark),
            max_concurrent_downloads: Some(7),
            finish_notice: Some(FinishNotice::Window),
            ..AppSettingsPatch::default()
        },
    )?;
    let night = api::create_queue(
        manager,
        QueueSettings {
            max_concurrent: 1,
            notify: QueueNotify::WhenDone,
            ..QueueSettings::new("Night")
        },
    )?;
    Ok(night)
}

/// Adds a running download and takes it through pause, resume, cancel and retry.
async fn pause_resume_cancel_retry(
    manager: &Manager,
    request: AddRequest,
) -> TestResult<DownloadId> {
    let id = api::add_download(manager, request).await?;
    wait_for(manager, id, "running", running).await;
    api::pause(manager, id)?;
    wait_status(manager, id, DownloadStatus::Paused).await;
    api::resume(manager, id)?;
    wait_for(manager, id, "running again", running).await;
    api::cancel(manager, id)?;
    wait_status(manager, id, DownloadStatus::Cancelled).await;
    api::retry(manager, id)?;
    wait_for(manager, id, "running after retry", running).await;
    Ok(id)
}

/// The downloads of `queue`, in their order.
fn in_queue(manager: &Manager, queue: &Queue) -> Vec<DownloadId> {
    api::list_downloads(manager)
        .into_iter()
        .filter(|r| r.queue == queue.id)
        .map(|r| r.id)
        .collect()
}

/// A finished download opens, its folder shows with the file selected, and an unfinished one
/// does not open.
async fn open_and_reveal(
    manager: &Manager,
    server: &TestServer,
    saved: &Path,
    unfinished: DownloadId,
) -> TestResult<DownloadId> {
    let id = api::add_download(manager, request(server, "small.zip", saved, true)).await?;
    let done = wait_status(manager, id, DownloadStatus::Completed).await;
    let ToOpen::File(file) = api::file_to_open(manager, id)? else {
        return Err("a zip file asked for trust before opening".into());
    };
    assert_eq!(std::fs::read(&file)?, server.data);
    assert_eq!(file, PathBuf::from(&done.path));
    assert_eq!(api::item_to_reveal(manager, id)?, Reveal::File(file));
    let Err(not_done) = api::file_to_open(manager, unfinished) else {
        return Err("an unfinished download opened".into());
    };
    assert!(not_done.message.contains("not finished"), "{not_done}");
    Ok(id)
}

/// A downloaded program opens only through the shell's own warning, which names its type and
/// the site it came from; nothing the page sends can skip it (DRS-5).
async fn a_program_opens_only_when_trusted(
    manager: &Manager,
    server: &TestServer,
    saved: &Path,
) -> TestResult {
    let program = api::add_download(manager, request(server, "Setup.EXE", saved, true)).await?;
    let done = wait_status(manager, program, DownloadStatus::Completed).await;
    let ToOpen::Program { path, warning } = api::file_to_open(manager, program)? else {
        return Err("a program opened without the person's trust".into());
    };
    assert_eq!(path, PathBuf::from(&done.path));
    assert!(warning.title.contains("Setup.EXE"), "{warning:?}");
    assert!(
        warning.message.contains(".exe file")
            && warning.message.contains("can run code")
            && warning.message.contains("127.0.0.1"),
        "{warning:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_drive_the_core_and_state_survives_a_restart() -> TestResult {
    let sandbox = Sandbox::new()?;
    let saved = saved_folder(&sandbox)?;
    let slow = serve(SLOW_LEN, Some(SLOW_BPS)).await;
    let fast = serve(FAST_LEN, None).await;
    let manager = sandbox.open().await?;

    let night = change_settings_and_make_a_queue(&manager)?;
    let first =
        pause_resume_cancel_retry(&manager, request(&slow, "big.bin", &saved, true)).await?;

    // Move between queues and reorder.
    let second = api::add_download(&manager, request(&slow, "b.bin", &saved, false)).await?;
    let third = api::add_download(&manager, request(&slow, "c.bin", &saved, false)).await?;
    wait_status(&manager, second, DownloadStatus::Paused).await;
    api::move_to_queue(&manager, &[second, third], night.id)?;
    assert_eq!(row(&manager, second).map(|r| r.queue), Some(night.id));
    api::reorder(&manager, night.id, &[third, second])?;
    assert_eq!(in_queue(&manager, &night), [third, second]);

    let finished = open_and_reveal(&manager, &fast, &saved, second).await?;
    a_program_opens_only_when_trusted(&manager, &fast, &saved).await?;

    // Errors reach the interface as the core's plain words.
    let mut bad = request(&fast, "x", &saved, true);
    bad.url = "not a link".into();
    let bad = api::add_download(&manager, bad).await.unwrap_err();
    assert!(bad.message.contains("is not a valid link"), "{bad}");

    // Remove, with the file.
    api::remove(&manager, &[first], true).await?;
    assert!(row(&manager, first).is_none());

    // The tray's Pause all and Resume all.
    perform(TrayAction::PauseAll, Some(&manager), &NoShell)?;
    assert!(
        api::list_downloads(&manager)
            .iter()
            .all(|r| !running(r) && r.status != DownloadStatus::Queued)
    );
    perform(TrayAction::ResumeAll, Some(&manager), &NoShell)?;
    wait_for(&manager, second, "resumed by Resume all", |r| {
        r.status != DownloadStatus::Paused
    })
    .await;

    // Restart: everything comes back.
    manager.shutdown().await;
    drop(manager);
    let manager = sandbox.open().await?;
    let settings = api::get_settings(&manager);
    assert_eq!(settings.theme, Theme::Dark);
    assert_eq!(settings.max_concurrent_downloads, 7);
    assert_eq!(settings.finish_notice, FinishNotice::Window);
    let queues = api::list_queues(&manager);
    let night_again = queues
        .iter()
        .find(|q| q.settings.name == "Night")
        .ok_or("the queue is missing after restart")?;
    assert_eq!(night_again.settings.max_concurrent, 1);
    assert_eq!(night_again.settings.notify, QueueNotify::WhenDone);
    let ids: Vec<_> = api::list_downloads(&manager)
        .into_iter()
        .map(|r| r.id)
        .collect();
    for id in [second, third, finished] {
        assert!(
            ids.contains(&id),
            "download {} is missing after restart",
            id.0
        );
    }
    assert_eq!(
        row(&manager, finished).map(|r| r.status),
        Some(DownloadStatus::Completed)
    );
    assert_eq!(in_queue(&manager, night_again), [third, second]);
    manager.shutdown().await;
    Ok(())
}

struct NoShell;

impl Shell for NoShell {
    fn open_window(&self) {}
    fn quit(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_right_after_start_opens_then_stops_the_manager() -> TestResult {
    let sandbox = Sandbox::new()?;
    let core = Core::new(sandbox.config()?);
    assert!(core.get().is_none(), "not opened yet");
    let exited = AtomicBool::new(false);
    quit_core(Some(&core), || {
        let manager = core.get().expect("opened by the quit");
        let err = api::pause_all(manager).unwrap_err();
        assert!(err.message.contains("shutting down"), "{err}");
        exited.store(true, Ordering::SeqCst);
    })
    .await;
    assert!(exited.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_stops_the_manager_before_exiting() -> TestResult {
    let sandbox = Sandbox::new()?;
    let slow = serve(SLOW_LEN, Some(SLOW_BPS)).await;
    let manager = sandbox.open().await?;
    let saved = saved_folder(&sandbox)?;
    let id = api::add_download(&manager, request(&slow, "q.bin", &saved, true)).await?;
    wait_for(&manager, id, "running", running).await;

    let exited = AtomicBool::new(false);
    quit_sequence(Some(&manager), || {
        // By the time the process exits, the manager has stopped taking work.
        let err = api::pause_all(&manager).unwrap_err();
        assert!(err.message.contains("shutting down"), "{err}");
        exited.store(true, Ordering::SeqCst);
    })
    .await;
    assert!(exited.load(Ordering::SeqCst));
    drop(manager);

    // The running download continues on the next start.
    let manager = sandbox.open().await?;
    wait_for(&manager, id, "running after the restart", running).await;
    manager.shutdown().await;
    Ok(())
}

/// WP12: the media and add-on commands reach the real core, not a stand-in. Nothing here
/// installs the add-on, so the test stays off the internet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_and_addon_commands_reach_the_core() -> TestResult {
    let sandbox = Sandbox::new()?;
    let manager = sandbox.open().await?;

    let addons = api::list_addons(&manager);
    assert_eq!(addons.len(), 1);
    assert_eq!(addons[0].id, AddonId::Media);
    assert_eq!(addons[0].state, AddonState::NotInstalled);
    assert!(
        addons[0].size.is_some(),
        "the download size is known before installing"
    );

    assert_eq!(api::pending_media_choices(&manager), []);
    assert_eq!(api::pending_addon_requests(&manager), []);
    let selection = MediaSelection {
        option_id: "best".into(),
        folder: None,
        file_name: None,
        queue: None,
        later: false,
        output: wirecatch_protocol::app::OutputFormat::default(),
        audio_kbps: None,
        subtitles: Vec::new(),
        subtitle_files: false,
        chapters: false,
        every_video: None,
    };
    let gone = api::choose_media(&manager, QuestionId(99), selection)
        .await
        .unwrap_err();
    assert_eq!(gone.code, ErrorCode::NotFound, "{gone}");
    let gone = api::dismiss_media_choice(&manager, QuestionId(99)).unwrap_err();
    assert_eq!(gone.code, ErrorCode::NotFound, "{gone}");

    let unknown = api::install_addon(&manager, AddonId::Unknown)
        .await
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound, "{unknown}");
    let unknown = api::update_addon(&manager, AddonId::Unknown)
        .await
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound, "{unknown}");
    // Removing one that is not installed is fine and leaves it not installed.
    api::remove_addon(&manager, AddonId::Media).await?;
    // Stopping with nothing in progress is fine; an unknown add-on is refused.
    api::cancel_addon_operation(&manager, AddonId::Media).await?;
    let unknown = api::cancel_addon_operation(&manager, AddonId::Unknown)
        .await
        .unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound, "{unknown}");
    // Nothing installed: the check has nothing to look at and says the add-on is not installed.
    let checked = api::check_addon_updates(&manager).await?;
    assert_eq!(checked[0].state, AddonState::NotInstalled);
    assert_eq!(
        api::list_addons(&manager)[0].state,
        AddonState::NotInstalled
    );

    // Opening looks the download up in the core by id.
    let missing = api::file_to_open(&manager, DownloadId(4242)).unwrap_err();
    assert!(missing.message.contains("no longer exists"), "{missing}");
    manager.shutdown().await;
    Ok(())
}

/// D23: a download window finds its question, says when the file is already in the list, and
/// Download later adds it paused, answering with the new download's id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_window_finds_its_question_and_its_download() -> TestResult {
    const REPORT_LEN: u64 = 64 * KIB;
    let sandbox = Sandbox::new()?;
    let fast = serve(usize::try_from(REPORT_LEN)?, None).await;
    let manager = sandbox.open().await?;
    assert!(
        api::get_settings(&manager).extension.ask_before_download,
        "a window shows first on a fresh install"
    );
    let capture = |handover: &str| NewDownload {
        handover_id: handover.into(),
        url: fast.url("report.pdf").to_string(),
        referrer: None,
        page_url: Some("https://intranet.test/page".into()),
        frame_url: None,
        suggested_name: None,
        mime: None,
        size_hint: Some(REPORT_LEN),
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    };
    manager.add_capture(capture("first")).await?;
    let asked = api::pending_confirmations(&manager).remove(0);
    let Some(Question::Capture(found)) = api::question(&manager, asked.id) else {
        panic!("the window cannot find its question")
    };
    assert_eq!(found, asked);
    assert!(api::earlier_download(&manager, asked.id).is_none());
    assert!(api::question(&manager, QuestionId(4242)).is_none());

    let later = CaptureConfirmation {
        later: true,
        ..CaptureConfirmation::default()
    };
    let id = api::confirm_capture(&manager, asked.id, later).await?;
    assert!(
        api::question(&manager, asked.id).is_none(),
        "answered: never asked again"
    );
    let row = api::get_download(&manager, id)?;
    assert_eq!(row.status, DownloadStatus::Paused);

    manager.add_capture(capture("second")).await?;
    let again = api::pending_confirmations(&manager).remove(0);
    assert_eq!(
        api::earlier_download(&manager, again.id).map(|r| r.id),
        Some(id)
    );
    assert!(api::get_download(&manager, DownloadId(4242)).is_err());
    manager.shutdown().await;
    Ok(())
}
