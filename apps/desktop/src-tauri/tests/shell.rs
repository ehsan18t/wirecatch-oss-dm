//! The desktop shell on Tauri's mock runtime: the tray's Quit, quitting before the download
//! list opened, forwarding core events (and what happens with no window), start-at-sign-in and
//! completion notifications.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::Sandbox;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::utils::config::WindowConfig;
use tauri::{App, Listener as _, Manager as _};
use tokio::sync::broadcast;
use wirecatch_desktop_lib::activity::Finished;
use wirecatch_desktop_lib::api;
use wirecatch_desktop_lib::autostart::{Action, EntryState};
use wirecatch_desktop_lib::background::{
    Launcher, ShownNotifications, apply_autostart, forward, notify,
};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::lifecycle::Lifecycle;
use wirecatch_desktop_lib::questions::Questions;
use wirecatch_desktop_lib::window::MAIN_WINDOW;
use wirecatch_desktop_lib::{EVENT_NAME, RESYNC_EVENT, tray};
use wirecatch_protocol::app::{Category, PendingCapture, UiEvent, UserError};
use wirecatch_protocol::{DownloadId, QueueId};

fn app_with(core: Option<Core>) -> App<MockRuntime> {
    let mut context = mock_context(noop_assets());
    let main: WindowConfig = serde_json::from_value(serde_json::json!({
        "label": MAIN_WINDOW,
        "create": false,
        "title": "Wirecatch"
    }))
    .unwrap();
    context.config_mut().app.windows = vec![main];
    let mut builder = mock_builder()
        .manage(Lifecycle::default())
        .manage(Questions::default())
        .manage(ShownNotifications::default());
    if let Some(core) = core {
        builder = builder.manage(core);
    }
    builder.build(context).unwrap()
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Quit from the tray menu on an app whose manager `open_first` decides is open or not yet.
fn quit_from_the_tray(open_first: bool) {
    let sandbox = Sandbox::new();
    let app = app_with(Some(Core::new(sandbox.config())));
    let handle = app.handle().clone();
    if open_first {
        tauri::async_runtime::block_on(async {
            handle.state::<Core>().manager().await.map(|_| ()).unwrap()
        });
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
    let manager = core.get().expect("the quit opened and stopped the manager");
    let err = api::pause_all(manager).unwrap_err();
    assert!(err.contains("shutting down"), "{err}");
}

#[test]
fn quit_in_the_tray_menu_stops_the_manager_then_exits() {
    quit_from_the_tray(true);
}

#[test]
fn quit_before_the_download_list_opened_still_stops_it_cleanly() {
    quit_from_the_tray(false);
}

fn capture() -> PendingCapture {
    PendingCapture {
        id: 1,
        url: "https://example.com/a.zip".into(),
        file_name: "a.zip".into(),
        folder: "C:/Downloads".into(),
        category: Category::Archive,
        size: None,
        queue: QueueId(1),
        page_url: None,
    }
}

#[test]
fn with_no_window_a_question_opens_one_and_a_storage_failure_notifies() {
    let app = app_with(None);
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
    let (title, body) = handle.state::<ShownNotifications>().0.lock().unwrap()[0].clone();
    assert!(title.contains("could not save"));
    assert_eq!(body, "The disk is full.");

    sender
        .send(UiEvent::ConfirmationRequested(capture()))
        .unwrap();
    wait_until("the window to open for the question", || {
        handle.get_webview_window(MAIN_WINDOW).is_some()
    });
    task.abort();
}

#[test]
fn with_a_window_events_reach_it_and_missed_events_ask_for_a_reload() {
    let app = app_with(None);
    let handle = app.handle().clone();
    wirecatch_desktop_lib::window::show_main_window(&handle);
    assert!(handle.get_webview_window(MAIN_WINDOW).is_some());

    let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = events.clone();
    handle.listen_any(EVENT_NAME, move |e| {
        seen.lock().unwrap().push(e.payload().to_owned())
    });
    let resyncs = Arc::new(AtomicBool::new(false));
    let resynced = resyncs.clone();
    handle.listen_any(RESYNC_EVENT, move |_| {
        resynced.store(true, Ordering::SeqCst)
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
}

struct FakeLauncher {
    calls: std::sync::Mutex<Vec<&'static str>>,
}

impl Launcher for FakeLauncher {
    fn enable(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("enable");
        Ok(())
    }
    fn disable(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("disable");
        Ok(())
    }
}

#[test]
fn start_at_sign_in_follows_the_setting_and_choices_made_elsewhere() {
    tauri::async_runtime::block_on(async {
        let sandbox = Sandbox::new();
        let manager = sandbox.open().await;
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
    });
}

#[test]
fn downloads_finishing_together_share_one_notification() {
    let app = app_with(None);
    let handle = app.handle().clone();
    let finished: Vec<Finished> = (1..=3)
        .map(|n| Finished {
            id: DownloadId(n),
            file_name: format!("file-{n}.zip"),
        })
        .collect();
    notify(&handle, &finished);
    notify(&handle, &[]);
    let shown = handle
        .state::<ShownNotifications>()
        .0
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        shown,
        [(
            "3 downloads finished".to_string(),
            "file-1.zip and 2 more".to_string()
        )]
    );
}
