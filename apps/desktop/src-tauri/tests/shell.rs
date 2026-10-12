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
use wirecatch_desktop_lib::download_windows::DownloadWindows;
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::lifecycle::Lifecycle;
use wirecatch_desktop_lib::window::MAIN_WINDOW;
use wirecatch_desktop_lib::{EVENT_NAME, RESYNC_EVENT, tray};
use wirecatch_protocol::app::{Category, PendingCapture, UiEvent, UserError};
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_test_support::TestResult;

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
        .manage(ShownNotifications::default());
    if let Some(core) = core {
        builder = builder.manage(core);
    }
    Ok(builder.build(context)?)
}

/// How long a wait may take before the test fails.
const PATIENCE: Duration = Duration::from_secs(30);
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
    handle.listen_any(EVENT_NAME, move |e| {
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
    let (title, body) = handle.state::<ShownNotifications>().0.lock().unwrap()[0].clone();
    assert!(title.contains("could not save"));
    assert_eq!(body, "The disk is full.");

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
    sender.send(UiEvent::ConfirmationClosed { id: 1 }).unwrap();
    wait_until("the events to be handled", || closed.load(Ordering::SeqCst));
    assert_eq!(
        handle.state::<ShownNotifications>().0.lock().unwrap().len(),
        1
    );
    assert_eq!(handle.state::<DownloadWindows>().snapshot().len(), 1);
    task.abort();
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
                id,
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
        handle.state::<DownloadWindows>().waiting() == [6]
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
    handle.listen_any(EVENT_NAME, move |e| {
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

#[test]
fn downloads_finishing_together_share_one_notification() -> TestResult {
    let app = app_with(None)?;
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
    Ok(())
}
