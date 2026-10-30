//! Staying alive in the tray and quitting cleanly (D13).
//!
//! With `close_to_tray` on, closing the main window hides it and puts its web view to sleep
//! (see [`crate::sleep`]), so it opens again at once from the tray. With it off, closing the
//! main window quits like the tray's Quit: every
//! download stops so it can continue on the next start, the database is flushed, then the
//! process exits. Closing a download window (D23) never quits, even when it is the last one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager as _, RunEvent, Runtime, WindowEvent};
pub use wirecatch_shell::quit::{keeps_running, quit_core, quit_sequence};

use crate::download_windows::{self, is_download_window};
use crate::host::Core;
use crate::sleep;
use crate::window::MAIN_WINDOW;

/// Where the app is in its life, and the background tasks to stop when it ends.
#[derive(Default)]
pub struct Lifecycle {
    quitting: AtomicBool,
    finished: AtomicBool,
    /// The tray icon could not be made, so closing the window quits.
    no_tray: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// A launch that arrived while quitting, to run once this process has exited.
    relaunch: Mutex<Option<Vec<String>>>,
    /// Replaces the final process exit (tests run without a real event loop to exit).
    exit_hook: Mutex<Option<ExitHook>>,
}

type ExitHook = Box<dyn FnOnce() + Send>;

/// Locks one of [`Lifecycle`]'s fields. A lock poisoned by a panic in another holder is taken
/// over as it is: each field is replaced or drained whole, so it is never half written, and
/// Wirecatch's release builds abort on panic (`panic = "abort"`), so there a poisoned lock cannot
/// be seen at all. Refusing the lock as well would only add a second, less telling panic.
fn locked<T>(field: &Mutex<T>) -> MutexGuard<'_, T> {
    field.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Lifecycle {
    /// Runs `hook` instead of exiting the process at the end of a quit. For tests, whose mock
    /// runtime cannot exit.
    pub fn on_exit(&self, hook: impl FnOnce() + Send + 'static) {
        *locked(&self.exit_hook) = Some(Box::new(hook));
    }

    /// Whether a quit has finished stopping everything and asked the process to exit.
    pub fn has_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// Records that there is no tray icon: with nothing to come back through, closing the window
    /// quits, as if `close_to_tray` were off.
    pub fn without_tray(&self) {
        self.no_tray.store(true, Ordering::SeqCst);
    }

    /// Keeps a background task so quitting can stop it.
    pub fn keep(&self, task: JoinHandle<()>) {
        locked(&self.tasks).push(task);
    }

    fn stop_tasks(&self) {
        for task in locked(&self.tasks).drain(..) {
            task.abort();
        }
    }
}

/// Quits the app: stops background work and the manager, then exits. Safe to call repeatedly.
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    let Some(life) = app.try_state::<Lifecycle>() else {
        app.exit(0);
        return;
    };
    if life.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    life.stop_tasks();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let core = app.try_state::<Core>();
        quit_core(core.as_deref(), || {
            let hook = app.try_state::<Lifecycle>().and_then(|life| {
                life.finished.store(true, Ordering::SeqCst);
                locked(&life.exit_hook).take()
            });
            if let Some(hook) = hook {
                hook();
            } else {
                // Everything has stopped, so a waiting update can replace the app now.
                crate::updates::install_pending(&app);
                app.exit(0);
            }
        })
        .await;
    });
}

impl Lifecycle {
    /// Whether the app is quitting.
    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }

    /// Called for a launch that arrives while the app is quitting: keeps its command line
    /// (program name first) to start the app again once this process has exited, instead of
    /// showing a window that is about to close. Returns `false` when not quitting, so the launch
    /// is handled as usual.
    pub fn relaunch_later(&self, args: &[String]) -> bool {
        if !self.is_quitting() {
            return false;
        }
        *locked(&self.relaunch) = Some(args.to_vec());
        true
    }

    /// The command line to start again after exiting, if a launch arrived while quitting.
    pub fn take_relaunch(&self) -> Option<Vec<String>> {
        locked(&self.relaunch).take()
    }
}

/// Starts the app again with `args` (program name first, which is skipped), after this process
/// gave up its single-instance claim.
fn relaunch<R: Runtime>(app: &AppHandle<R>, args: &[String]) {
    tauri_plugin_single_instance::destroy(app);
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(e) => {
            tracing::warn!("could not start Wirecatch again: {e}");
            return;
        }
    };
    if let Err(e) = std::process::Command::new(program)
        .args(args.iter().skip(1))
        .spawn()
    {
        tracing::warn!("could not start Wirecatch again: {e}");
    }
}

fn closing_quits_now<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    let has_tray = app
        .try_state::<Lifecycle>()
        .is_some_and(|life| !life.no_tray.load(Ordering::SeqCst));
    let close_to_tray = app
        .try_state::<Core>()
        .and_then(|core| core.get().map(|m| m.get_settings().close_to_tray));
    closing_quits(label, has_tray, close_to_tray)
}

fn hides_on_close_now<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    let quitting = app
        .try_state::<Lifecycle>()
        .is_some_and(|life| life.quitting.load(Ordering::SeqCst));
    !quitting && hides_on_close(label) && !closing_quits_now(app, label)
}

/// Whether closing the window `label` hides it instead (while the app keeps running in the
/// tray): only the main window, which then opens again at once. A download window closes.
#[must_use]
pub fn hides_on_close(label: &str) -> bool {
    label == MAIN_WINDOW
}

/// What closing the window `label` does: only closing the main window can quit (with no tray,
/// or with `close_to_tray` off). A download window is only a view of one download (D23), so
/// closing it never quits, even when it was the last window.
#[must_use]
pub fn closing_quits(label: &str, has_tray: bool, close_to_tray: Option<bool>) -> bool {
    label == MAIN_WINDOW && !keeps_running(has_tray, close_to_tray)
}

/// An exit was asked for. Our own, after the manager stopped, goes ahead; any other waits for the
/// manager. A `code` of `None` means the last window closed, which is handled when the main window
/// is destroyed ([`on_run_event`]); anything else is a request to exit.
fn exit_requested<R: Runtime>(app: &AppHandle<R>, code: Option<i32>, api: &tauri::ExitRequestApi) {
    let finished = app
        .try_state::<Lifecycle>()
        .is_some_and(|life| life.finished.load(Ordering::SeqCst));
    if finished {
        return;
    }
    api.prevent_exit();
    if code.is_some() {
        quit(app);
    }
}

/// Handles the app's run events.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { code, api, .. } => exit_requested(app, code, &api),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } => {
            if hides_on_close_now(app, &label)
                && let Some(window) = app.get_webview_window(&label)
            {
                api.prevent_close();
                sleep::hide_main(&window);
            }
        }
        RunEvent::WindowEvent {
            label,
            event: event @ (WindowEvent::Resized(_) | WindowEvent::Focused(_)),
            ..
        } => {
            if let Some(window) = app.get_webview_window(&label) {
                crate::frame::send_frame(&window);
                if matches!(event, WindowEvent::Resized(_)) {
                    crate::frame::place_overlay(&window);
                }
            }
            // Coming back from the system's settings is when a new accent colour is noticed.
            if matches!(event, WindowEvent::Focused(true)) {
                crate::appearance::refresh_system_accent(app);
            }
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::ScaleFactorChanged { .. },
            ..
        } => {
            // The button keeps its CSS size, so the page reports nothing: place it again.
            if let Some(window) = app.get_webview_window(&label) {
                crate::frame::place_overlay(&window);
            }
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::ThemeChanged(_),
            ..
        } => crate::appearance::system_theme_changed(app, &label),
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } => {
            crate::frame::forget(app, &label);
            if label == MAIN_WINDOW
                && let Some(selection) = app.try_state::<crate::window::PendingSelection>()
            {
                selection.page_gone();
            }
            if is_download_window(&label) {
                download_windows::window_gone(app, &label);
                crate::finished_window::gone(app, &label);
            } else if closing_quits_now(app, &label) {
                quit(app);
            }
        }
        RunEvent::Exit => {
            if let Some(args) = app
                .try_state::<Lifecycle>()
                .and_then(|life| life.take_relaunch())
            {
                relaunch(app, &args);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_during_quitting_waits_for_the_restart() {
        let life = Lifecycle::default();
        let args = vec![
            "wirecatch-app".to_string(),
            "https://x.example/f".to_string(),
        ];
        assert!(!life.relaunch_later(&args), "not quitting: handle it now");
        assert!(life.take_relaunch().is_none());
        life.quitting.store(true, Ordering::SeqCst);
        assert!(life.relaunch_later(&args));
        assert_eq!(life.take_relaunch(), Some(args));
        assert!(life.take_relaunch().is_none());
    }

    #[test]
    fn closing_the_main_window_hides_it_and_a_download_window_closes() {
        assert!(hides_on_close(MAIN_WINDOW));
        assert!(!hides_on_close("download-q1"));
        assert!(!hides_on_close("download-d7"));
    }

    #[test]
    fn only_closing_the_main_window_can_quit() {
        assert!(closing_quits(MAIN_WINDOW, true, Some(false)));
        assert!(closing_quits(MAIN_WINDOW, false, Some(true)));
        assert!(!closing_quits(MAIN_WINDOW, true, Some(true)));
        // The last download window closing, with the main window long gone: keep running.
        for (tray, to_tray) in [(true, Some(false)), (false, None), (true, None)] {
            assert!(!closing_quits("download-q3", tray, to_tray));
            assert!(!closing_quits("download-d9", tray, to_tray));
        }
    }
}
