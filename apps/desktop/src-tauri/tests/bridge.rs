//! WP6 acceptance 3: the real bridge executable, started the way a browser starts it (with an
//! allowed extension origin), reaches the endpoint this app serves and gets a hello reply.
//!
//! The app side is the same code the app runs: `bridge::bind` on the real per-user endpoint and
//! `bridge::serve` with a manager. If a Wirecatch is already running for this user, it owns the
//! endpoint and the test says so instead of interfering with it.

mod common;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use common::Sandbox;
use tokio::process::Command;
use wirecatch_desktop_lib::bridge::{self, BRIDGE_FILE_NAME};
use wirecatch_ipc::IpcError;
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame;
use wirecatch_protocol::wire::{BrowserInfo, Call, ClientKind, Hello, Message, Outcome, Reply};

/// The development extension's origin, which the bridge allows (`config/extension-ids.json`).
const CHROME_ORIGIN: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";

/// The bridge executable of this workspace, built if needed. It lands in the same target
/// folder as this test.
fn bridge_exe() -> PathBuf {
    let test_exe = std::env::current_exe().unwrap();
    // <target>/debug/deps/<test>.exe -> <target>/debug
    let profile_dir = test_exe.parent().and_then(|p| p.parent()).unwrap();
    let target_dir = profile_dir.parent().unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new(cargo)
        .args(["build", "--quiet", "-p", "wirecatch-bridge", "--target-dir"])
        .arg(target_dir)
        .current_dir(&manifest)
        .status()
        .expect("could not run cargo to build the bridge");
    assert!(status.success(), "building the bridge failed");
    let exe = profile_dir.join(BRIDGE_FILE_NAME);
    assert!(exe.is_file(), "no bridge at {}", exe.display());
    exe
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bridge_started_like_a_browser_does_gets_a_hello_reply() {
    let exe = bridge_exe();
    let listener = match bridge::bind() {
        Ok(listener) => listener,
        Err(IpcError::AlreadyInUse { endpoint }) => {
            eprintln!(
                "SKIPPED: a running Wirecatch owns {endpoint}. Quit it and run this test again."
            );
            return;
        }
        Err(e) => panic!("could not listen: {e}"),
    };
    let sandbox = Sandbox::new();
    let manager = sandbox.open().await;
    let server = tokio::spawn(bridge::serve(listener, manager.clone()));

    let mut child = Command::new(&exe)
        .arg(CHROME_ORIGIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    let hello = Message::Request {
        id: 1,
        call: Call::Hello(Hello {
            client: ClientKind::Extension,
            client_version: "test".into(),
            protocol_version: PROTOCOL_VERSION,
            browser: Some(BrowserInfo {
                name: "chrome".into(),
                version: None,
            }),
        }),
    };
    frame::write_async(&mut stdin, &hello).await.unwrap();
    let reply: Option<Message> =
        tokio::time::timeout(Duration::from_secs(10), frame::read_async(&mut stdout))
            .await
            .expect("no reply from the bridge")
            .unwrap();
    match reply {
        Some(Message::Response {
            id: 1,
            outcome: Outcome::Ok(Reply::Hello(reply)),
        }) => {
            assert_eq!(reply.protocol_version, PROTOCOL_VERSION);
            assert_eq!(reply.app_version, env!("CARGO_PKG_VERSION"));
        }
        other => panic!("expected a hello reply, got {other:?}"),
    }

    // Closing stdin is the browser closing the port: the bridge exits.
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("the bridge did not exit")
        .unwrap();
    assert!(status.success());
    server.abort();
    manager.shutdown().await;
}
