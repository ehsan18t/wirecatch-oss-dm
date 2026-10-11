//! WP6 acceptance 1 and 4: the commands the interface calls drive the real core against a
//! local HTTP server, settings and queues survive a restart, and the tray's actions work.

mod common;

use common::{Sandbox, TestServer, request, row, running, wait_for, wait_status};
use wirecatch_desktop_lib::api::{self, Reveal};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::lifecycle::{quit_core, quit_sequence};
use wirecatch_desktop_lib::questions::Question;
use wirecatch_desktop_lib::tray::{Shell, TrayAction, perform};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    AddonState, CaptureConfirmation, DownloadStatus, MediaSelection, Theme,
};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};

/// A file big and slow enough to still be running while the test pauses and moves it.
const SLOW_LEN: usize = 4 * 1024 * 1024;
const SLOW_BPS: u64 = 64 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_drive_the_core_and_state_survives_a_restart() {
    let sandbox = Sandbox::new();
    let saved = sandbox.downloads();
    let slow = TestServer::start(SLOW_LEN, Some(SLOW_BPS)).await;
    let fast = TestServer::start(300 * 1024, None).await;
    let m = sandbox.open().await;

    // Settings and a queue, changed through the same calls the Settings screen makes.
    let mut settings = api::get_settings(&m);
    settings.theme = Theme::Dark;
    settings.max_concurrent_downloads = 7;
    settings.notify_on_complete = false;
    api::update_settings(&m, settings).unwrap();
    let mut night = api::create_queue(&m, "Night").unwrap();
    night.max_concurrent = 1;
    api::update_queue(&m, night.clone()).unwrap();

    // Add, pause, resume, cancel, retry.
    let a = api::add_download(&m, request(slow.url("big.bin"), &saved, true))
        .await
        .unwrap();
    wait_for(&m, a, "running", running).await;
    api::pause(&m, a).unwrap();
    wait_status(&m, a, DownloadStatus::Paused).await;
    api::resume(&m, a).unwrap();
    wait_for(&m, a, "running again", running).await;
    api::cancel(&m, a).unwrap();
    wait_status(&m, a, DownloadStatus::Cancelled).await;
    api::retry(&m, a).unwrap();
    wait_for(&m, a, "running after retry", running).await;

    // Move between queues and reorder.
    let b = api::add_download(&m, request(slow.url("b.bin"), &saved, false))
        .await
        .unwrap();
    let c = api::add_download(&m, request(slow.url("c.bin"), &saved, false))
        .await
        .unwrap();
    wait_status(&m, b, DownloadStatus::Paused).await;
    api::move_to_queue(&m, &[b, c], night.id).unwrap();
    assert_eq!(row(&m, b).unwrap().queue, night.id);
    api::reorder(&m, night.id, &[c, b]).unwrap();
    let in_night: Vec<_> = api::list_downloads(&m)
        .into_iter()
        .filter(|r| r.queue == night.id)
        .map(|r| r.id)
        .collect();
    assert_eq!(in_night, [c, b]);

    // A finished download opens, and its folder shows with the file selected.
    let d = api::add_download(&m, request(fast.url("small.zip"), &saved, true))
        .await
        .unwrap();
    let done = wait_status(&m, d, DownloadStatus::Completed).await;
    let file = api::file_to_open(&m, d, false).unwrap();
    assert!(!api::download_is_program(&m, d).unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), fast.data);
    assert_eq!(file, std::path::PathBuf::from(&done.path));
    assert_eq!(api::item_to_reveal(&m, d).unwrap(), Reveal::File(file));
    let not_done = api::file_to_open(&m, b, false).unwrap_err();
    assert!(not_done.contains("not finished"), "{not_done}");

    // A downloaded program only opens once the person said they trust its site.
    let program = api::add_download(&m, request(fast.url("Setup.EXE"), &saved, true))
        .await
        .unwrap();
    wait_status(&m, program, DownloadStatus::Completed).await;
    assert!(api::download_is_program(&m, program).unwrap());
    let refused = api::file_to_open(&m, program, false).unwrap_err();
    assert!(
        refused.contains("can run code") && refused.contains("127.0.0.1"),
        "{refused}"
    );
    assert!(api::file_to_open(&m, program, true).is_ok());

    // Errors reach the interface as the core's plain words.
    let bad = api::add_download(&m, request("not a link".into(), &saved, true))
        .await
        .unwrap_err();
    assert!(bad.contains("is not a valid link"), "{bad}");

    // Remove, with the file.
    api::remove(&m, &[a], true).await.unwrap();
    assert!(row(&m, a).is_none());

    // The tray's Pause all and Resume all.
    let shell = NoShell;
    perform(TrayAction::PauseAll, Some(&m), &shell).unwrap();
    assert!(
        api::list_downloads(&m)
            .iter()
            .all(|r| !running(r) && r.status != DownloadStatus::Queued)
    );
    perform(TrayAction::ResumeAll, Some(&m), &shell).unwrap();
    wait_for(&m, b, "resumed by Resume all", |r| {
        r.status != DownloadStatus::Paused
    })
    .await;

    // Restart: everything comes back.
    m.shutdown().await;
    drop(m);
    let m = sandbox.open().await;
    let settings = api::get_settings(&m);
    assert_eq!(settings.theme, Theme::Dark);
    assert_eq!(settings.max_concurrent_downloads, 7);
    assert!(!settings.notify_on_complete);
    let queues = api::list_queues(&m);
    let night_again = queues.iter().find(|q| q.name == "Night").unwrap();
    assert_eq!(night_again.max_concurrent, 1);
    let ids: Vec<_> = api::list_downloads(&m).into_iter().map(|r| r.id).collect();
    for id in [b, c, d] {
        assert!(
            ids.contains(&id),
            "download {} is missing after restart",
            id.0
        );
    }
    assert_eq!(row(&m, d).unwrap().status, DownloadStatus::Completed);
    let in_night: Vec<_> = api::list_downloads(&m)
        .into_iter()
        .filter(|r| r.queue == night_again.id)
        .map(|r| r.id)
        .collect();
    assert_eq!(in_night, [c, b]);
    m.shutdown().await;
}

struct NoShell;

impl Shell for NoShell {
    fn open_window(&self) {}
    fn quit(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_right_after_start_opens_then_stops_the_manager() {
    let sandbox = Sandbox::new();
    let core = Core::new(sandbox.config());
    assert!(core.get().is_none(), "not opened yet");
    let exited = std::sync::atomic::AtomicBool::new(false);
    quit_core(Some(&core), || {
        let manager = core.get().expect("opened by the quit");
        let err = api::pause_all(manager).unwrap_err();
        assert!(err.contains("shutting down"), "{err}");
        exited.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .await;
    assert!(exited.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_stops_the_manager_before_exiting() {
    let sandbox = Sandbox::new();
    let slow = TestServer::start(SLOW_LEN, Some(SLOW_BPS)).await;
    let m = sandbox.open().await;
    let id = api::add_download(&m, request(slow.url("q.bin"), &sandbox.downloads(), true))
        .await
        .unwrap();
    wait_for(&m, id, "running", running).await;

    let exited = std::sync::atomic::AtomicBool::new(false);
    quit_sequence(Some(&m), || {
        // By the time the process exits, the manager has stopped taking work.
        let err = api::pause_all(&m).unwrap_err();
        assert!(err.contains("shutting down"), "{err}");
        exited.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .await;
    assert!(exited.load(std::sync::atomic::Ordering::SeqCst));
    drop(m);

    // The running download continues on the next start.
    let m = sandbox.open().await;
    wait_for(&m, id, "running after the restart", running).await;
    m.shutdown().await;
}

/// WP12: the media and add-on commands reach the real core, not a stand-in. Nothing here
/// installs the add-on, so the test stays off the internet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_and_addon_commands_reach_the_core() {
    let sandbox = Sandbox::new();
    let m = sandbox.open().await;

    let addons = api::list_addons(&m);
    assert_eq!(addons.len(), 1);
    assert_eq!(addons[0].id, "media");
    assert_eq!(addons[0].state, AddonState::NotInstalled);
    assert!(
        addons[0].size.is_some(),
        "the download size is known before installing"
    );

    assert!(api::pending_media_choices(&m).is_empty());
    assert!(api::pending_addon_requests(&m).is_empty());
    let selection = MediaSelection {
        option_id: "best".into(),
        folder: None,
        file_name: None,
        queue: None,
        later: false,
    };
    let gone = api::choose_media(&m, 99, selection).await.unwrap_err();
    assert!(gone.contains("already confirmed or dismissed"), "{gone}");
    let gone = api::dismiss_media_choice(&m, 99).unwrap_err();
    assert!(gone.contains("already confirmed or dismissed"), "{gone}");

    let unknown = api::install_addon(&m, "flash").await.unwrap_err();
    assert!(unknown.contains("no add-on called"), "{unknown}");
    let unknown = api::update_addon(&m, "flash").await.unwrap_err();
    assert!(unknown.contains("no add-on called"), "{unknown}");
    // Removing one that is not installed is fine and leaves it not installed.
    api::remove_addon(&m, "media").await.unwrap();
    // Stopping with nothing in progress is fine; an unknown add-on is refused.
    api::cancel_addon_operation(&m, "media").await.unwrap();
    assert!(api::cancel_addon_operation(&m, "flash").await.is_err());
    // Nothing installed: the check has nothing to look at and says the add-on is not installed.
    let checked = api::check_addon_updates(&m).await.unwrap();
    assert_eq!(checked[0].state, AddonState::NotInstalled);
    assert_eq!(api::list_addons(&m)[0].state, AddonState::NotInstalled);

    // Opening looks the download up in the core by id.
    let missing = api::file_to_open(&m, DownloadId(4242), false).unwrap_err();
    assert!(missing.contains("no longer exists"), "{missing}");
    assert!(api::download_is_program(&m, DownloadId(4242)).is_err());
    m.shutdown().await;
}

/// D23: a download window finds its question, says when the file is already in the list, and
/// Download later adds it paused, answering with the new download's id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_window_finds_its_question_and_its_download() {
    let sandbox = Sandbox::new();
    let fast = TestServer::start(64 * 1024, None).await;
    let m = sandbox.open().await;
    assert!(
        api::get_settings(&m).extension.ask_before_download,
        "a window shows first on a fresh install"
    );
    let capture = |handover: &str| NewDownload {
        handover_id: handover.into(),
        url: fast.url("report.pdf"),
        referrer: None,
        page_url: Some("https://example.com/page".into()),
        suggested_name: None,
        mime: None,
        size_hint: Some(64 * 1024),
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    };
    m.add_capture(capture("first")).await.unwrap();
    let asked = api::pending_confirmations(&m).remove(0);
    let Some(Question::Capture(found)) = api::question(&m, asked.id) else {
        panic!("the window cannot find its question")
    };
    assert_eq!(found, asked);
    assert!(api::earlier_download(&m, asked.id).is_none());
    assert!(api::question(&m, 4242).is_none());

    let later = CaptureConfirmation {
        later: true,
        ..CaptureConfirmation::default()
    };
    let id = api::confirm_capture(&m, asked.id, later).await.unwrap();
    assert!(
        api::question(&m, asked.id).is_none(),
        "answered: never asked again"
    );
    let row = api::get_download(&m, id).unwrap();
    assert_eq!(row.status, DownloadStatus::Paused);

    m.add_capture(capture("second")).await.unwrap();
    let again = api::pending_confirmations(&m).remove(0);
    assert_eq!(api::earlier_download(&m, again.id).map(|r| r.id), Some(id));
    assert!(api::get_download(&m, DownloadId(4242)).is_err());
    m.shutdown().await;
}
