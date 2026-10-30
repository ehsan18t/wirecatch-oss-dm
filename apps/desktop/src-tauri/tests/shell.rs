//! The desktop shell on Tauri's mock runtime: the tray's Quit, quitting before the download
//! list opened, forwarding core events (and what happens with no window), start-at-sign-in, and
//! how finished and failed downloads are told (D52).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::Sandbox;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::utils::config::WindowConfig;
use tauri::{App, Listener as _, Manager as _};
use tokio::sync::broadcast;
use wirecatch_desktop_lib::activity::Ended;
use wirecatch_desktop_lib::api;
use wirecatch_desktop_lib::autostart::{Action, EntryState};
use wirecatch_desktop_lib::background::{
    Launcher, NOTIFY_BATCH, Tell, Teller, apply_autostart, forward, show_notification, tell,
};
use wirecatch_desktop_lib::browser_notices::{self, BrowserNotices};
use wirecatch_desktop_lib::download_windows::{DownloadWindows, Shows, may_act_on};
use wirecatch_desktop_lib::finished_window::{self, FinishedWindow};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::lifecycle::Lifecycle;
use wirecatch_desktop_lib::notifications::Lasts;
use wirecatch_desktop_lib::system_notifications::ShownNotifications;
use wirecatch_desktop_lib::window::MAIN_WINDOW;
use wirecatch_desktop_lib::{RESYNC_EVENT, UI_EVENT, tray};
use wirecatch_protocol::app::{
    Category, DownloadStatus, DownloadSummary, NotifyOnFinish, PendingCapture, Queue, QueueNotify,
    QueueReport, QueueSettings, QueueStatus, UiEvent, UserError,
};
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};
use wirecatch_test_support::{PATIENCE, TestResult};

fn app_with(core: Option<Core>) -> TestResult<App<MockRuntime>> {
    let mut context = mock_context(noop_assets());
    let main: WindowConfig = serde_json::from_value(serde_json::json!({
        "label": MAIN_WINDOW,
        "create": false,
        "title": "Wirecatch"
    }))?;
    context.config_mut().app.windows = vec![main];
    let mut builder = mock_builder()
        .manage(Lifecycle::default())
        .manage(DownloadWindows::default())
        .manage(FinishedWindow::default())
        .manage(ShownNotifications::default())
        .manage(BrowserNotices::default());
    if let Some(core) = core {
        builder = builder.manage(core);
    }
    Ok(builder.build(context)?)
}

/// How often a wait looks again: the mock runtime's windows and notifications offer nothing
/// to wait on.
const POLL: Duration = Duration::from_millis(20);

fn wait_until(what: &str, done: impl Fn() -> bool) {
    tauri::async_runtime::block_on(async {
        let deadline = Instant::now() + PATIENCE;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting: {what}");
            tokio::time::sleep(POLL).await;
        }
    });
}

/// Whether `forward` has passed on an event whose payload contains `name`, which it does
/// only after handling the event and every one before it.
fn passed_on(handle: &tauri::AppHandle<MockRuntime>, name: &'static str) -> Arc<AtomicBool> {
    let seen = Arc::new(AtomicBool::new(false));
    let flag = seen.clone();
    handle.listen_any(UI_EVENT, move |e| {
        if e.payload().contains(name) {
            flag.store(true, Ordering::SeqCst);
        }
    });
    seen
}

/// Quit from the tray menu on an app whose manager `open_first` decides is open or not yet.
fn quit_from_the_tray(open_first: bool) -> TestResult {
    let sandbox = Sandbox::new()?;
    let app = app_with(Some(Core::new(sandbox.config()?)))?;
    let handle = app.handle().clone();
    if open_first {
        tauri::async_runtime::block_on(async {
            handle.state::<Core>().manager().await.map(|_| ())
        })?;
    }
    let exited = Arc::new(AtomicBool::new(false));
    let flag = exited.clone();
    handle
        .state::<Lifecycle>()
        .on_exit(move || flag.store(true, Ordering::SeqCst));

    tray::on_menu(&handle, "quit");
    wait_until("the quit to finish", || exited.load(Ordering::SeqCst));
    assert!(handle.state::<Lifecycle>().has_finished());
    // The manager stopped before the exit: it takes no more work.
    let core = handle.state::<Core>();
    let manager = core
        .get()
        .ok_or("the quit did not open and stop the manager")?;
    let Err(err) = api::pause_all(manager) else {
        return Err("the stopped manager still took work".into());
    };
    assert!(err.message.contains("shutting down"), "{err}");
    Ok(())
}

#[test]
fn quit_in_the_tray_menu_stops_the_manager_then_exits() -> TestResult {
    quit_from_the_tray(true)
}

#[test]
fn quit_before_the_download_list_opened_still_stops_it_cleanly() -> TestResult {
    quit_from_the_tray(false)
}

fn capture() -> PendingCapture {
    PendingCapture {
        id: QuestionId(1),
        url: "https://example.com/a.zip".into(),
        file_name: "a.zip".into(),
        folder: "C:/Downloads".into(),
        type_folder: "C:/Downloads".into(),
        category: Category::Archive,
        size: None,
        queue: QueueId(1),
        queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
        page_url: None,
    }
}

#[test]
fn a_question_opens_its_own_window_and_a_storage_failure_notifies() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    let (sender, receiver) = broadcast::channel(16);
    let task = tauri::async_runtime::spawn(forward(handle.clone(), receiver));

    sender
        .send(UiEvent::StorageFailed(UserError {
            message: "The disk is full.".into(),
            fix: None,
        }))
        .unwrap();
    wait_until("the notification", || {
        !handle
            .state::<ShownNotifications>()
            .0
            .lock()
            .unwrap()
            .is_empty()
    });
    assert!(handle.get_webview_window(MAIN_WINDOW).is_none());
    let shown = handle.state::<ShownNotifications>().0.lock().unwrap()[0].clone();
    // D64: what the person must know shows during Do Not Disturb by default.
    assert!(shown.through_do_not_disturb);
    let notification = shown.notification;
    assert!(notification.title.contains("could not save"));
    assert_eq!(notification.body, "The disk is full.");

    // A question opens its own download window (D23), not the main window.
    sender
        .send(UiEvent::ConfirmationRequested(capture()))
        .unwrap();
    wait_until("the window to open for the question", || {
        handle.get_webview_window("download-q1").is_some()
    });
    assert!(handle.get_webview_window(MAIN_WINDOW).is_none());
    // Its next steps reuse the window instead of opening another.
    sender
        .send(UiEvent::ConfirmationRequested(capture()))
        .unwrap();
    // With the main window open, it shows a storage failure itself: no notification.
    wirecatch_desktop_lib::window::show_main_window(&handle);
    let closed = passed_on(&handle, "confirmation_closed");
    sender
        .send(UiEvent::StorageFailed(UserError {
            message: "Still full.".into(),
            fix: None,
        }))
        .unwrap();
    sender
        .send(UiEvent::ConfirmationClosed { id: QuestionId(1) })
        .unwrap();
    wait_until("the events to be handled", || closed.load(Ordering::SeqCst));
    assert_eq!(
        handle.state::<ShownNotifications>().0.lock().unwrap().len(),
        1
    );
    assert_eq!(handle.state::<DownloadWindows>().snapshot().len(), 1);
    task.abort();
    Ok(())
}

/// D63: a browser download that started without a window of its own is told in a brief
/// notification and brings no window up, whether the main window is closed or open.
#[test]
fn a_download_started_without_asking_is_told_briefly_and_opens_no_window() -> TestResult {
    let sandbox = Sandbox::new()?;
    let app = app_with(Some(Core::new(sandbox.config()?)))?;
    tauri::async_runtime::block_on(app.state::<Core>().manager())?;
    let handle = app.handle().clone();
    let (sender, receiver) = broadcast::channel(16);
    let task = tauri::async_runtime::spawn(forward(handle.clone(), receiver));
    let started = ended_row(7, 1, DownloadStatus::Downloading);

    sender.send(UiEvent::Captured(started.clone())).unwrap();
    wait_until("the notification", || titles(&handle).len() == 1);
    assert!(handle.get_webview_window(MAIN_WINDOW).is_none());
    let shown = handle.state::<ShownNotifications>().0.lock().unwrap()[0].clone();
    assert!(
        !shown.through_do_not_disturb,
        "news waits out Do Not Disturb"
    );
    let notification = shown.notification;
    assert_eq!(notification.lasts, Lasts::Brief(Duration::from_secs(3)));

    wirecatch_desktop_lib::window::show_main_window(&handle);
    sender.send(UiEvent::Captured(started)).unwrap();
    wait_until("the second notification", || titles(&handle).len() == 2);
    task.abort();
    Ok(())
}

/// The next notice the app asked the browsers for.
fn notice_requested(
    events: &mut broadcast::Receiver<wirecatch_protocol::wire::Event>,
) -> TestResult<wirecatch_protocol::extension::BrowserNotice> {
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(PATIENCE, async {
            loop {
                if let wirecatch_protocol::wire::Event::NoticeRequested(notice) =
                    events.recv().await?
                {
                    return TestResult::Ok(notice);
                }
            }
        })
        .await?
    })
}

/// D65: with no browser to ask, a notice is the system notification at once; a browser that shows
/// it keeps the notification away, one that does not answer gets it after the wait, and pressing
/// a notice's button does what the notification's would.
#[test]
fn a_note_goes_to_the_browser_that_shows_it_or_to_a_notification() -> TestResult {
    let sandbox = Sandbox::new()?;
    let app = app_with(Some(Core::new(sandbox.config()?)))?;
    let handle = app.handle().clone();
    let manager = tauri::async_runtime::block_on(handle.state::<Core>().manager())?.clone();
    let in_browser = told_by(NotifyOnFinish::Browser);
    let finished = |id| Tell::Finished(vec![ended_row(id, 1, DownloadStatus::Completed)]);

    tell(&handle, &in_browser, &finished(1));
    assert_eq!(titles(&handle), ["Download finished"], "no browser listens");

    let mut browser = manager.subscribe_events();
    tell(&handle, &in_browser, &finished(2));
    let shown = notice_requested(&mut browser)?;
    assert_eq!(
        shown.kind,
        wirecatch_protocol::extension::NoticeKind::Finished
    );
    // The notification's buttons, Open included (D90).
    assert_eq!(shown.buttons, ["Open", "Show in folder"]);
    assert_eq!(shown.stays_ms, Some(8000));
    browser_notices::shown(&handle, shown.id);
    // Asked after the one shown, so its notification can only come after the other's would.
    tell(
        &handle,
        &in_browser,
        &Tell::Failed(vec![ended_row(3, 1, DownloadStatus::Failed)]),
    );
    let unanswered = notice_requested(&mut browser)?;
    assert_eq!(
        unanswered.stays_ms, None,
        "a failure stays until it is closed"
    );
    wait_until("the notification for the notice no browser showed", || {
        titles(&handle).len() == 2
    });
    assert_eq!(titles(&handle), ["Download finished", "Download failed"]);

    // Try again on the notice no browser answered in time, for a download the list does not have:
    // its buttons still work for a browser that showed it late, and what it did is told.
    browser_notices::pressed(
        &handle,
        &wirecatch_protocol::extension::NoticePressed {
            id: unanswered.id,
            button: Some(0),
        },
    );
    wait_until("the notification that it could not try again", || {
        titles(&handle).len() == 3
    });
    assert_eq!(titles(&handle)[2], "Wirecatch could not do that");
    Ok(())
}

/// D67: a video read without a window is told as added when the browser hands it over, by its
/// title, and not again when its download starts.
#[test]
fn a_video_is_told_as_added_once_when_handed_over() -> TestResult {
    let sandbox = Sandbox::new()?;
    let app = app_with(Some(Core::new(sandbox.config()?)))?;
    tauri::async_runtime::block_on(app.state::<Core>().manager())?;
    let handle = app.handle().clone();
    let (sender, receiver) = broadcast::channel(16);
    let task = tauri::async_runtime::spawn(forward(handle.clone(), receiver));

    sender
        .send(UiEvent::MediaCaptured {
            id: QuestionId(3),
            title: "Keynote".into(),
        })
        .unwrap();
    let mut video = ended_row(8, 1, DownloadStatus::Downloading);
    video.media = Some(wirecatch_protocol::app::MediaSummary {
        quality: "720p".into(),
        source: wirecatch_protocol::extension::MediaKind::Page,
        video_key: None,
        instead_of: None,
    });
    sender.send(UiEvent::Captured(video)).unwrap();
    // A file after it: its notice shows only once the events before it were handled.
    sender
        .send(UiEvent::Captured(ended_row(
            9,
            1,
            DownloadStatus::Downloading,
        )))
        .unwrap();
    wait_until("the file's notice", || titles(&handle).len() == 2);
    let bodies: Vec<String> = handle
        .state::<ShownNotifications>()
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.notification.body.clone())
        .collect();
    assert_eq!(bodies, ["Keynote: getting video details", "file-9.zip"]);
    assert_eq!(titles(&handle), ["Download added", "Download added"]);
    task.abort();
    Ok(())
}

/// D64: what the person must know asks to show during Do Not Disturb until they turn that off.
#[test]
fn important_notices_follow_the_do_not_disturb_switch() -> TestResult {
    let sandbox = Sandbox::new()?;
    let app = app_with(Some(Core::new(sandbox.config()?)))?;
    let handle = app.handle().clone();
    let manager = tauri::async_runtime::block_on(handle.state::<Core>().manager())?.clone();
    show_notification(&handle, "Before".into(), String::new());
    let mut settings = manager.get_settings();
    settings.notify_during_do_not_disturb = false;
    manager.update_settings(settings)?;
    show_notification(&handle, "After".into(), String::new());
    let through: Vec<bool> = handle
        .state::<ShownNotifications>()
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.through_do_not_disturb)
        .collect();
    assert_eq!(through, [true, false]);
    Ok(())
}

#[test]
fn five_questions_ask_at_once_and_the_sixth_waits_for_a_window_to_close() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    let (sender, receiver) = broadcast::channel(16);
    let task = tauri::async_runtime::spawn(forward(handle.clone(), receiver));
    for id in 1..=6 {
        sender
            .send(UiEvent::ConfirmationRequested(PendingCapture {
                id: QuestionId(id),
                ..capture()
            }))
            .unwrap();
    }
    wait_until("five windows", || {
        (1..=5).all(|id| {
            handle
                .get_webview_window(&format!("download-q{id}"))
                .is_some()
        })
    });
    wait_until("the sixth question to wait", || {
        handle.state::<DownloadWindows>().waiting() == [QuestionId(6)]
    });
    assert!(handle.get_webview_window("download-q6").is_none());

    // A window closing makes room for the next question.
    wirecatch_desktop_lib::download_windows::window_gone(&handle, "download-q2");
    wait_until("the sixth window", || {
        handle.get_webview_window("download-q6").is_some()
    });
    let slots: Vec<usize> = handle
        .state::<DownloadWindows>()
        .snapshot()
        .iter()
        .map(|w| w.slot)
        .collect();
    assert!(
        slots.contains(&1),
        "the closed window's place is used again: {slots:?}"
    );
    task.abort();
    Ok(())
}

#[test]
fn with_a_window_events_reach_it_and_missed_events_ask_for_a_reload() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    wirecatch_desktop_lib::window::show_main_window(&handle);
    assert!(handle.get_webview_window(MAIN_WINDOW).is_some());

    let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = events.clone();
    handle.listen_any(UI_EVENT, move |e| {
        seen.lock().unwrap().push(e.payload().to_owned());
    });
    let resyncs = Arc::new(AtomicBool::new(false));
    let resynced = resyncs.clone();
    handle.listen_any(RESYNC_EVENT, move |_| {
        resynced.store(true, Ordering::SeqCst);
    });

    // A tiny channel, filled before the forwarder runs, so it falls behind.
    let (sender, receiver) = broadcast::channel(2);
    for id in 1..=4 {
        sender
            .send(UiEvent::DownloadsRemoved(vec![DownloadId(id)]))
            .unwrap();
    }
    let task = tauri::async_runtime::spawn(forward(handle.clone(), receiver));
    wait_until("the reload request", || resyncs.load(Ordering::SeqCst));
    wait_until("the events after it", || events.lock().unwrap().len() == 2);
    assert!(events.lock().unwrap()[0].contains("downloads_removed"));
    task.abort();
    Ok(())
}

struct FakeLauncher {
    calls: std::sync::Mutex<Vec<&'static str>>,
}

impl Launcher for FakeLauncher {
    fn enable(&self) -> Result<(), String> {
        self.calls.lock().map_err(|e| e.to_string())?.push("enable");
        Ok(())
    }
    fn disable(&self) -> Result<(), String> {
        self.calls
            .lock()
            .map_err(|e| e.to_string())?
            .push("disable");
        Ok(())
    }
}

#[test]
fn start_at_sign_in_follows_the_setting_and_choices_made_elsewhere() -> TestResult {
    tauri::async_runtime::block_on(async {
        let sandbox = Sandbox::new()?;
        let manager = sandbox.open().await?;
        let launcher = FakeLauncher {
            calls: std::sync::Mutex::new(Vec::new()),
        };

        // On at start: the entry is written again, so a moved copy starts from the right place.
        let done = apply_autostart(&launcher, &manager, EntryState::On, true, false).unwrap();
        assert_eq!(done, Action::Write);

        // Turned off in Task Manager: the setting follows instead of re-enabling the entry.
        let mut settings = manager.get_settings();
        settings.start_with_system = true;
        manager.update_settings(settings).unwrap();
        let done =
            apply_autostart(&launcher, &manager, EntryState::TurnedOff, true, false).unwrap();
        assert_eq!(done, Action::TurnSettingOff);
        assert!(!manager.get_settings().start_with_system);

        // Switched off in Wirecatch: the entry goes.
        apply_autostart(&launcher, &manager, EntryState::On, false, false).unwrap();
        assert_eq!(*launcher.calls.lock().unwrap(), ["enable", "disable"]);
        manager.shutdown().await;
        Ok(())
    })
}

fn ended_row(id: i64, queue: i64, status: DownloadStatus) -> DownloadSummary {
    DownloadSummary {
        id: DownloadId(id),
        url: format!("https://example.com/file-{id}.zip"),
        file_name: format!("file-{id}.zip"),
        path: format!("C:/Downloads/file-{id}.zip"),
        queue: QueueId(queue),
        queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
        category: Category::Archive,
        status,
        total: Some(100),
        done: 100,
        speed: 0,
        connections: 0,
        eta_secs: None,
        error: None,
        waiting: None,
        media: None,
        added_at: 0,
        completed_at: None,
        processing: None,
        proxy: None,
    }
}

fn queue_telling(id: i64, notify: QueueNotify) -> Queue {
    Queue {
        id: QueueId(id),
        is_default: id == 1,
        settings: QueueSettings {
            notify,
            ..QueueSettings::new(format!("Queue {id}"))
        },
        status: QueueStatus::default(),
    }
}

/// The settings that tell finished downloads `how`.
fn told_by(how: NotifyOnFinish) -> wirecatch_protocol::app::AppSettings {
    let mut settings = wirecatch_core::default_settings();
    settings.notify_on_finish = how;
    settings
}

fn titles(handle: &tauri::AppHandle<MockRuntime>) -> Vec<String> {
    handle
        .state::<ShownNotifications>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|n| n.notification.title.clone())
        .collect()
}

#[test]
fn downloads_finishing_together_share_one_notification() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    let mut teller = Teller::default();
    teller.queues(&[queue_telling(1, QueueNotify::EachDownload)]);
    let start = tokio::time::Instant::now();
    teller.ended(
        Ended {
            completed: (1..=3)
                .map(|n| ended_row(n, 1, DownloadStatus::Completed))
                .collect(),
            failed: Vec::new(),
        },
        start,
    );
    assert_eq!(teller.due(), Some(start + NOTIFY_BATCH));
    assert!(teller.take_due(start).is_empty(), "not due yet");
    for due in teller.take_due(start + NOTIFY_BATCH) {
        tell(&handle, &told_by(NotifyOnFinish::Notification), &due);
    }
    assert_eq!(titles(&handle), ["3 downloads finished"]);
    assert_eq!(teller.due(), None);
    Ok(())
}

#[test]
fn a_queue_that_tells_once_done_waits_for_its_report_but_failures_are_told_at_once() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    let mut teller = Teller::default();
    teller.queues(&[queue_telling(2, QueueNotify::WhenDone)]);
    let start = tokio::time::Instant::now();
    teller.ended(
        Ended {
            completed: vec![ended_row(1, 2, DownloadStatus::Completed)],
            failed: vec![ended_row(2, 2, DownloadStatus::Failed)],
        },
        start,
    );
    let due = teller.take_due(start + NOTIFY_BATCH);
    assert!(
        matches!(due.as_slice(), [Tell::Failed(rows)] if rows.len() == 1),
        "{due:?}"
    );
    for due in &due {
        tell(&handle, &told_by(NotifyOnFinish::Notification), due);
    }
    let report = QueueReport {
        queue: QueueId(2),
        name: "Night".into(),
        completed: vec![
            ended_row(1, 2, DownloadStatus::Completed),
            ended_row(3, 2, DownloadStatus::Completed),
        ],
        failed: vec![ended_row(2, 2, DownloadStatus::Failed)],
    };
    let told = teller
        .queue_finished(&report)
        .ok_or("the queue's report was not told")?;
    tell(&handle, &told_by(NotifyOnFinish::Notification), &told);
    assert_eq!(
        titles(&handle),
        [
            "Download failed",
            "Night: 2 downloads finished and 1 failed"
        ]
    );

    // A queue telling each download says nothing more once it is done.
    teller.queues(&[queue_telling(2, QueueNotify::EachDownload)]);
    assert_eq!(teller.queue_finished(&report), None);
    Ok(())
}

#[test]
fn the_complete_window_lists_what_ended_and_acts_only_on_that() -> TestResult {
    let app = app_with(None)?;
    let handle = app.handle().clone();
    tell(
        &handle,
        &told_by(NotifyOnFinish::Nothing),
        &Tell::Finished(vec![ended_row(9, 1, DownloadStatus::Completed)]),
    );
    assert_eq!(titles(&handle), Vec::<String>::new());
    assert!(
        handle.state::<FinishedWindow>().list().groups.is_empty(),
        "nothing means nothing"
    );

    tell(
        &handle,
        &told_by(NotifyOnFinish::Window),
        &Tell::Finished(vec![
            ended_row(1, 1, DownloadStatus::Completed),
            ended_row(2, 1, DownloadStatus::Completed),
        ]),
    );
    wait_until("the download complete window to open", || {
        handle.get_webview_window(finished_window::LABEL).is_some()
    });
    assert!(
        titles(&handle).is_empty(),
        "the window instead of a notification"
    );
    let listed: Vec<_> = handle.state::<FinishedWindow>().list().groups[0]
        .downloads
        .iter()
        .map(|d| d.id.0)
        .collect();
    assert_eq!(listed, [2, 1]);
    assert!(may_act_on(
        &handle,
        finished_window::LABEL,
        Shows::Download(DownloadId(1))
    ));
    assert!(!may_act_on(
        &handle,
        finished_window::LABEL,
        Shows::Download(DownloadId(9))
    ));
    assert!(
        !may_act_on(&handle, "download-d1", Shows::Download(DownloadId(1))),
        "another download window"
    );

    finished_window::gone(&handle, finished_window::LABEL);
    assert!(!may_act_on(
        &handle,
        finished_window::LABEL,
        Shows::Download(DownloadId(1))
    ));
    Ok(())
}
