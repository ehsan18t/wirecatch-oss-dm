//! End-to-end scenarios (WP7): a scripted extension talks to the real
//! `wirecatch-bridge` executable, which relays to a real manager over a real
//! local endpoint, which downloads from a local HTTP server.
//!
//! Every test has its own temporary folder, endpoint and server, so they run in
//! parallel. See `crates/e2e/README.md` for how the bridge is built.

use std::time::{Duration, Instant};

use wirecatch_e2e::PATIENCE;
use wirecatch_e2e::app::{App, Sandbox, capture, run, wait_for_row};
use wirecatch_e2e::extension::{
    Caller, FakeExtension, Launch, added, hello_call, withdraw_call, withdrawn,
};
use wirecatch_e2e::server::{serve, serve_held};
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::app::{AddRequest, CaptureConfirmation, DownloadStatus};
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, ExtensionSettingsPatch, WithdrawOutcome,
};
use wirecatch_protocol::wire::{Call, ErrorCode, Event, Outcome};
use wirecatch_test_support::test_data;

const FILE_LEN: usize = 256 * 1024;
const COOKIE: &str = "session=e2e-cookie";

/// How long a reply that should not come yet is waited for.
const QUIET: Duration = Duration::from_millis(500);

/// A sandbox with the app running and an extension that has said hello.
async fn connected(sandbox: &Sandbox) -> (App, FakeExtension) {
    let app = App::start(sandbox).await;
    let mut ext = FakeExtension::chrome(&sandbox.endpoint);
    let hello = ext.hello().await;
    assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
    (app, ext)
}

fn no_rows_for(app: &App, url: &str) -> bool {
    app.manager.list_downloads().iter().all(|r| r.url != url)
}

/// Scenario 1: the reply is `started` only once the server has answered, the file
/// completes byte for byte, and the browser's cookie reached the server.
#[test]
fn a_capture_starts_after_the_server_answers_and_completes() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;
        let url = server.url("complete.bin").to_string();

        let request = ext
            .send(Call::AddDownload(capture(&url, "handover-complete")))
            .await;
        server.wait_for_request("/complete.bin", PATIENCE).await;
        assert!(
            ext.outcome_within(request, QUIET).await.is_none(),
            "the app replied before the server answered"
        );
        server.release();
        let reply = added(ext.outcome(request).await).expect("the capture was accepted");
        assert_eq!(reply.action, AddAction::Started);
        let id = reply.id.expect("a started capture has a download id");

        let row = wait_for_row(&app.manager, "the capture to complete", |r| {
            r.id == id && r.status == DownloadStatus::Completed
        })
        .await;
        let file = std::fs::read(&row.path).expect("the downloaded file exists");
        assert!(server.data == file, "the file differs from the server's");
        assert!(
            sandbox.part_files().is_empty(),
            "{:?}",
            sandbox.part_files()
        );

        let seen = server.seen_at("/complete.bin");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.header("cookie") == Some(COOKIE)));

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 2, as the extension does it after losing its connection: the
/// capture is withdrawn from a new bridge while the server still holds its
/// answer. Nothing is left behind.
#[test]
fn withdrawing_from_a_new_connection_before_the_server_answers_leaves_nothing() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;
        let url = server.url("withdraw.bin").to_string();

        let request = ext
            .send(Call::AddDownload(capture(&url, "handover-withdraw")))
            .await;
        server.wait_for_request("/withdraw.bin", PATIENCE).await;

        let mut reconnected = FakeExtension::chrome(&sandbox.endpoint);
        reconnected.hello().await;
        assert_eq!(
            reconnected.withdraw_download("handover-withdraw").await,
            Ok(WithdrawOutcome::Withdrawn)
        );
        let refused = added(ext.outcome(request).await).expect_err("the withdrawn capture failed");
        assert_eq!(refused.code, ErrorCode::NotStarted, "{refused:?}");

        assert!(
            no_rows_for(&app, &url),
            "{:#?}",
            app.manager.list_downloads()
        );
        assert!(
            sandbox.part_files().is_empty(),
            "{:?}",
            sandbox.part_files()
        );
        server.release();

        assert!(reconnected.close().await.success());
        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 2 on one connection: the withdraw is sent while `add_download` is
/// still waiting for the server.
#[test]
fn withdrawing_on_the_same_connection_before_the_server_answers_leaves_nothing() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;
        let url = server.url("withdraw.bin").to_string();

        let request = ext
            .send(Call::AddDownload(capture(&url, "handover-withdraw")))
            .await;
        server.wait_for_request("/withdraw.bin", PATIENCE).await;

        assert_eq!(
            ext.withdraw_download("handover-withdraw").await,
            Ok(WithdrawOutcome::Withdrawn)
        );
        let refused = added(ext.outcome(request).await).expect_err("the withdrawn capture failed");
        assert_eq!(refused.code, ErrorCode::NotStarted, "{refused:?}");

        assert!(
            no_rows_for(&app, &url),
            "{:#?}",
            app.manager.list_downloads()
        );
        assert!(
            sandbox.part_files().is_empty(),
            "{:?}",
            sandbox.part_files()
        );
        server.release();

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 2, the other side: once the reply was `started`, the download is the
/// app's and a withdrawal says so.
#[test]
fn withdrawing_after_started_is_already_started() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve(test_data(FILE_LEN)).await;
        let url = server.url("started.bin").to_string();

        let reply = ext
            .add_download(capture(&url, "handover-started"))
            .await
            .expect("the capture was accepted");
        assert_eq!(reply.action, AddAction::Started);
        assert_eq!(
            ext.withdraw_download("handover-started").await,
            Ok(WithdrawOutcome::AlreadyStarted)
        );
        let id = reply.id.expect("a started capture has a download id");
        wait_for_row(&app.manager, "the capture to complete", |r| {
            r.id == id && r.status == DownloadStatus::Completed
        })
        .await;

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 3: the same handover id sent twice (back to back, before the first
/// is answered) and once more later adds one download, and every answer is the
/// first one.
#[test]
fn a_repeated_handover_id_adds_one_download() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;
        let url = server.url("twice.bin").to_string();

        let first = ext
            .send(Call::AddDownload(capture(&url, "handover-twice")))
            .await;
        let second = ext
            .send(Call::AddDownload(capture(&url, "handover-twice")))
            .await;
        server.wait_for_request("/twice.bin", PATIENCE).await;
        server.release();
        let first = added(ext.outcome(first).await).expect("the capture was accepted");
        let second = added(ext.outcome(second).await).expect("the repeat was answered");
        assert_eq!(first.action, AddAction::Started);
        assert_eq!(second, first);

        let id = first.id.expect("a started capture has a download id");
        wait_for_row(&app.manager, "the capture to complete", |r| {
            r.id == id && r.status == DownloadStatus::Completed
        })
        .await;
        let third = ext
            .add_download(capture(&url, "handover-twice"))
            .await
            .expect("the late repeat was answered");
        assert_eq!(third, first);
        assert_eq!(app.manager.list_downloads().len(), 1);

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 4: a change from the extension is the app's setting at once and
/// reaches the other browser; a change in the app reaches both browsers.
#[test]
fn extension_settings_sync_both_ways() {
    run(|sandbox| async move {
        let (app, mut chrome) = connected(&sandbox).await;
        let mut firefox = FakeExtension::firefox(&sandbox.endpoint);
        firefox.hello().await;

        assert_eq!(
            chrome.get_extension_settings().await,
            app.manager.extension_settings()
        );

        let changed = chrome
            .update_extension_settings(ExtensionSettingsPatch {
                show_media_button: Some(false),
                add_ignored_sites: Some(vec!["Ignored.Example".into()]),
                min_capture_size: Some(4096),
                ..ExtensionSettingsPatch::default()
            })
            .await;
        assert!(!changed.show_media_button);
        assert!(
            changed
                .ignored_sites
                .contains(&"ignored.example".to_string())
        );
        assert_eq!(changed.min_capture_size, 4096);
        assert_eq!(app.manager.get_settings().extension, changed);
        firefox
            .event("extension_settings_changed from Chrome", |e| match e {
                Event::ExtensionSettingsChanged(s) if *s == changed => Some(()),
                _ => None,
            })
            .await;

        let mut settings = app.manager.get_settings();
        settings.extension.capture_enabled = false;
        settings.extension.capture_extensions = vec!["zip".into()];
        app.manager
            .update_settings(settings)
            .expect("the app saved the change");
        let from_app = app.manager.extension_settings();
        assert!(!from_app.capture_enabled);
        for ext in [&mut chrome, &mut firefox] {
            ext.event("extension_settings_changed from the app", |e| match e {
                Event::ExtensionSettingsChanged(s) if *s == from_app => Some(()),
                _ => None,
            })
            .await;
            assert_eq!(ext.get_extension_settings().await, from_app);
        }

        assert!(firefox.close().await.success());
        assert!(chrome.close().await.success());
        app.quit().await;
    });
}

/// The popup's other calls: the queue list matches the app's, and pause all and
/// resume all act on a running download.
#[test]
fn popup_controls_reach_the_app() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let queues = ext.list_queues().await;
        let app_queues = app.manager.list_queues();
        assert_eq!(queues.len(), app_queues.len());
        assert!(queues.iter().any(|q| q.is_default));
        for (summary, queue) in queues.iter().zip(&app_queues) {
            assert_eq!((summary.id, &summary.name), (queue.id, &queue.name));
        }

        let server = serve_held(test_data(FILE_LEN)).await;
        let id = app
            .manager
            .add_download(AddRequest {
                url: server.url("popup.bin").to_string().into(),
                queue: None,
                folder: None,
                file_name: None,
                start: true,
            })
            .await
            .expect("the download was added");
        server.wait_for_request("/popup.bin", PATIENCE).await;
        ext.pause_all().await;
        wait_for_row(&app.manager, "the download to pause", |r| {
            r.id == id && r.status == DownloadStatus::Paused
        })
        .await;
        server.release();
        ext.resume_all().await;
        wait_for_row(&app.manager, "the download to complete", |r| {
            r.id == id && r.status == DownloadStatus::Completed
        })
        .await;

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 5: a capture waiting on a server that does not answer must not hold
/// up the next request on the same connection.
#[test]
fn a_capture_waiting_on_a_silent_server_does_not_delay_other_calls() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;

        let capture_request = ext
            .send(Call::AddDownload(capture(
                server.url("silent.bin").as_str(),
                "handover-silent",
            )))
            .await;
        server.wait_for_request("/silent.bin", PATIENCE).await;
        let asked = Instant::now();
        let list = ext.send(Call::ListQueues).await;
        let answer = ext.outcome_within(list, Duration::from_secs(1)).await;
        assert!(
            matches!(answer, Some(Outcome::Ok(_))),
            "list_queues was not answered within a second while a capture waited (got {answer:?} after {:?})",
            asked.elapsed()
        );

        server.release();
        let reply = added(ext.outcome(capture_request).await).expect("the capture was accepted");
        assert_eq!(reply.action, AddAction::Started);

        assert!(ext.close().await.success());
        app.quit().await;
    });
}

/// Scenario 6: the app quits while a request waits; the bridge answers it with
/// `app_unavailable` and exits, closing stdout so the extension reconnects.
#[test]
fn the_app_quitting_mid_session_closes_the_bridge() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve_held(test_data(FILE_LEN)).await;

        let request = ext
            .send(Call::AddDownload(capture(
                server.url("quit.bin").as_str(),
                "handover-quit",
            )))
            .await;
        server.wait_for_request("/quit.bin", PATIENCE).await;
        app.quit().await;

        let mut answers = ext.wait_stdout_closed().await;
        match answers.remove(&request) {
            Some(Outcome::Err(e)) => assert_eq!(e.code, ErrorCode::AppUnavailable, "{e:?}"),
            other => panic!("the waiting request got {other:?}"),
        }
        assert!(answers.is_empty(), "{answers:?}");
        let status = ext.wait_exit().await;
        assert!(status.success(), "{status}\n{}", ext.stderr());
    });
}

/// Scenario 7: nothing listens, so the bridge starts the app (here a stand-in
/// running the same manager), and the first request succeeds within 5 seconds.
#[test]
fn the_bridge_starts_the_app_when_it_is_not_running() {
    run(|sandbox| async move {
        let launch = Launch::Program(vec![
            env!("CARGO_BIN_EXE_wirecatch-e2e-app").into(),
            "--background".into(),
            "--endpoint".into(),
            sandbox.endpoint.as_os_str().into(),
            "--root".into(),
            sandbox.path().into(),
        ]);
        let mut ext = FakeExtension::start(&sandbox.endpoint, &Caller::Chrome, launch);

        let asked = Instant::now();
        let hello = ext.hello().await;
        let took = asked.elapsed();
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert!(took < Duration::from_secs(5), "the app took {took:?}");
        assert!(ext.list_queues().await.iter().any(|q| q.is_default));
        assert!(ext.close().await.success());

        // The stand-in exits once its client is gone; wait so nothing outlives the test.
        let exited = sandbox.path().join("app-exited");
        let deadline = Instant::now() + PATIENCE;
        while !exited.exists() {
            assert!(Instant::now() < deadline, "the started app did not exit");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
}

/// Takes the outcome of writing to a bridge that refuses its caller. It may already have exited, so the write may fail; what the test checks is that nothing comes back.
fn bridge_may_have_exited(sent: &std::io::Result<u64>) {
    let (Ok(_) | Err(_)) = sent;
}

/// Scenario 8: a caller that is not our extension is refused before anything
/// reaches the app.
#[test]
fn an_unknown_caller_is_refused_and_nothing_reaches_the_app() {
    run(|sandbox| async move {
        let app = App::start(&sandbox).await;
        let strangers = [
            Caller::ChromeOrigin("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".into()),
            Caller::ChromeOrigin("https://example.com/".into()),
        ];
        for caller in strangers {
            let mut ext = FakeExtension::start(&sandbox.endpoint, &caller, Launch::Nothing);
            bridge_may_have_exited(&ext.try_send(hello_call()).await);
            bridge_may_have_exited(&ext.try_send(withdraw_call("handover-stranger")).await);
            let answers = ext.wait_stdout_closed().await;
            assert!(answers.is_empty(), "{caller:?} got {answers:?}");
            let status = ext.wait_exit().await;
            assert!(!status.success(), "{caller:?} was served");
            assert!(
                ext.stderr().contains("refused to start"),
                "{}",
                ext.stderr()
            );
        }
        assert_eq!(app.accepted(), 0, "a refused caller reached the app");
        app.quit().await;
    });
}

/// Scenario 9: with "ask before each download" on, the reply is
/// `awaiting_confirmation` and the link is not tried; confirming in the app
/// downloads it, rejecting leaves nothing.
#[test]
fn ask_before_download_waits_for_the_user() {
    run(|sandbox| async move {
        let (app, mut ext) = connected(&sandbox).await;
        let server = serve(test_data(FILE_LEN)).await;
        ext.update_extension_settings(ExtensionSettingsPatch {
            ask_before_download: Some(true),
            ..ExtensionSettingsPatch::default()
        })
        .await;
        assert!(app.manager.get_settings().extension.ask_before_download);
        let waiting = AddedDownload {
            id: None,
            action: AddAction::AwaitingConfirmation,
        };

        // Confirmed: downloaded with the browser's cookie.
        let url = server.url("confirm.bin").to_string();
        let reply = ext.add_download(capture(&url, "handover-confirm")).await;
        assert_eq!(reply, Ok(waiting.clone()));
        assert!(
            server.arrived("/confirm.bin") == 0,
            "the link was tried before confirming"
        );
        let pending = app.manager.pending_confirmations();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].url, url);
        app.manager
            .confirm_capture(pending[0].id, CaptureConfirmation::default())
            .await
            .expect("the capture was confirmed");
        let row = wait_for_row(&app.manager, "the confirmed capture to complete", |r| {
            r.url == url && r.status == DownloadStatus::Completed
        })
        .await;
        let file = std::fs::read(&row.path).expect("the downloaded file exists");
        assert!(server.data == file, "the file differs from the server's");
        let seen = server.seen_at("/confirm.bin");
        assert!(!seen.is_empty() && seen.iter().all(|s| s.header("cookie") == Some(COOKIE)));

        // Rejected: nothing downloaded, nothing kept.
        let url = server.url("reject.bin").to_string();
        let reply = ext.add_download(capture(&url, "handover-reject")).await;
        assert_eq!(reply, Ok(waiting));
        let pending = app.manager.pending_confirmations();
        let rejected = pending
            .iter()
            .find(|p| p.url == url)
            .expect("the capture waits for the user");
        app.manager
            .reject_capture(rejected.id)
            .expect("the capture was rejected");
        assert_eq!(app.manager.pending_confirmations(), []);
        assert!(
            no_rows_for(&app, &url),
            "{:#?}",
            app.manager.list_downloads()
        );
        assert_eq!(server.arrived("/reject.bin"), 0);
        assert!(
            sandbox.part_files().is_empty(),
            "{:?}",
            sandbox.part_files()
        );
        let id = ext.send(withdraw_call("handover-reject")).await;
        assert_eq!(
            withdrawn(ext.outcome(id).await),
            Ok(WithdrawOutcome::NotFound)
        );

        assert!(ext.close().await.success());
        app.quit().await;
    });
}
