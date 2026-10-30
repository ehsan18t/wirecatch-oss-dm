//! Staying in the tray and quitting cleanly (D13, D104). Closing the main window leaves the app
//! in the tray with `close_to_tray` on and a tray icon to come back through; otherwise it quits.
//! A quit closes every window at once, then exits once every download has stopped.

use gpui_kit::{App, Global};
use wirecatch_shell::quit;

use crate::backend::Backend;
use crate::tray;

/// Present once a quit has begun.
struct Quitting;

impl Global for Quitting {}

/// Whether the app is quitting: nothing opens again.
pub fn is_quitting(cx: &App) -> bool {
    cx.has_global::<Quitting>()
}

/// The main window is closing; quits unless the app stays in the tray. `close_to_tray` is
/// `None` while the manager is still opening.
pub fn window_closing(close_to_tray: Option<bool>, cx: &mut App) {
    if !quit::keeps_running(tray::exists(cx), close_to_tray) {
        self::quit(cx);
    }
}

/// Quits: the windows and the tray icon go at once, every download stops so it continues next
/// time (within [`quit::QUIT_PATIENCE`]), then the process exits. Safe to call repeatedly.
pub fn quit(cx: &mut App) {
    if is_quitting(cx) {
        return;
    }
    cx.set_global(Quitting);
    let stopped = Backend::stop(cx);
    tray::remove(cx);
    for window in cx.windows() {
        _ = window.update(cx, |_, window, _| window.remove_window());
    }
    cx.spawn(async move |cx| {
        if let Err(e) = stopped.await {
            tracing::error!("stopping the downloads failed: {e}");
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
