//! A window for each download (D23).
//!
//! Every question the core asks about a browser download (confirm a file, the video being read,
//! pick a quality, install the media add-on) opens its own small window in front, whatever the
//! main window is doing. After Download now the same window becomes that download's progress
//! window. "Open in its own window" in the main list opens one for any download.
//!
//! - **Labels**: `download-q<question id>` for a window opened for a question and
//!   `download-d<download id>` for one opened for a download, so the two id spaces never meet. A
//!   question window keeps its label when it starts showing its download; the [`Registry`]
//!   knows what each window shows now.
//! - **Limit**: at most [`MAX_QUESTION_WINDOWS`] windows ask a question at once; later
//!   questions wait and open, oldest first, as those windows close or start showing progress.
//! - **Placement**: each window takes the lowest free slot, and slot `n` sits
//!   [`DOWNLOAD_WINDOW_CASCADE_STEP`] logical pixels down and right of slot `n - 1`, on the
//!   screen under the pointer.
//! - **Front**: windows open always on top and focused, and drop always-on-top the first time
//!   they lose focus after having had it, so they behave like normal windows from then on.
//! - **Closing**: closing a question window does not answer it. The question stays open in the
//!   core and is counted in the main window's sidebar; it is not opened again by itself (only an
//!   event that moves it on, or the person, opens it again).
//!
//! - **Scope**: a download window shows text that websites chose, so it is trusted with little
//!   (D43). Its capability grants only the commands it calls, and [`may_act_on`] and
//!   [`may_change`] keep those to the question or download it shows and to its own two settings.
//!
//! Windows are created on the main thread ([`AppHandle::run_on_main_thread`]), never from an
//! async task or a command directly, so nothing blocks and nothing deadlocks on Windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, LogicalPosition, Manager as _, Runtime, WebviewUrl, WebviewWindowBuilder};
use wirecatch_core::Manager;
use wirecatch_protocol::app::AppSettingsPatch;
use wirecatch_protocol::desktop::{DOWNLOAD_PARAMETER, DOWNLOAD_WINDOW_PAGE, QUESTION_PARAMETER};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::{DownloadId, QuestionId};

pub use wirecatch_shell::download_windows::{
    Area, LABEL_PREFIX, MAX_WINDOW_HEIGHT, MIN_WINDOW_SIZE, OpenWindow, Registry, SCREEN_MARGIN,
    Shows, Step, WINDOW_SIZE, fitted_height, is_download_window, open_question_ids, open_questions,
    place,
};

use crate::host::Core;
use crate::window::{bring_forward, show_focused, warn_if_failed};

/// The interface's page for this, relative to the app's address.
#[must_use]
pub fn route(shows: Shows) -> String {
    match shows {
        Shows::Question(id) => format!("{DOWNLOAD_WINDOW_PAGE}?{QUESTION_PARAMETER}={}", id.0),
        Shows::Download(id) => format!("{DOWNLOAD_WINDOW_PAGE}?{DOWNLOAD_PARAMETER}={}", id.0),
    }
}

/// Makes a download window as tall as its content (`height`, in logical pixels, as the page
/// measured it), keeping its width, and moves it up if it would now reach past the bottom of
/// its screen.
pub fn fit_to_content<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    let monitor = window.current_monitor().ok().flatten();
    let scale = window.scale_factor().unwrap_or(1.0);
    let area = monitor.as_ref().map(|m| {
        let s = m.scale_factor();
        let work = m.work_area();
        (
            f64::from(work.position.y) / s,
            f64::from(work.size.height) / s,
        )
    });
    let height = fitted_height(height, area.map(|(_, h)| h));
    let Ok(size) = window.inner_size() else {
        return;
    };
    let width = f64::from(size.width) / scale;
    if (f64::from(size.height) / scale - height).abs() < 1.0 {
        return;
    }
    warn_if_failed(
        "resize a download window",
        window.set_size(tauri::LogicalSize::new(width, height)),
    );
    if let (Some((top, screen)), Ok(at)) = (area, window.outer_position()) {
        let y = f64::from(at.y) / scale;
        let bottom = top + screen - SCREEN_MARGIN / 2.0;
        if y + height > bottom {
            let x = f64::from(at.x) / scale;
            warn_if_failed(
                "move a download window onto the screen",
                window.set_position(LogicalPosition::new(x, (bottom - height).max(top))),
            );
        }
    }
}

/// The download windows, kept by the app.
#[derive(Default)]
pub struct DownloadWindows(Mutex<Registry>);

impl DownloadWindows {
    fn with<T>(&self, change: impl FnOnce(&mut Registry) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// What is open now, for tests.
    pub fn snapshot(&self) -> Vec<OpenWindow> {
        self.with(|r| r.windows().to_vec())
    }

    /// The questions waiting for room to open their window, oldest first, for tests.
    pub fn waiting(&self) -> Vec<QuestionId> {
        self.with(|r| r.waiting().collect())
    }
}

/// What a download window is told when it acts on anything but what it shows.
pub const NOT_ITS_OWN: &str =
    "This window acts only on its own download. Use the main window for the others.";

/// What a download window is told when it changes a setting other than its own two.
pub const NOT_ITS_SETTING: &str =
    "This window changes only its own settings. Use Settings in the main window for the others.";

/// Whether the window `label` may act on `target`: the main window on anything, a download
/// window only on the question or download it shows now (D43), and the download complete
/// window on the downloads it lists (D52).
pub fn may_act_on<R: Runtime>(app: &AppHandle<R>, label: &str, target: Shows) -> bool {
    !is_download_window(label)
        || app
            .try_state::<DownloadWindows>()
            .is_some_and(|windows| windows.with(|r| r.shows(label)) == Some(target))
        || matches!(target, Shows::Download(id) if crate::finished_window::lists(app, label, id))
}

/// Whether a progress window shows download `id` now.
pub fn shows_download<R: Runtime>(app: &AppHandle<R>, id: DownloadId) -> bool {
    app.try_state::<DownloadWindows>()
        .is_some_and(|windows| windows.with(|r| r.window_showing(Shows::Download(id)).is_some()))
}

/// Whether a download window may make the settings change `patch`: only the two settings it
/// shows, whether browser downloads start without asking and whether progress windows open
/// (D43). The download complete window has its own (`finished_window::may_change`).
#[must_use]
pub fn may_change(patch: &AppSettingsPatch) -> bool {
    let its_own = AppSettingsPatch {
        extension: patch
            .extension
            .as_ref()
            .map(|extension| ExtensionSettingsPatch {
                ask_before_download: extension.ask_before_download,
                ..ExtensionSettingsPatch::default()
            }),
        show_progress_windows: patch.show_progress_windows,
        ..AppSettingsPatch::default()
    };
    *patch == its_own
}

fn update<R: Runtime>(app: &AppHandle<R>, change: impl FnOnce(&mut Registry) -> Vec<Step>) {
    let Some(windows) = app.try_state::<DownloadWindows>() else {
        return;
    };
    let steps = windows.with(change);
    for step in steps {
        apply(app, step);
    }
}

/// The core asked question `id` (or moved it on): its window opens if it is not open.
pub fn ask<R: Runtime>(app: &AppHandle<R>, id: QuestionId) {
    update(app, |r| r.ask(id).into_iter().collect());
}

/// Question `id` closed in the core.
pub fn question_closed<R: Runtime>(app: &AppHandle<R>, id: QuestionId) {
    update(app, |r| {
        r.closed(id);
        Vec::new()
    });
}

/// Opens windows for the questions already open in the core (at start, or after events were
/// missed), except those the person put aside. Does nothing while the manager is not open.
pub fn restore<R: Runtime>(app: &AppHandle<R>) {
    let Some(open) = app
        .try_state::<Core>()
        .and_then(|core| core.get().map(open_questions))
    else {
        return;
    };
    update(app, |r| r.restore(&open));
}

/// Brings every open question's window to the front, opening the closed ones again.
pub fn show_all<R: Runtime>(app: &AppHandle<R>, manager: &Manager) {
    let open = open_questions(manager);
    update(app, |r| r.show_all(&open));
}

/// The window `label` now shows download `id`.
pub fn became_download<R: Runtime>(app: &AppHandle<R>, label: &str, id: DownloadId) {
    update(app, |r| r.became_download(label, id));
}

/// The window `label` was destroyed.
pub fn window_gone<R: Runtime>(app: &AppHandle<R>, label: &str) {
    update(app, |r| r.gone(label));
}

/// Brings question `id`'s window forward, opening it again if it was closed.
pub fn show_question<R: Runtime>(app: &AppHandle<R>, id: QuestionId) {
    update(app, |r| r.show_question(id).into_iter().collect());
}

/// Opens (or brings forward) the progress window of download `id`.
pub fn show_download<R: Runtime>(app: &AppHandle<R>, id: DownloadId) {
    update(app, |r| vec![r.show_download(id)]);
}

fn apply<R: Runtime>(app: &AppHandle<R>, step: Step) {
    let handle = app.clone();
    let run = app.run_on_main_thread(move || match step {
        Step::Open(window) => create(&handle, &window),
        Step::Focus(label) => {
            if let Some(window) = handle.get_webview_window(&label) {
                bring_forward(&window);
            }
        }
    });
    if let Err(e) = run {
        tracing::warn!("could not reach the window thread: {e}");
    }
}

/// The screen a new window goes on: the one under the pointer (where the browser most likely
/// is), else the main screen. `None` when the system does not say.
fn screen_area<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Area> {
    let monitor = window
        .cursor_position()
        .ok()
        .and_then(|at| window.monitor_from_point(at.x, at.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    Some(Area {
        x: f64::from(work.position.x) / scale,
        y: f64::from(work.position.y) / scale,
        width: f64::from(work.size.width) / scale,
        height: f64::from(work.size.height) / scale,
    })
}

fn create<R: Runtime>(app: &AppHandle<R>, open: &OpenWindow) {
    if let Some(existing) = app.get_webview_window(&open.label) {
        // Still closing from before: show it rather than fail on the label.
        warn_if_failed("focus a download window", existing.set_focus());
        return;
    }
    let title = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| crate::host::CHANNEL.name().to_owned());
    let builder =
        WebviewWindowBuilder::new(app, &open.label, WebviewUrl::App(route(open.shows).into()));
    let built = crate::appearance::frameless(app, builder)
        .title(title)
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .resizable(true)
        .maximizable(false)
        .always_on_top(true)
        .focused(true)
        .visible(false)
        .disable_drag_drop_handler()
        .build();
    let window = match built {
        Ok(window) => {
            crate::appearance::paint_background(&window);
            window
        }
        Err(e) => {
            tracing::error!("could not open a download window: {e}");
            update(app, |r| r.failed(&open.label));
            return;
        }
    };
    match screen_area(&window) {
        Some(area) => {
            let (x, y) = place(open.slot, area, WINDOW_SIZE);
            warn_if_failed(
                "place a download window",
                window.set_position(LogicalPosition::new(x, y)),
            );
        }
        None => warn_if_failed("center a download window", window.center()),
    }
    show_focused(&window);
    // On top until it has had the focus and lost it, then a normal window.
    let had_focus = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let this = window.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::Focused(focused) = event else {
            return;
        };
        if *focused {
            had_focus.store(true, Ordering::SeqCst);
            return;
        }
        if !had_focus.load(Ordering::SeqCst) || dropped.load(Ordering::SeqCst) {
            return;
        }
        // On Windows the window also reports losing the focus when its own page takes it, so
        // look again once things settle: only another window having it counts.
        let (this, dropped) = (this.clone(), dropped.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(FOCUS_SETTLE).await;
            if this.is_focused().unwrap_or(true) {
                return;
            }
            if !dropped.swap(true, Ordering::SeqCst) {
                drop_always_on_top(&this);
            }
        });
    });
}

/// Makes a download window a normal window again, behind the window that took the focus.
fn drop_always_on_top<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    warn_if_failed(
        "stop keeping a download window on top",
        window.set_always_on_top(false),
    );
    // Windows puts a window that stops being always on top above every normal window, so it
    // would jump over the browser the person just went back to. Put it right behind that one.
    #[cfg(windows)]
    {
        let this = window.clone();
        warn_if_failed(
            "put a download window behind the browser",
            window.run_on_main_thread(move || behind_foreground(&this)),
        );
    }
}

#[cfg(windows)]
fn behind_foreground<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    let Ok(handle) = window.hwnd() else {
        return;
    };
    let ours = handle.0 as HWND;
    // SAFETY: both calls take window handles by value and only fail on a stale one; nothing
    // is read or written through them.
    unsafe {
        let front = GetForegroundWindow();
        if front.is_null() || front == ours {
            return;
        }
        SetWindowPos(
            ours,
            front,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// How long after a lost focus a download window checks whether another window has it.
const FOCUS_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::PortablePath;

    fn q(id: u64) -> QuestionId {
        QuestionId(id)
    }

    #[test]
    fn a_download_window_changes_only_its_own_two_settings() {
        let start_without_asking = AppSettingsPatch {
            extension: Some(ExtensionSettingsPatch {
                ask_before_download: Some(false),
                ..ExtensionSettingsPatch::default()
            }),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(&start_without_asking));
        let close_when_started = AppSettingsPatch {
            show_progress_windows: Some(false),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(&close_when_started));
        assert!(may_change(&AppSettingsPatch::default()));

        let elsewhere = AppSettingsPatch {
            default_folder: Some(PortablePath(r"C:\Users\someone\Startup".to_owned())),
            show_progress_windows: Some(false),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&elsewhere));
        let more_of_the_extension = AppSettingsPatch {
            extension: Some(ExtensionSettingsPatch {
                ask_before_download: Some(false),
                capture_enabled: Some(true),
                ..ExtensionSettingsPatch::default()
            }),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&more_of_the_extension));
        let start_with_system = AppSettingsPatch {
            start_with_system: Some(true),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&start_with_system));
    }

    #[test]
    fn labels_never_mix_questions_and_downloads() {
        assert_eq!(Shows::Question(q(7)).label(), "download-q7");
        assert_eq!(Shows::Download(DownloadId(7)).label(), "download-d7");
        assert!(is_download_window("download-q7"));
        assert!(!is_download_window("main"));
        assert_eq!(route(Shows::Question(q(7))), "download.html?question=7");
        assert_eq!(
            route(Shows::Download(DownloadId(3))),
            "download.html?download=3"
        );
    }

    /// Tauri gives a window permissions by its label: without this capability a download
    /// window could not listen to events or pick a folder.
    #[test]
    fn the_capability_covers_every_download_window() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/download-windows.json")).unwrap();
        let windows = capability["windows"].as_array().unwrap();
        assert_eq!(windows, &[serde_json::json!(format!("{LABEL_PREFIX}*"))]);
        let permissions = capability["permissions"].to_string();
        assert!(permissions.contains("core:event:allow-listen"));
        assert!(permissions.contains("dialog:allow-open"));
    }
}
