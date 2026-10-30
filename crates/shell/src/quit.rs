//! Quitting cleanly (D13): every download stops so it continues on the next start, within a
//! time limit, and the app keeps running in the tray only when there is one to come back through.

use std::time::Duration;

use wirecatch_core::Manager;

use crate::host::Core;

/// The longest a quit waits for downloads to stop before exiting anyway. The manager already
/// abandons a stuck download after its own stop timeout; this is the backstop.
pub const QUIT_PATIENCE: Duration = Duration::from_secs(15);

/// The longest a quit waits for a manager that is still opening.
pub const OPEN_PATIENCE: Duration = Duration::from_secs(10);

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

/// Quits with the manager in `core`, even one still opening: waits up to [`OPEN_PATIENCE`] for it
/// to open, so a quit right after start still stops it cleanly, then runs [`quit_sequence`].
pub async fn quit_core(core: Option<&Core>, exit: impl FnOnce()) {
    let manager = match core {
        Some(core) => match tokio::time::timeout(OPEN_PATIENCE, core.manager()).await {
            Ok(Ok(manager)) => Some(manager),
            Ok(Err(_)) => None,
            Err(_) => {
                tracing::warn!("the download list took too long to open; exiting anyway");
                None
            }
        },
        None => None,
    };
    quit_sequence(manager, exit).await;
}

/// Whether closing the main window should leave the app running: only with a tray icon to come
/// back through, and when `close_to_tray` is on (`None`, while the manager is still opening,
/// counts as on, the default).
#[must_use]
pub fn keeps_running(has_tray: bool, close_to_tray: Option<bool>) -> bool {
    has_tray && close_to_tray.unwrap_or(true)
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
