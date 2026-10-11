//! Work that runs for as long as the app does: forwarding core events to the webview, and
//! keeping the tray tooltip, notifications and start-with-system in step with the core.

use std::time::Duration;

use tauri::{AppHandle, Emitter, EventTarget, Manager as _, Runtime};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use wirecatch_core::Manager;
use wirecatch_protocol::app::{AppSettingsPatch, UiEvent};

use crate::activity::{Activity, Finished, notification_text};
use crate::autostart::{self, Action};
use crate::questions::{Attention, attention};
use crate::window::MAIN_WINDOW;
use crate::{EVENT_NAME, RESYNC_EVENT, download_windows, sleep, tray};

/// The tooltip changes at most this often.
pub const TOOLTIP_EVERY: Duration = Duration::from_secs(1);

/// Downloads finishing within this long of the first one share one notification.
pub const NOTIFY_BATCH: Duration = Duration::from_secs(3);

/// Sends every core event to every open window (the main window and the download windows) on
/// [`EVENT_NAME`], and opens a download window for each question the core asks (see
/// [`crate::questions`] and [`crate::download_windows`]), whatever the main window is doing. A
/// storage failure with no main window to show it becomes a system notification. With no
/// window open at all, events are not sent: a window loads everything fresh when it opens. If
/// this falls behind and misses events, the windows are told to reload on [`RESYNC_EVENT`] and
/// questions that missed their window get one.
pub async fn forward_events<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    forward(app, manager.subscribe()).await;
}

/// [`forward_events`] from any receiver of core events, until its sender is gone.
pub async fn forward<R: Runtime>(app: AppHandle<R>, mut events: broadcast::Receiver<UiEvent>) {
    loop {
        match events.recv().await {
            Ok(event) => {
                match attention(&event) {
                    Attention::None => {}
                    Attention::Ask(id) => download_windows::ask(&app, id),
                    Attention::Closed(id) => download_windows::question_closed(&app, id),
                    Attention::ShowMain => crate::window::show_main_window(&app),
                    Attention::Notify { title, body } => {
                        let unseen = app.get_webview_window(MAIN_WINDOW).is_none()
                            || sleep::skips_events(&app, MAIN_WINDOW);
                        if unseen {
                            show_notification(&app, title, body);
                        }
                    }
                }
                if app.webview_windows().is_empty() {
                    continue;
                }
                // A main window asleep in the tray gets nothing: it loads everything again
                // when it wakes.
                let asleep = sleep::skips_events(&app, crate::window::MAIN_WINDOW);
                let sent = if asleep {
                    app.emit_filter(EVENT_NAME, &event, |target| {
                        !matches!(target, EventTarget::WebviewWindow { label } if label == crate::window::MAIN_WINDOW)
                    })
                } else {
                    app.emit(EVENT_NAME, &event)
                };
                if let Err(e) = sent {
                    tracing::warn!("could not send an event to the windows: {e}");
                }
            }
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!("the windows missed {missed} events; asking them to reload");
                let _ = app.emit(RESYNC_EVENT, ());
                download_windows::restore(&app);
            }
            Err(RecvError::Closed) => return,
        }
    }
}

async fn at(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Keeps the tray tooltip, completion notifications and start-with-system up to date.
pub async fn watch<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    let mut events = manager.subscribe();
    let mut activity = Activity::default();
    activity.load(&manager.list_downloads());
    let mut settings = manager.get_settings();
    sync_autostart(&app, &manager, settings.start_with_system, false);

    let mut shown = String::new();
    let mut last_tooltip: Option<Instant> = None;
    let mut tooltip_stale = true;
    let mut finished: Vec<Finished> = Vec::new();
    let mut notify_at: Option<Instant> = None;

    loop {
        let tooltip_at =
            tooltip_stale.then(|| last_tooltip.map_or_else(Instant::now, |t| t + TOOLTIP_EVERY));
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let done = activity.apply(&event);
                    if !done.is_empty() {
                        finished.extend(done);
                        notify_at.get_or_insert_with(|| Instant::now() + NOTIFY_BATCH);
                    }
                    if let UiEvent::SettingsChanged(new) = event {
                        if new.start_with_system != settings.start_with_system {
                            sync_autostart(&app, &manager, new.start_with_system, new.start_with_system);
                        }
                        settings = new;
                    }
                    tooltip_stale = true;
                }
                Err(RecvError::Lagged(_)) => {
                    activity.load(&manager.list_downloads());
                    settings = manager.get_settings();
                    tooltip_stale = true;
                }
                Err(RecvError::Closed) => return,
            },
            () = at(tooltip_at) => {
                let text = activity.tooltip();
                if text != shown {
                    tray::set_tooltip(&app, &text);
                    shown = text;
                }
                last_tooltip = Some(Instant::now());
                tooltip_stale = false;
            }
            () = at(notify_at) => {
                if settings.notify_on_complete {
                    notify(&app, &finished);
                }
                finished.clear();
                notify_at = None;
            }
        }
    }
}

/// One notification for downloads that finished close together.
pub fn notify<R: Runtime>(app: &AppHandle<R>, finished: &[Finished]) {
    let Some((title, body)) = notification_text(finished) else {
        return;
    };
    show_notification(app, title, body);
}

/// Notifications shown, recorded when this is managed (tests do; the app does not).
#[derive(Default)]
pub struct ShownNotifications(pub std::sync::Mutex<Vec<(String, String)>>);

/// Shows a system notification. Does nothing more than log when the notification plugin is
/// missing.
pub fn show_notification<R: Runtime>(app: &AppHandle<R>, title: String, body: String) {
    if let Some(shown) = app.try_state::<ShownNotifications>() {
        shown
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((title.clone(), body.clone()));
    }
    if app
        .try_state::<tauri_plugin_notification::Notification<R>>()
        .is_none()
    {
        tracing::info!("notification (no notification support): {title}: {body}");
        return;
    }
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("could not show a notification: {e}");
    }
}

/// What changes the start-at-sign-in entry: the system's own (the autostart plugin), or a stand-in.
pub trait Launcher {
    /// Writes the entry, turned on.
    fn enable(&self) -> Result<(), String>;
    /// Removes the entry.
    fn disable(&self) -> Result<(), String>;
}

impl Launcher for tauri_plugin_autostart::AutoLaunchManager {
    fn enable(&self) -> Result<(), String> {
        tauri_plugin_autostart::AutoLaunchManager::enable(self).map_err(|e| e.to_string())
    }
    fn disable(&self) -> Result<(), String> {
        tauri_plugin_autostart::AutoLaunchManager::disable(self).map_err(|e| e.to_string())
    }
}

/// Carries out one start-at-sign-in step: decides from the entry's `state` and does it, through
/// `launcher` or, to follow a choice made outside Wirecatch, by turning the setting off.
pub fn apply_autostart(
    launcher: &impl Launcher,
    manager: &Manager,
    state: autostart::EntryState,
    wanted: bool,
    just_turned_on: bool,
) -> Result<Action, String> {
    let action = autostart::decide(wanted, state, just_turned_on);
    match action {
        Action::Write => launcher.enable()?,
        Action::Remove => launcher.disable()?,
        Action::Nothing => {}
        Action::TurnSettingOff => {
            tracing::info!("starting with the system was turned off outside Wirecatch");
            // Only this one setting, so a change made meanwhile in a window is kept.
            manager
                .change_settings(AppSettingsPatch {
                    start_with_system: Some(false),
                    ..AppSettingsPatch::default()
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(action)
}

/// Makes the system's start-at-sign-in entry match the setting (see [`crate::autostart`]). The
/// entry starts the app with `--background`. `just_turned_on` is set when the person switched
/// the setting on a moment ago. Runs off the async threads because it touches the registry or
/// the disk.
pub fn sync_autostart<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Manager,
    wanted: bool,
    just_turned_on: bool,
) {
    let app = app.clone();
    let manager = manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if app
            .try_state::<tauri_plugin_autostart::AutoLaunchManager>()
            .is_none()
        {
            return;
        }
        let state = autostart::read_state(crate::autostart_name());
        if let Err(e) = apply_autostart(&*app.autolaunch(), &manager, state, wanted, just_turned_on)
        {
            tracing::warn!("could not change starting with the system: {e}");
        }
    });
}
