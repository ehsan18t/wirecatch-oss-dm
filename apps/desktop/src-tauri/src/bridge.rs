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

/// Pause after a failed accept, so a persistent error cannot spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Starts listening on this build's endpoint (see [`crate::host::endpoint`]). Must run inside
/// the async runtime.
///
/// [`IpcError::AlreadyInUse`] means another Wirecatch already serves this user; this process
/// must then not open a second manager on the same database.
pub fn bind() -> Result<Listener, IpcError> {
    Listener::bind(&crate::host::endpoint()?)
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

/// The bridge to register for the app at `app_exe`. Under an AppImage (`appimage` set) that is
/// a private copy in `private_dir`; otherwise the bridge next to the app. A bridge in a
/// temporary location is never registered.
pub fn bridge_to_register(
    app_exe: &Path,
    appimage: Option<&Path>,
    private_dir: &Path,
) -> Result<PathBuf, String> {
    let bundled = bridge_path(app_exe)
        .ok_or_else(|| "Could not find the browser bridge next to the app.".to_owned())?;
    if let Some(appimage) = appimage {
        #[cfg(unix)]
        return crate::appimage::install_private_bridge(&bundled, appimage, private_dir);
        #[cfg(not(unix))]
        {
            let _ = (appimage, private_dir);
        }
    }
    if crate::appimage::is_temporary(&bundled) {
        return Err(format!(
            "The browser bridge is in a temporary folder ({}), so it is not registered. Install Wirecatch, or run it from a permanent folder.",
            bundled.display()
        ));
    }
    Ok(bundled)
}

/// Whether this build registers the bridge on start. Debug builds never do: they serve their
/// own endpoint, which the registered bridge does not use, and registering them would point
/// every browser at a source checkout instead of the installed app.
pub fn registers_on_start() -> bool {
    !crate::host::DEV_BUILD
}

/// Runs `<bridge> install` on a thread of its own, so startup never waits on it. The command
/// is idempotent: it writes the same manifests and registry values each time, which also
/// repairs a registration that a browser update or a moved install broke. Failures are logged.
///
/// `find` picks the bridge to register, on that thread (under an AppImage it copies one out
/// first, see [`crate::appimage`]); `None` registers nothing.
pub fn register_in_background(find: impl FnOnce() -> Option<PathBuf> + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("register-bridge".into())
        .spawn(move || {
            let Some(bridge) = find() else {
                return;
            };
            match run_bridge(&bridge, "install") {
                Ok(()) => tracing::info!("registered the browser bridge {}", bridge.display()),
                Err(e) => tracing::warn!("{e}"),
            }
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
    fn a_bridge_in_a_temporary_folder_is_never_registered() {
        let private = Path::new("/home/ana/.local/share/wirecatch/bin");
        let in_temp = std::env::temp_dir().join("x").join("wirecatch-app");
        assert!(bridge_to_register(&in_temp, None, private).is_err());
        let mount = Path::new("/tmp/.mount_Wirecatch/usr/bin/wirecatch-app");
        assert!(bridge_to_register(mount, None, private).is_err());
        let installed = Path::new("/opt/wirecatch/wirecatch-app");
        assert_eq!(
            bridge_to_register(installed, None, private).unwrap(),
            Path::new("/opt/wirecatch").join(BRIDGE_FILE_NAME)
        );
    }

    #[cfg(unix)]
    #[test]
    fn under_an_appimage_the_private_copy_is_registered() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join(".mount_Wirecatch").join("usr").join("bin");
        std::fs::create_dir_all(&mount).unwrap();
        std::fs::write(mount.join(BRIDGE_FILE_NAME), b"bridge").unwrap();
        let private = dir.path().join("data").join("bin");
        let chosen = bridge_to_register(
            &mount.join("wirecatch-app"),
            Some(Path::new("/opt/Wirecatch.AppImage")),
            &private,
        )
        .unwrap();
        assert_eq!(chosen, private.join(BRIDGE_FILE_NAME));
    }

    #[test]
    fn a_missing_bridge_is_reported_plainly() {
        let err = run_bridge(Path::new("no-such-folder/wirecatch-bridge"), "install").unwrap_err();
        assert!(err.contains("missing"));
        assert!(!err.contains('\u{2014}'));
    }
}
