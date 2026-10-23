//! Work that runs for as long as the app does: forwarding core events to the webview, and
//! keeping the tray tooltip, notifications and start-with-system in step with the core.

use std::collections::HashMap;
use std::time::Duration;

use tauri::{AppHandle, Emitter, EventTarget, Manager as _, Runtime};
use tauri_plugin_autostart::ManagerExt as _;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use wirecatch_core::Manager;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AppSettingsPatch, DownloadSummary, FinishNotice, Queue, QueueNotify, QueueReport, UiEvent,
};

use crate::activity::{Activity, Ended};
use crate::autostart::{self, Action};
use crate::questions::{Attention, attention};
use crate::window::MAIN_WINDOW;
use crate::{
    RESYNC_EVENT, UI_EVENT, download_windows, finished_window, sleep, system_notices, tray,
};

/// The tooltip changes at most this often.
pub const TOOLTIP_EVERY: Duration = Duration::from_secs(1);

/// Downloads finishing (or failing) within this long of the first one share one notice.
pub const NOTIFY_BATCH: Duration = Duration::from_secs(3);

/// Sends every core event to every open window (the main window and the download windows) on
/// [`UI_EVENT`], and opens a download window for each question the core asks (see
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
                    Attention::ShowBrowserSettings => crate::window::reveal(
                        &app,
                        &wirecatch_protocol::desktop::Reveal::browser_settings(),
                    ),
                    Attention::ShowCaptured(row) => {
                        if let Some(captured) = app.try_state::<crate::window::PendingCaptures>() {
                            captured.started(&row);
                        }
                        // The person is browsing: the window comes forward, the focus stays.
                        crate::window::show_main_window_with(&app, crate::window::Focus::Leave);
                    }
                    Attention::Theme(theme) => crate::appearance::follow_theme(&app, theme),
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
                    app.emit_filter(UI_EVENT, &event, |target| {
                        !matches!(target, EventTarget::WebviewWindow { label } if label == crate::window::MAIN_WINDOW)
                    })
                } else {
                    app.emit(UI_EVENT, &event)
                };
                if let Err(e) = sent {
                    tracing::warn!("could not send an event to the windows: {e}");
                }
            }
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!("the windows missed {missed} events; asking them to reload");
                crate::window::warn_if_failed(
                    "ask the windows to reload",
                    app.emit(RESYNC_EVENT, ()),
                );
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

/// What to tell the person about downloads that ended (D52).
#[derive(Debug, Clone, PartialEq)]
pub enum Tell {
    /// Downloads that finished close together, in queues that tell each download.
    Finished(Vec<DownloadSummary>),
    /// Downloads that failed close together, whatever their queue: a failure is told at once.
    Failed(Vec<DownloadSummary>),
    /// A queue that tells once it is done is done.
    Queue(QueueReport),
}

/// Decides when to tell what: each queue's choice, and downloads that end close together
/// sharing one notice. Holds no clock of its own, so tests give it the time.
#[derive(Debug, Default)]
pub struct Teller {
    modes: HashMap<QueueId, QueueNotify>,
    finished: Vec<DownloadSummary>,
    finished_due: Option<Instant>,
    failed: Vec<DownloadSummary>,
    failed_due: Option<Instant>,
}

impl Teller {
    /// Follows the queues' choices.
    pub fn queues(&mut self, queues: &[Queue]) {
        self.modes = queues.iter().map(|q| (q.id, q.notify)).collect();
    }

    fn tells_each(&self, queue: QueueId) -> bool {
        self.modes.get(&queue).copied().unwrap_or_default() == QueueNotify::EachDownload
    }

    /// Takes in downloads that ended `now`.
    pub fn ended(&mut self, ended: Ended, now: Instant) {
        let Ended { completed, failed } = ended;
        let told: Vec<DownloadSummary> = completed
            .into_iter()
            .filter(|r| self.tells_each(r.queue))
            .collect();
        if !told.is_empty() {
            self.finished.extend(told);
            self.finished_due.get_or_insert(now + NOTIFY_BATCH);
        }
        if !failed.is_empty() {
            self.failed.extend(failed);
            self.failed_due.get_or_insert(now + NOTIFY_BATCH);
        }
    }

    /// A queue is done: told now when it tells once it is done.
    #[must_use]
    pub fn queue_finished(&self, report: &QueueReport) -> Option<Tell> {
        (!self.tells_each(report.queue)).then(|| Tell::Queue(report.clone()))
    }

    /// When the next notice is due, if any waits.
    #[must_use]
    pub fn due(&self) -> Option<Instant> {
        match (self.finished_due, self.failed_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The notices due by `now`.
    pub fn take_due(&mut self, now: Instant) -> Vec<Tell> {
        let mut tells = Vec::new();
        if self.finished_due.is_some_and(|due| due <= now) {
            self.finished_due = None;
            tells.push(Tell::Finished(std::mem::take(&mut self.finished)));
        }
        if self.failed_due.is_some_and(|due| due <= now) {
            self.failed_due = None;
            tells.push(Tell::Failed(std::mem::take(&mut self.failed)));
        }
        tells
    }
}

/// Tells the person, the way `how` says (D52).
pub fn tell<R: Runtime>(app: &AppHandle<R>, how: FinishNotice, tell: &Tell) {
    match how {
        FinishNotice::Nothing => {}
        FinishNotice::Notification => {
            let notice = match tell {
                Tell::Finished(rows) => crate::notices::finished(&rows.iter().collect::<Vec<_>>()),
                Tell::Failed(rows) => crate::notices::failed(&rows.iter().collect::<Vec<_>>()),
                Tell::Queue(report) => crate::notices::queue_done(report),
            };
            if let Some(notice) = notice {
                system_notices::show(app, notice);
            }
        }
        FinishNotice::Window => match tell {
            Tell::Finished(rows) | Tell::Failed(rows) => finished_window::add(app, None, rows),
            Tell::Queue(report) => {
                let rows: Vec<DownloadSummary> = report
                    .completed
                    .iter()
                    .chain(&report.failed)
                    .cloned()
                    .collect();
                let header = finished_window::queue_header(report.queue, &report.name, &rows);
                finished_window::add(app, Some(header), &rows);
            }
        },
    }
}

/// Keeps the tray tooltip, the notices of finished and failed downloads, and start-with-system
/// up to date.
pub async fn watch<R: Runtime>(app: AppHandle<R>, manager: Manager) {
    let mut events = manager.subscribe();
    let mut activity = Activity::default();
    activity.load(&manager.list_downloads());
    let mut settings = manager.get_settings();
    let mut teller = Teller::default();
    teller.queues(&manager.list_queues());
    sync_autostart(&app, &manager, settings.start_with_system, false);

    let mut shown = String::new();
    let mut last_tooltip: Option<Instant> = None;
    let mut tooltip_stale = true;

    loop {
        let tooltip_at =
            tooltip_stale.then(|| last_tooltip.map_or_else(Instant::now, |t| t + TOOLTIP_EVERY));
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    finished_window::follow(&app, &event);
                    teller.ended(activity.apply(&event), Instant::now());
                    match event {
                        UiEvent::SettingsChanged(new) => {
                            if new.start_with_system != settings.start_with_system {
                                sync_autostart(&app, &manager, new.start_with_system, new.start_with_system);
                            }
                            settings = new;
                        }
                        UiEvent::QueuesChanged(queues) => teller.queues(&queues),
                        UiEvent::QueueFinished(report) => {
                            if let Some(now) = teller.queue_finished(&report) {
                                tell(&app, settings.finish_notice, &now);
                            }
                        }
                        _ => {}
                    }
                    tooltip_stale = true;
                }
                Err(RecvError::Lagged(_)) => {
                    activity.load(&manager.list_downloads());
                    settings = manager.get_settings();
                    teller.queues(&manager.list_queues());
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
            () = at(teller.due()) => {
                for due in teller.take_due(Instant::now()) {
                    tell(&app, settings.finish_notice, &due);
                }
            }
        }
    }
}

/// Shows a system notification with words only; a click shows the main window.
pub fn show_notification<R: Runtime>(app: &AppHandle<R>, title: String, body: String) {
    system_notices::show(app, system_notices::plain(title, body));
}

/// What changes the start-at-sign-in entry: the system's own (the autostart plugin), or a stand-in.
pub trait Launcher {
    /// Writes the entry, turned on.
    ///
    /// # Errors
    ///
    /// The system refused to write the entry (the registry or the autostart folder); the text
    /// says why.
    fn enable(&self) -> Result<(), String>;
    /// Removes the entry.
    ///
    /// # Errors
    ///
    /// The system refused to remove the entry; the text says why.
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
///
/// # Errors
///
/// The `launcher` could not write or remove the entry, or the setting could not be saved (the
/// manager has shut down).
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
