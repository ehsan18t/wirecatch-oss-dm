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
/// # Errors
///
/// [`IpcError::AlreadyInUse`] means another Wirecatch already serves this user; this process
/// must then not open a second manager on the same database. Any other error means this
/// user's endpoint cannot be found or listened on.
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

/// Serves clients while the download manager could not open: every request is answered with
/// `reason`, so the extension tells the person what is wrong instead of waiting in silence.
pub async fn serve_unavailable(mut listener: Listener, reason: String) {
    loop {
        match listener.accept().await {
            Ok(connection) => {
                let reason = reason.clone();
                tokio::spawn(async move {
                    if let Err(e) = answer_unavailable(connection, &reason).await {
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

/// Answers every request on `stream` with an "app unavailable" error carrying `reason`, until
/// the client hangs up.
///
/// # Errors
///
/// Reading from `stream` failed or brought something that is not a frame, or writing the
/// answer failed.
pub async fn answer_unavailable<S>(mut stream: S, reason: &str) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use wirecatch_protocol::frame;
    use wirecatch_protocol::wire::{ErrorCode, Message, Outcome, ProtocolError};

    let message = format!(
        "Wirecatch is running but could not open its download list: {reason} Restart Wirecatch; if that does not help, see its tray icon for details."
    );
    loop {
        let request: Option<Message> = frame::read_async(&mut stream)
            .await
            .map_err(std::io::Error::other)?;
        let id = match request {
            None => return Ok(()),
            Some(Message::Request { id, .. }) => id,
            Some(_) => continue,
        };
        let answer = Message::Response {
            id,
            outcome: Outcome::Err(ProtocolError {
                code: ErrorCode::AppUnavailable,
                message: message.clone(),
            }),
        };
        frame::write_async(&mut stream, &answer)
            .await
            .map_err(std::io::Error::other)?;
    }
}

/// Where the bridge is: next to the app executable.
#[must_use]
pub fn bridge_path(app_exe: &Path) -> Option<PathBuf> {
    app_exe.parent().map(|dir| dir.join(BRIDGE_FILE_NAME))
}

/// The bridge to register for the app at `app_exe`. Under an AppImage (`appimage` set) that is
/// a private copy in `private_dir`; otherwise the bridge next to the app. A bridge in a
/// temporary location is never registered.
///
/// # Errors
///
/// No bridge can be found next to the app, the bridge is in a temporary folder, or (under an
/// AppImage) the private copy cannot be made; see [`crate::appimage::install_private_bridge`].
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
#[must_use]
pub fn registers_on_start() -> bool {
    !crate::host::DEV_BUILD
}

/// Runs `<bridge> install` on a thread of its own, so startup never waits on it. The command
/// is idempotent: it writes the same manifests and registry values each time, which also
/// repairs a registration that a browser update or a moved install broke. Under a per-machine
/// install it writes nothing for the account, and only removes the account's own registration
/// and any that names a bridge which no longer exists, since those would hide the machine one.
/// Failures are logged.
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
///
/// # Errors
///
/// The bridge is missing, cannot be started, or exits with a failure; the text carries what
/// it wrote to its error output.
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

    #[tokio::test]
    async fn without_a_manager_every_request_gets_a_clear_error() {
        use wirecatch_protocol::frame;
        use wirecatch_protocol::wire::{
            Call, ClientKind, ErrorCode, Hello, Message, Outcome, ProtocolError,
        };

        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let serving =
            tokio::spawn(async move { answer_unavailable(server, "The disk is full.").await });
        let hello = Message::Request {
            id: 7,
            call: Call::Hello(Hello {
                client: ClientKind::Extension,
                client_version: "1".into(),
                protocol_version: wirecatch_protocol::PROTOCOL_VERSION,
                browser: None,
                start_app: true,
            }),
        };
        frame::write_async(&mut client, &hello).await.unwrap();
        let answer: Option<Message> = frame::read_async(&mut client).await.unwrap();
        match answer {
            Some(Message::Response {
                id: 7,
                outcome: Outcome::Err(ProtocolError { code, message }),
            }) => {
                assert_eq!(code, ErrorCode::AppUnavailable);
                assert!(message.contains("The disk is full."), "{message}");
            }
            other => panic!("expected an error, got {other:?}"),
        }
        drop(client);
        serving.await.unwrap().unwrap();
    }

    #[test]
    fn a_missing_bridge_is_reported_plainly() {
        let err = run_bridge(Path::new("no-such-folder/wirecatch-bridge"), "install").unwrap_err();
        assert!(err.contains("missing"));
        assert!(!err.contains('\u{2014}'));
    }
}
