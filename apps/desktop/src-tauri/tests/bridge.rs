//! WP6 acceptance 3: the real bridge executable, started the way a browser starts it (with an
//! allowed extension origin), reaches the endpoint this app serves and gets a hello reply.
//!
//! The app side is the code the app runs (`bridge::serve` with a manager), on a private
//! endpoint of the test's own. The bridge is pointed at it with its test-only override, so the
//! test never depends on, or disturbs, a Wirecatch running for this user, and can never pass by
//! talking to one.

mod common;

use std::process::Stdio;

use common::Sandbox;
use tokio::process::Command;
use wirecatch_desktop_lib::bridge;
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_product::{CHROMIUM_EXTENSION_ORIGIN, LOG_FOLDER_VARIABLE};
use wirecatch_protocol::frame;
use wirecatch_protocol::wire::{BrowserInfo, Call, ClientKind, Hello, Message, Outcome, Reply};
use wirecatch_protocol::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION};
use wirecatch_test_support::bridge::{ENDPOINT_VAR, LAUNCH_VAR, bridge_exe};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bridge_started_like_a_browser_does_gets_a_hello_reply() -> TestResult {
    let exe = bridge_exe();
    // A private endpoint nobody else uses, in a folder only this user can enter.
    let dir = private_dir("desktop-bridge")?;
    let endpoint = Endpoint::from_raw(dir.endpoint_name());
    let listener = Listener::bind(&endpoint)?;
    let sandbox = Sandbox::new()?;
    let manager = sandbox.open().await?;
    let server = tokio::spawn(bridge::serve(listener, manager.clone()));

    let mut child = Command::new(exe)
        .arg(CHROMIUM_EXTENSION_ORIGIN)
        .env(ENDPOINT_VAR, endpoint.as_os_str())
        // Whatever happens, the bridge must not start a real app.
        .env(LAUNCH_VAR, r#"["wirecatch-desktop-test-no-such-program"]"#)
        // Its log file goes in the test's folder, not the person's.
        .env(LOG_FOLDER_VARIABLE, dir.path())
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
            protocol_version: MIN_PROTOCOL_VERSION,
            max_protocol_version: Some(PROTOCOL_VERSION),
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
