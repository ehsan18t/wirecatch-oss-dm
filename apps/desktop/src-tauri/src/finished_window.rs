//! The download complete window (D52): one small window, at the bottom right of the main screen,
//! that collects what finished or failed until the person closes it. It opens without taking
//! the keyboard focus and never closes by itself: the person chose it to see what finished when
//! they come back. It is a download window (its label starts with
//! [`crate::download_windows::LABEL_PREFIX`]), so it gets their capability, and it may act only
//! on the downloads it lists.

use std::sync::{Mutex, PoisonError};

use tauri::{
    AppHandle, Emitter as _, LogicalPosition, Manager as _, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AppSettingsPatch, DownloadSummary, NotifyOnFinish, UiEvent};
use wirecatch_protocol::desktop::{
    DOWNLOAD_WINDOW_PAGE, FINISHED_EVENT, FINISHED_PARAMETER, FinishedList, FinishedQueue,
};

use crate::download_windows::{self, MIN_WINDOW_SIZE, SCREEN_MARGIN, WINDOW_SIZE, fitted_height};
use crate::window::{show_quietly, warn_if_failed};
pub use wirecatch_shell::finished::{FinishedBook, queue_header};

/// The window's label.
pub const LABEL: &str = "download-finished";

/// The window's page, relative to the app's address.
#[must_use]
pub fn route() -> String {
    format!("{DOWNLOAD_WINDOW_PAGE}?{FINISHED_PARAMETER}=1")
}

/// What the window lists, kept by the app while it is open.
#[derive(Debug, Default)]
pub struct FinishedWindow(Mutex<FinishedBook>);

impl FinishedWindow {
    fn book(&self) -> std::sync::MutexGuard<'_, FinishedBook> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the window lists now.
    #[must_use]
    pub fn list(&self) -> FinishedList {
        self.book().list().clone()
    }
}

/// Adds what ended to the window, opening it if it is closed. A download whose own progress
/// window is open is left out: that window shows it finished, so it is never shown twice.
pub fn add<R: Runtime>(
    app: &AppHandle<R>,
    queue: Option<FinishedQueue>,
    ended: &[DownloadSummary],
) {
    let Some(window) = app.try_state::<FinishedWindow>() else {
        return;
    };
    let ended: Vec<DownloadSummary> = ended
        .iter()
        .filter(|d| !download_windows::shows_download(app, d.id))
        .cloned()
        .collect();
    // Counted from what the window lists, so the heading agrees with the rows under it.
    let queue = queue.map(|q| queue_header(q.id, &q.name, &ended));
    let list = {
        let mut book = window.book();
        if !book.add(queue, &ended) {
            return;
        }
        book.list().clone()
    };
    let handle = app.clone();
    let run = app.run_on_main_thread(move || match handle.get_webview_window(LABEL) {
        Some(open) => {
            warn_if_failed(
                "update the download complete window",
                open.emit_to(LABEL, FINISHED_EVENT, &list),
            );
            show_quietly(&open);
        }
        None => create(&handle),
    });
    if let Err(e) = run {
        tracing::warn!("could not reach the window thread: {e}");
    }
}

/// Keeps what the window lists in step with the core's events.
pub fn follow<R: Runtime>(app: &AppHandle<R>, event: &UiEvent) {
    if let Some(window) = app.try_state::<FinishedWindow>() {
        window.book().follow(event);
    }
}

/// Whether the window `label` may make the settings change `patch`: only this window, and only
/// its "Use notifications instead" (D52). A switch to anything else, such as nothing at all, is
/// Settings' alone, since a script in a page showing a website's text must not silence failures.
#[must_use]
pub fn may_change(label: &str, patch: &AppSettingsPatch) -> bool {
    label == LABEL
        && *patch
            == AppSettingsPatch {
                notify_on_finish: Some(NotifyOnFinish::Notification),
                ..AppSettingsPatch::default()
            }
}

/// Whether the window `label` is this one and lists download `id`.
pub fn lists<R: Runtime>(app: &AppHandle<R>, label: &str, id: DownloadId) -> bool {
    label == LABEL
        && app
            .try_state::<FinishedWindow>()
            .is_some_and(|window| window.book().lists(id))
}

/// The window is gone: what it listed is forgotten.
pub fn gone<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if label != LABEL {
        return;
    }
    if let Some(window) = app.try_state::<FinishedWindow>() {
        *window.book() = FinishedBook::default();
    }
}

/// The main screen's work area, in logical pixels: `(x, y, width, height)`.
fn work_area<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<(f64, f64, f64, f64)> {
    let monitor = window.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    Some((
        f64::from(work.position.x) / scale,
        f64::from(work.position.y) / scale,
        f64::from(work.size.width) / scale,
        f64::from(work.size.height) / scale,
    ))
}

/// Puts the window `height` tall at the bottom right of the main screen, a margin from its edges.
fn place<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    let Some((x, y, width, screen)) = work_area(window) else {
        return;
    };
    let height = fitted_height(height, Some(screen));
    let margin = SCREEN_MARGIN / 2.0;
    warn_if_failed(
        "size the download complete window",
        window.set_size(tauri::LogicalSize::new(WINDOW_SIZE.0, height)),
    );
    warn_if_failed(
        "place the download complete window",
        window.set_position(LogicalPosition::new(
            (x + width - WINDOW_SIZE.0 - margin).max(x),
            (y + screen - height - margin).max(y),
        )),
    );
}

/// Makes the window as tall as its content, keeping it at the bottom right.
pub fn fit<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    place(window, height);
}

fn create<R: Runtime>(app: &AppHandle<R>) {
    let title = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| crate::host::CHANNEL.name().to_owned());
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(route().into()));
    let built = crate::appearance::frameless(app, builder)
        .title(title)
        .inner_size(WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .resizable(true)
        .maximizable(false)
        .focused(false)
        .visible(false)
        .skip_taskbar(false)
        .disable_drag_drop_handler()
        .build();
    match built {
        Ok(window) => {
            crate::appearance::paint_background(&window);
            place(&window, MIN_WINDOW_SIZE.1);
            show_quietly(&window);
        }
        Err(e) => tracing::error!("could not open the download complete window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_window_may_switch_to_notifications_and_nothing_else() {
        let to = |notification| AppSettingsPatch {
            notify_on_finish: Some(notification),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(LABEL, &to(NotifyOnFinish::Notification)));
        assert!(!may_change(LABEL, &to(NotifyOnFinish::Nothing)));
        assert!(!may_change(
            "download-d1",
            &to(NotifyOnFinish::Notification)
        ));
    }
}
