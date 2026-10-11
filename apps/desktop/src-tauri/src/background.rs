//! Work that runs for as long as the app does: forwarding core events to the webview, and
//! keeping the tray tooltip, notifications and start-with-system in step with the core.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager as _, Runtime};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use wirecatch_core::Manager;
use wirecatch_protocol::app::UiEvent;

use crate::activity::{Activity, Finished, notification_text};
use crate::autostart::{self, Action};
use crate::questions::{Attention, Questions, attention};
use crate::window::MAIN_WINDOW;
use crate::{EVENT_NAME, RESYNC_EVENT, tray, window};

/// The tooltip changes at most this often.
pub const TOOLTIP_EVERY: Duration = Duration::from_secs(1);

/// Downloads finishing within this long of the first one share one notification.
pub const NOTIFY_BATCH: Duration = Duration::from_secs(3);

/// Sends every core event to the webview on [`EVENT_NAME`]. While no window exists, a question
/// opens one and a storage failure becomes a system notification (see [`crate::questions`]);
/// other events are skipped, since the interface loads everything fresh when it opens. If this
/// falls behind and misses events, the interface is told to reload on [`RESYNC_EVENT`].
pub async fn forward_events<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    forward(app, manager.subscribe()).await;
}

/// [`forward_events`] from any receiver of core events, until its sender is gone.
pub async fn forward<R: Runtime>(app: AppHandle<R>, mut events: broadcast::Receiver<UiEvent>) {
    loop {
        match events.recv().await {
            Ok(event) => {
                if let Some(questions) = app.try_state::<Questions>() {
                    questions.note(&event);
                }
                if app.get_webview_window(MAIN_WINDOW).is_none() {
                    match attention(&event) {
                        Attention::None => {}
                        Attention::OpenWindow => window::show_main_window(&app),
                        Attention::Notify { title, body } => show_notification(&app, title, body),
                    }
                    continue;
                }
                if let Err(e) = app.emit(EVENT_NAME, &event) {
                    tracing::warn!("could not send an event to the window: {e}");
                }
            }
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!("the window missed {missed} events; asking it to reload");
                let _ = app.emit(RESYNC_EVENT, ());
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
            let mut settings = manager.get_settings();
            settings.start_with_system = false;
            manager
                .update_settings(settings)
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
