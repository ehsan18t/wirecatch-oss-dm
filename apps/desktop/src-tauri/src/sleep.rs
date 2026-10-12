//! The main window closed to the tray: hidden, not destroyed, so it opens again at once (D13).
//!
//! While hidden it costs as little as it can. On Windows its WebView2 is suspended (no script,
//! timers or painting, like a sleeping tab in Edge) and asked to keep its memory low; showing it
//! wakes it. Core events are not sent to a sleeping window (they would only pile up), so on
//! waking it is told to load its list again (`RESYNC_EVENT`), which it does in place.
//! WebKitGTK has no such switch, so on Linux the window is only hidden.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindow};

use crate::window::MAIN_WINDOW;

/// Whether the main window is hidden in the tray and its web view asleep.
#[derive(Default)]
pub struct MainAsleep(AtomicBool);

impl MainAsleep {
    /// True while the main window is hidden in the tray.
    pub fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn set(&self, asleep: bool) -> bool {
        self.0.swap(asleep, Ordering::SeqCst)
    }
}

/// Whether events should skip the window `label` now: only the main window, while asleep.
pub fn skips_events<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    label == MAIN_WINDOW && app.try_state::<MainAsleep>().is_some_and(|s| s.get())
}

/// Hides the main window to the tray and puts its web view to sleep.
pub fn hide_main<R: Runtime>(window: &WebviewWindow<R>) {
    if let Some(state) = window.try_state::<MainAsleep>() {
        state.set(true);
    }
    crate::window::warn_if_failed("hide the main window", window.hide());
    set_asleep(window, true);
}

/// Wakes the main window's web view if it was asleep, and tells it to load its list again,
/// since it missed the events sent while it slept. Call before showing it.
pub fn wake_main<R: Runtime>(window: &WebviewWindow<R>) {
    let was_asleep = window
        .try_state::<MainAsleep>()
        .is_some_and(|state| state.set(false));
    if !was_asleep {
        return;
    }
    set_asleep(window, false);
    crate::window::warn_if_failed(
        "ask the woken main window to reload",
        window.emit_to(MAIN_WINDOW, crate::RESYNC_EVENT, ()),
    );
}

#[cfg(windows)]
fn set_asleep<R: Runtime>(window: &WebviewWindow<R>, asleep: bool) {
    let result = window.with_webview(move |webview| {
        if let Err(e) = webview2::set_asleep(&webview.controller(), asleep) {
            tracing::debug!(asleep, "could not change the hidden window's web view: {e}");
        }
    });
    if let Err(e) = result {
        tracing::debug!("could not reach the window's web view: {e}");
    }
}

#[cfg(not(windows))]
fn set_asleep<R: Runtime>(_window: &WebviewWindow<R>, _asleep: bool) {}

#[cfg(windows)]
mod webview2 {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_3, ICoreWebView2_19, ICoreWebView2Controller,
    };
    use webview2_com::TrySuspendCompletedHandler;
    use windows_core::Interface as _;

    /// Suspends (or resumes) the web view and lowers (or restores) its memory target. A web view
    /// too old for either just stays as it is.
    pub fn set_asleep(
        controller: &ICoreWebView2Controller,
        asleep: bool,
    ) -> windows_core::Result<()> {
        // SAFETY: COM calls on the WebView2 controller and its web view, made on the window's
        // own thread (`with_webview` runs there), with interfaces checked by `cast`.
        unsafe {
            let webview = controller.CoreWebView2()?;
            let level = if asleep {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            };
            if let Ok(newer) = webview.cast::<ICoreWebView2_19>() {
                newer.SetMemoryUsageTargetLevel(level)?;
            }
            let Ok(suspendable) = webview.cast::<ICoreWebView2_3>() else {
                return Ok(());
            };
            if asleep {
                // Only a web view that is not visible can be suspended.
                controller.SetIsVisible(false)?;
                let done = TrySuspendCompletedHandler::create(Box::new(|result, suspended| {
                    if result.is_err() || !suspended {
                        tracing::debug!("the hidden window's web view did not go to sleep");
                    }
                    Ok(())
                }));
                suspendable.TrySuspend(&done)?;
            } else {
                suspendable.Resume()?;
                controller.SetIsVisible(true)?;
            }
        }
        Ok(())
    }
}
