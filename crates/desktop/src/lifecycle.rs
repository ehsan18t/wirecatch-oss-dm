//! Staying in the tray and quitting cleanly (D13, D104). Closing the main window leaves the app
//! in the tray with `close_to_tray` on and a tray icon to come back through; otherwise it quits.
//! A quit closes every window at once, then exits once every download has stopped. A launch
//! that arrives meanwhile starts the app again once it has exited.

use gpui_kit::{App, Global};
use wirecatch_shell::{autostart, quit};

use crate::backend::Backend;
use crate::library::Asked;
use crate::tray;

/// Present once a quit has begun.
#[derive(Default)]
struct Quitting {
    /// The command line to start again with (program name left out), once a launch arrived.
    again: Option<Vec<String>>,
}

impl Global for Quitting {}

/// Whether the app is quitting: nothing opens again.
pub fn is_quitting(cx: &App) -> bool {
    cx.has_global::<Quitting>()
}

/// Keeps `asked` to start the app again with once it has exited, when it is quitting: a window
/// that is about to close would only flash. Returns `false` when not quitting.
pub fn start_again_later(asked: &Asked, cx: &mut App) -> bool {
    if !is_quitting(cx) {
        return false;
    }
    let links = match asked {
        Asked::Links(links) => links.clone(),
        Asked::Window | Asked::BrowserSettings | Asked::Reveal(_) => Vec::new(),
    };
    cx.global_mut::<Quitting>().again = Some(links);
    true
}

/// The main window is closing; quits unless the app stays in the tray. `close_to_tray` is
/// `None` while the manager is still opening.
pub fn window_closing(close_to_tray: Option<bool>, cx: &mut App) {
    if !quit::keeps_running(tray::exists(cx), close_to_tray) {
        self::quit(cx);
    }
}

/// Quits, and starts the program again once this copy has exited: an AppImage just replaced.
pub fn quit_and_start_again(cx: &mut App) {
    self::quit(cx);
    cx.global_mut::<Quitting>().again.get_or_insert_default();
}

/// Quits: the windows and the tray icon go at once, every download stops so it continues next
/// time (within [`quit::QUIT_PATIENCE`]), then the process ends, with whatever is left (D144).
/// Safe to call repeatedly.
pub fn quit(cx: &mut App) {
    if is_quitting(cx) {
        return;
    }
    cx.set_global(Quitting::default());
    let stopped = Backend::stop(cx);
    tray::remove(cx);
    for window in cx.windows() {
        _ = window.update(cx, |_, window, _| window.remove_window());
    }
    cx.spawn(async move |cx| {
        if let Err(e) = stopped.await {
            tracing::error!("stopping the downloads failed: {e}");
        }
        // Endpoint first, whether or not a start is due: a launch answered after `again` is read
        // would be lost, with the program that sent it gone too.
        cx.update(Backend::free_endpoint).await;
        let again = cx.update(|cx| cx.global_mut::<Quitting>().again.take());
        if let Some(args) = again {
            start_again(&args);
        }
        // At once after the start: past the limit, the new copy needs the database lock this one holds.
        quit::end_process();
    })
    .detach();
}

/// Starts a new copy of the app with `args`, after this one freed its endpoint.
fn start_again(args: &[String]) {
    tracing::info!("starting again after quitting");
    let started = autostart::program()
        .and_then(|program| std::process::Command::new(program).args(args).spawn());
    if let Err(e) = started {
        tracing::warn!("could not start the app again: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui_kit::TestAppContext;
    use wirecatch_test_support::PATIENCE;

    use super::*;

    /// Set for the child of the test below, whose quit must end its process.
    const QUITTING_CHILD_VAR: &str = "WIRECATCH_TEST_QUITTING";

    /// A child whose quit only returned to the harness would print its result (D144).
    #[gpui_kit::test]
    fn a_quit_ends_the_process(cx: &mut TestAppContext) {
        if std::env::var_os(QUITTING_CHILD_VAR).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "lifecycle::tests::a_quit_ends_the_process"])
                .env(QUITTING_CHILD_VAR, "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let printed = String::from_utf8_lossy(&output.stdout);
            assert!(printed.contains("running 1 test"), "{printed}");
            assert!(!printed.contains("test result"), "{printed}");
            return;
        }
        cx.update(|cx| {
            cx.set_global(Backend::unopened());
            quit(cx);
        });
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            cx.run_until_parked();
            // Another thread stops the manager: there is no event here to wait on.
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
}
