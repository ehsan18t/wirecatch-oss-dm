//! The browser side: serving the bridge on the per-user endpoint (D14), and registering the
//! bridge with the browsers on every start (D19).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wirecatch_core::{Manager, serve_connection};
use wirecatch_ipc::{IpcError, Listener};

/// File name of the bridge, installed next to `wirecatch-app`. The bridge starts the app from
/// its own folder, so both must sit side by side.
pub const BRIDGE_FILE_NAME: &str = if cfg!(windows) {
    "wirecatch-bridge.exe"
} else {
    "wirecatch-bridge"
};

/// Set to `1` to register the bridge from a debug build. Debug builds skip it by default, so
/// running the app from a source checkout does not point every browser at that checkout.
pub const REGISTER_VAR: &str = "WIRECATCH_REGISTER_BRIDGE";

/// Pause after a failed accept, so a persistent error cannot spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Starts listening on this user's endpoint. Must run inside the async runtime.
///
/// [`IpcError::AlreadyInUse`] means another Wirecatch already serves this user; this process
/// must then not open a second manager on the same database.
pub fn bind() -> Result<Listener, IpcError> {
    Listener::bind(&wirecatch_ipc::endpoint()?)
}

/// Serves every client that connects (the bridge, later the CLI) until the task is stopped.
pub async fn serve(mut listener: Listener, manager: Manager) {
    tracing::info!("serving browser connections at {}", listener.endpoint());
    loop {
        match listener.accept().await {
            Ok(connection) => {
                let manager = manager.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_connection(manager, connection).await {
                        tracing::debug!("a browser connection ended: {e}");
                    }
                });
            }
            Err(e) => {
                tracing::warn!("refused a browser connection: {e}");
                tokio::time::sleep(ACCEPT_BACKOFF).await;
            }
        }
    }
}

/// Where the bridge is: next to the app executable.
pub fn bridge_path(app_exe: &Path) -> Option<PathBuf> {
    app_exe.parent().map(|dir| dir.join(BRIDGE_FILE_NAME))
}

/// Whether this build registers the bridge on start.
pub fn registers_on_start() -> bool {
    !cfg!(debug_assertions) || std::env::var(REGISTER_VAR).is_ok_and(|v| v == "1")
}

/// Runs `<bridge> install` on a thread of its own, so startup never waits on it. The command
/// is idempotent: it writes the same manifests and registry values each time, which also
/// repairs a registration that a browser update or a moved install broke. Failures are logged.
pub fn register_in_background(bridge: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("register-bridge".into())
        .spawn(move || match run_bridge(&bridge, "install") {
            Ok(()) => tracing::info!("registered the browser bridge {}", bridge.display()),
            Err(e) => tracing::warn!("{e}"),
        });
    if let Err(e) = spawned {
        tracing::warn!("could not register the browser bridge: {e}");
    }
}

/// Runs the bridge with one subcommand (`install` or `uninstall`) and waits for it.
pub fn run_bridge(bridge: &Path, subcommand: &str) -> Result<(), String> {
    if !bridge.is_file() {
        return Err(format!(
            "The browser bridge is missing ({}). Reinstall Wirecatch so browser downloads reach it.",
            bridge.display()
        ));
    }
    let mut command = Command::new(bridge);
    command
        .arg(subcommand)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Keeps a console window from flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("Could not run {}: {e}", bridge.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{} {subcommand} failed ({}): {}",
        bridge.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_sits_next_to_the_app() {
        let app = Path::new("install").join(if cfg!(windows) {
            "wirecatch-app.exe"
        } else {
            "wirecatch-app"
        });
        let bridge = bridge_path(&app).unwrap();
        assert_eq!(bridge, Path::new("install").join(BRIDGE_FILE_NAME));
    }

    #[test]
    fn a_missing_bridge_is_reported_plainly() {
        let err = run_bridge(Path::new("no-such-folder/wirecatch-bridge"), "install").unwrap_err();
        assert!(err.contains("missing"));
        assert!(!err.contains('\u{2014}'));
    }
}
