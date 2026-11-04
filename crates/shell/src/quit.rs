//! Quitting cleanly (D13): every download stops so it continues on the next start, within a
//! time limit, and the app keeps running in the tray only when there is one to come back through.

use std::time::Duration;

use crate::host::Core;

/// The longest a quit waits for downloads to stop before ending the process anyway (D144). The manager already abandons a stuck download after its own stop timeout; this is the backstop, and it also cuts a long move short (D119).
pub const QUIT_PATIENCE: Duration = Duration::from_secs(15);

/// The longest a quit waits for a manager that is still opening.
pub const OPEN_PATIENCE: Duration = Duration::from_secs(10);

/// Stops the manager in `core`, even one still opening, so every download continues next time: waits up to [`OPEN_PATIENCE`] for it to open, so a quit right after start still stops it cleanly, then up to [`QUIT_PATIENCE`] for it to stop. What is left then ends with the process.
pub async fn stop(core: &Core) {
    let manager = match tokio::time::timeout(OPEN_PATIENCE, core.manager()).await {
        Ok(Ok(manager)) => manager,
        Ok(Err(_)) => return,
        Err(_) => {
            tracing::warn!("the download list took too long to open; exiting anyway");
            return;
        }
    };
    if tokio::time::timeout(QUIT_PATIENCE, manager.shutdown())
        .await
        .is_err()
    {
        tracing::warn!("downloads took too long to stop; exiting anyway");
    }
}

/// Ends the process now. Never by returning from `main`: dropping the runtime waits, with no limit, for file work that is stuck (D144).
pub fn end_process() -> ! {
    std::process::exit(0)
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

    /// Set for the child of the test below, which is the one that gets stuck and ends.
    const STUCK_CHILD_VAR: &str = "WIRECATCH_TEST_STUCK_QUIT";

    #[tokio::test]
    async fn stuck_file_work_does_not_keep_the_process_alive() {
        if std::env::var_os(STUCK_CHILD_VAR).is_some() {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let (started, is_started) = std::sync::mpsc::channel();
            drop(runtime.spawn_blocking(move || {
                started.send(()).unwrap();
                loop {
                    std::thread::park();
                }
            }));
            is_started.recv().unwrap();
            end_process();
        }
        let child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "quit::tests::stuck_file_work_does_not_keep_the_process_alive",
            ])
            .env(STUCK_CHILD_VAR, "1")
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let output =
            tokio::time::timeout(wirecatch_test_support::PATIENCE, child.wait_with_output())
                .await
                .expect("the process ends with its file work still stuck")
                .unwrap();
        assert!(output.status.success());
        // A misspelt test name runs nothing, and the harness reports that; the child never returns to it.
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("test result"),
            "the child did not reach the end of the process"
        );
    }

    #[test]
    fn closing_the_window_keeps_running_only_with_a_tray_and_close_to_tray_on() {
        assert!(keeps_running(true, Some(true)));
        assert!(keeps_running(true, None), "the default while opening is on");
        assert!(!keeps_running(true, Some(false)));
        assert!(!keeps_running(false, Some(true)), "no tray, no way back");
        assert!(!keeps_running(false, None));
    }
}
