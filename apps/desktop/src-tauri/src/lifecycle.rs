//! Staying alive in the tray and quitting cleanly (D13).
//!
//! Closing the window destroys it, which frees the webview's memory. With `close_to_tray` on,
//! the process keeps running (Tauri asks to exit when the last window closes; that request is
//! refused). With it off, closing the window quits like the tray's Quit: every download stops
//! so it can continue on the next start, the database is flushed, then the process exits.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager as _, RunEvent, Runtime};
use wirecatch_core::Manager;

use crate::host::Core;

/// The longest a quit waits for downloads to stop before exiting anyway. The manager already
/// abandons a stuck download after its own stop timeout; this is the backstop.
const QUIT_PATIENCE: Duration = Duration::from_secs(15);

/// Where the app is in its life, and the background tasks to stop when it ends.
#[derive(Default)]
pub struct Lifecycle {
    quitting: AtomicBool,
    finished: AtomicBool,
    /// The tray icon could not be made, so closing the window quits.
    no_tray: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Lifecycle {
    /// Records that there is no tray icon: with nothing to come back through, closing the window
    /// quits, as if `close_to_tray` were off.
    pub fn without_tray(&self) {
        self.no_tray.store(true, Ordering::SeqCst);
    }

    /// Keeps a background task so quitting can stop it.
    pub fn keep(&self, task: JoinHandle<()>) {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(task);
    }

    fn stop_tasks(&self) {
        for task in self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            task.abort();
        }
    }
}

/// Stops the manager so every download continues next time, then calls `exit`. Waits at most
/// [`QUIT_PATIENCE`].
pub async fn quit_sequence(manager: Option<&Manager>, exit: impl FnOnce()) {
    if let Some(manager) = manager
        && tokio::time::timeout(QUIT_PATIENCE, manager.shutdown())
            .await
            .is_err()
    {
        tracing::warn!("downloads took too long to stop; exiting anyway");
    }
    exit();
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
        let manager = core.as_ref().and_then(|c| c.get());
        quit_sequence(manager, || {
            if let Some(life) = app.try_state::<Lifecycle>() {
                life.finished.store(true, Ordering::SeqCst);
            }
            app.exit(0);
        })
        .await;
    });
}

/// Whether closing the last window should leave the app running: only with a tray icon to come
/// back through, and when `close_to_tray` is on (`None`, while the manager is still opening,
/// counts as on, the default).
pub fn keeps_running(has_tray: bool, close_to_tray: Option<bool>) -> bool {
    has_tray && close_to_tray.unwrap_or(true)
}

fn keeps_running_now<R: Runtime>(app: &AppHandle<R>) -> bool {
    let has_tray = app
        .try_state::<Lifecycle>()
        .is_some_and(|life| !life.no_tray.load(Ordering::SeqCst));
    let close_to_tray = app
        .try_state::<Core>()
        .and_then(|core| core.get().map(|m| m.get_settings().close_to_tray));
    keeps_running(has_tray, close_to_tray)
}

/// Handles the app's run events.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    if let RunEvent::ExitRequested { code, api, .. } = event {
        let finished = app
            .try_state::<Lifecycle>()
            .is_some_and(|life| life.finished.load(Ordering::SeqCst));
        if finished {
            // Our own exit, after the manager stopped.
            return;
        }
        api.prevent_exit();
        // `None` means the last window closed; anything else is a request to exit.
        if code.is_some() || !keeps_running_now(app) {
            quit(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_window_keeps_running_only_with_a_tray_and_close_to_tray_on() {
        assert!(keeps_running(true, Some(true)));
        assert!(keeps_running(true, None), "the default while opening is on");
        assert!(!keeps_running(true, Some(false)));
        assert!(!keeps_running(false, Some(true)), "no tray, no way back");
        assert!(!keeps_running(false, None));
    }
}
