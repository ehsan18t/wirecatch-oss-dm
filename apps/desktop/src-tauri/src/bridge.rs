//! The browser side: serving the bridge on the per-user endpoint (D14), and registering the
//! bridge with the browsers on every start (D19).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wirecatch_core::{Manager, serve_connection};
use wirecatch_ipc::{IpcError, Listener};

/// File name of the bridge, installed next to the app. The bridge starts the app from its own
/// folder, so both must sit side by side (D19).
pub use wirecatch_product::BRIDGE_FILE_NAME;

/// Pause after a failed accept, so a persistent error cannot spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Logs a failed accept and pauses before the next one, unless the failure concerned only
/// that client.
async fn after_failed_accept(error: &IpcError) {
    tracing::warn!("refused a browser connection: {error}");
    if let Some(pause) = backoff_after(error) {
        tokio::time::sleep(pause).await;
    }
}

/// How long to wait after a failed accept. A refused client (another account, a sandboxed
/// program) says nothing about the next one, so the next accept starts at once; any other
/// failure may repeat, and a pause keeps it from spinning.
fn backoff_after(error: &IpcError) -> Option<Duration> {
    match error {
        IpcError::PeerRefused { .. } => None,
        _ => Some(ACCEPT_BACKOFF),
    }
}

/// Starts listening on this build's endpoint, named with the endpoint key the listener keeps in
/// its channel's [key folder](wirecatch_ipc::key_folder), where [`crate::host::endpoint`] and the
/// bridge read it (D41.4). Must run inside the async runtime.
///
/// # Errors
///
/// [`IpcError::AlreadyInUse`] means another Wirecatch already serves this user; this process
/// must then not open a second manager on the same database. Any other error means this
/// user's endpoint cannot be found or listened on.
pub fn bind() -> Result<Listener, IpcError> {
    Listener::bind_keyed(
        crate::host::CHANNEL,
        &wirecatch_ipc::key_folder(crate::host::CHANNEL)?,
    )
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
            Err(e) => after_failed_accept(&e).await,
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
            Err(e) => after_failed_accept(&e).await,
        }
    }
}

/// Answers every request on `stream` with an "app unavailable" error carrying `reason`, until
/// the client hangs up. Only a request's id is read, so a call this build cannot read whole,
/// such as one from a newer extension, gets the same answer (D32), and a frame that is no
/// request is passed over.
///
/// # Errors
///
/// Reading from `stream` failed or brought something that is not a frame within the size
/// limit, or writing the answer failed.
pub async fn answer_unavailable<S>(mut stream: S, reason: &str) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use wirecatch_protocol::wire::{
        Envelope, ErrorCode, Message, MessageKind, Outcome, ProtocolError,
    };
    use wirecatch_protocol::{MIN_PROTOCOL_VERSION, frame};

    let message = format!(
        "Wirecatch is running but could not open its download list: {reason} Restart Wirecatch; if that does not help, see its tray icon for details."
    );
    loop {
        let Some(body) = frame::read_body_async(&mut stream)
            .await
            .map_err(std::io::Error::other)?
        else {
            return Ok(());
        };
        let Some(Envelope {
            kind: MessageKind::Request,
            id: Some(id),
        }) = Envelope::read(&body)
        else {
            continue;
        };
        let answer = Message::Response {
            id,
            outcome: Outcome::Err(ProtocolError {
                code: ErrorCode::AppUnavailable,
                message: message.clone(),
            }),
        };
        // No version is agreed here, so the answer is one the oldest version has.
        let Some(answer) = answer.as_of(MIN_PROTOCOL_VERSION) else {
            continue;
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

/// The bridge bundled with the app at `app_exe`.
///
/// # Errors
///
/// `app_exe` has no folder to look in.
pub fn bundled_bridge(app_exe: &Path) -> Result<PathBuf, String> {
    bridge_path(app_exe)
        .ok_or_else(|| "Could not find the browser bridge next to the app.".to_owned())
}

/// The bridge to register for the app installed at `app_exe`: the one next to it, never one in
/// a temporary location. An AppImage registers a private copy instead; see
/// [`crate::appimage::install_private_bridge`].
///
/// # Errors
///
/// No bridge can be found next to the app, or the bridge is in a temporary folder.
pub fn bridge_to_register(app_exe: &Path) -> Result<PathBuf, String> {
    let bundled = bundled_bridge(app_exe)?;
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

    // The pause was meant for errors that repeat; a refused client made the next one wait
    // too (W4I review N3).
    #[test]
    fn a_refused_client_does_not_pause_the_next_accept() {
        let refused = IpcError::PeerRefused {
            peer: "another user account (uid 1001)".into(),
        };
        assert_eq!(backoff_after(&refused), None);
        let failed = IpcError::Io {
            context: "A client failed to connect".into(),
            source: std::io::Error::from(std::io::ErrorKind::BrokenPipe),
        };
        assert_eq!(backoff_after(&failed), Some(ACCEPT_BACKOFF));
    }

    #[test]
    fn the_bridge_sits_next_to_the_app() {
        let app = Path::new("install").join(wirecatch_product::APP_FILE_NAME);
        let bridge = bridge_path(&app).unwrap();
        assert_eq!(bridge, Path::new("install").join(BRIDGE_FILE_NAME));
    }

    #[test]
    fn a_bridge_in_a_temporary_folder_is_never_registered() {
        let in_temp = std::env::temp_dir().join("x").join("wirecatch-app");
        assert!(bridge_to_register(&in_temp).is_err());
        let mount = Path::new("/tmp/.mount_Wirecatch/usr/bin/wirecatch-app");
        assert!(bridge_to_register(mount).is_err());
        let installed = Path::new("/opt/wirecatch/wirecatch-app");
        assert_eq!(
            bridge_to_register(installed).unwrap(),
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
        let app_exe = mount.join("wirecatch-app");
        assert!(bridge_to_register(&app_exe).is_err());
        let chosen = crate::appimage::install_private_bridge(
            &bundled_bridge(&app_exe).unwrap(),
            Path::new("/opt/Wirecatch.AppImage"),
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

        let (mut client, server) = tokio::io::duplex(64 * wirecatch_engine::units::KIB_USIZE);
        let serving =
            tokio::spawn(async move { answer_unavailable(server, "The disk is full.").await });
        let hello = Message::Request {
            id: 7,
            call: Call::Hello(Hello {
                client: ClientKind::Extension,
                client_version: "1".into(),
                protocol_version: wirecatch_protocol::MIN_PROTOCOL_VERSION,
                max_protocol_version: Some(wirecatch_protocol::PROTOCOL_VERSION),
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

    /// A request this app cannot read whole, such as a call from a newer extension, still gets
    /// the clear error, and the connection stays for the next one (D32.4).
    #[tokio::test]
    async fn without_a_manager_an_unreadable_request_does_not_end_the_connection() {
        use tokio::io::AsyncWriteExt;
        use wirecatch_protocol::frame;
        use wirecatch_protocol::wire::{Call, ErrorCode, Message, Outcome};

        let (mut client, server) = tokio::io::duplex(64 * wirecatch_engine::units::KIB_USIZE);
        let serving =
            tokio::spawn(async move { answer_unavailable(server, "The disk is full.").await });
        for body in [
            &br#"{"kind":"request","id":3,"call":{"method":"teleport","params":{"to":"mars"}}}"#[..],
            br#"{"kind":"request","id":4,"call":{"method":"add_download","params":{"url":3}}}"#,
            b"not json",
            br#"{"kind":"event","event":{"type":"queues_changed","data":[]}}"#,
        ] {
            let mut framed = frame::length_prefix(body.len()).unwrap().to_vec();
            framed.extend_from_slice(body);
            client.write_all(&framed).await.unwrap();
        }
        frame::write_async(
            &mut client,
            &Message::Request {
                id: 5,
                call: Call::ListQueues,
            },
        )
        .await
        .unwrap();
        for expected in [3, 4, 5] {
            let answer: Option<Message> = frame::read_async(&mut client).await.unwrap();
            match answer {
                Some(Message::Response {
                    id,
                    outcome: Outcome::Err(error),
                }) if id == expected => assert_eq!(error.code, ErrorCode::AppUnavailable),
                other => panic!("expected an error for {expected}, got {other:?}"),
            }
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
