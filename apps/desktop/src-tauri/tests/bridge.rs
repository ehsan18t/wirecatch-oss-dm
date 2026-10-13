//! WP6 acceptance 3: the real bridge executable, started the way a browser starts it (with an
//! allowed extension origin), reaches the endpoint this app serves and gets a hello reply.
//!
//! The app side is the code the app runs (`bridge::serve` with a manager), on a private
//! endpoint of the test's own. The bridge is pointed at it with its test-only override, so the
//! test never depends on, or disturbs, a Wirecatch running for this user, and can never pass by
//! talking to one.

mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;

use common::Sandbox;
use tokio::process::Command;
use wirecatch_desktop_lib::bridge::{self, BRIDGE_FILE_NAME};
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_product::CHROMIUM_EXTENSION_ORIGIN;
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame;
use wirecatch_protocol::wire::{BrowserInfo, Call, ClientKind, Hello, Message, Outcome, Reply};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir};

/// The bridge's test-only overrides (`crates/bridge/src/app.rs`).
const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";

/// The bridge executable of this workspace, built with its test-only overrides (as the
/// workspace's own test command builds it). It lands in the same target folder as this test.
fn bridge_exe() -> TestResult<PathBuf> {
    let test_exe = std::env::current_exe()?;
    // <target>/debug/deps/<test>.exe -> <target>/debug
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .ok_or("the test executable is not in a target folder")?;
    let target_dir = profile_dir
        .parent()
        .ok_or("the profile folder has no parent")?;
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
        .status()?;
    if !status.success() {
        return Err("building the bridge failed".into());
    }
    let exe = profile_dir.join(BRIDGE_FILE_NAME);
    if !exe.is_file() {
        return Err(format!("no bridge at {}", exe.display()).into());
    }
    Ok(exe)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bridge_started_like_a_browser_does_gets_a_hello_reply() -> TestResult {
    let exe = bridge_exe()?;
    // A private endpoint nobody else uses, in a folder only this user can enter.
    let dir = private_dir("desktop-bridge")?;
    let endpoint = Endpoint::from_raw(dir.endpoint_name());
    let listener = Listener::bind(&endpoint)?;
    let sandbox = Sandbox::new()?;
    let manager = sandbox.open().await?;
    let server = tokio::spawn(bridge::serve(listener, manager.clone()));

    let mut child = Command::new(&exe)
        .arg(CHROMIUM_EXTENSION_ORIGIN)
        .env(ENDPOINT_VAR, endpoint.as_os_str())
        // Whatever happens, the bridge must not start a real app.
        .env(LAUNCH_VAR, r#"["wirecatch-desktop-test-no-such-program"]"#)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("the bridge has no stdin pipe")?;
    let mut stdout = child.stdout.take().ok_or("the bridge has no stdout pipe")?;

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
            start_app: true,
        }),
    };
    frame::write_async(&mut stdin, &hello).await?;
    let reply: Option<Message> = tokio::time::timeout(PATIENCE, frame::read_async(&mut stdout))
        .await
        .map_err(|_| "no reply from the bridge")??;
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
    let status = tokio::time::timeout(PATIENCE, child.wait())
        .await
        .map_err(|_| "the bridge did not exit")??;
    assert!(status.success());
    server.abort();
    manager.shutdown().await;
    Ok(())
}
