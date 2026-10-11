//! WP6 acceptance 3: the real bridge executable, started the way a browser starts it (with an
//! allowed extension origin), reaches the endpoint this app serves and gets a hello reply.
//!
//! The app side is the code the app runs (`bridge::serve` with a manager), on a private
//! endpoint of the test's own. The bridge is pointed at it with its test-only override, so the
//! test never depends on, or disturbs, a Wirecatch running for this user, and can never pass by
//! talking to one.

mod common;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use common::Sandbox;
use tokio::process::Command;
use wirecatch_desktop_lib::bridge::{self, BRIDGE_FILE_NAME};
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame;
use wirecatch_protocol::wire::{BrowserInfo, Call, ClientKind, Hello, Message, Outcome, Reply};

/// The extension's Chromium origin, which every bridge allows (`config/extension-ids.json`).
const CHROME_ORIGIN: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";

/// The bridge's test-only overrides (`crates/bridge/src/app.rs`).
const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";

/// A private endpoint nobody else uses, and the folder that must outlive it on Linux.
fn private_endpoint() -> (Endpoint, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let endpoint = if cfg!(windows) {
        Endpoint::from_raw(format!(r"\\.\pipe\wirecatch-desktop-test-{unique}"))
    } else {
        // tempfile makes the folder 0700, as the listener requires.
        Endpoint::from_raw(dir.path().join("app.sock"))
    };
    (endpoint, dir)
}

/// The bridge executable of this workspace, built with its test-only overrides (as the
/// workspace's own test command builds it). It lands in the same target folder as this test.
fn bridge_exe() -> PathBuf {
    let test_exe = std::env::current_exe().unwrap();
    // <target>/debug/deps/<test>.exe -> <target>/debug
    let profile_dir = test_exe.parent().and_then(|p| p.parent()).unwrap();
    let target_dir = profile_dir.parent().unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new(cargo)
        .args([
            "build",
            "--quiet",
            "-p",
            "wirecatch-bridge",
            "--features",
            "wirecatch-bridge/test-overrides",
            "--target-dir",
        ])
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
    let (endpoint, _dir) = private_endpoint();
    let listener = Listener::bind(&endpoint).expect("could not listen on a private endpoint");
    let sandbox = Sandbox::new();
    let manager = sandbox.open().await;
    let server = tokio::spawn(bridge::serve(listener, manager.clone()));

    let mut child = Command::new(&exe)
        .arg(CHROME_ORIGIN)
        .env(ENDPOINT_VAR, endpoint.as_os_str())
        // Whatever happens, the bridge must not start a real app.
        .env(LAUNCH_VAR, r#"["wirecatch-desktop-test-no-such-program"]"#)
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
