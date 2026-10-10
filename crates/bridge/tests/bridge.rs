//! End-to-end tests of the `wirecatch-bridge` executable: a real process with
//! piped stdin and stdout, talking to a fake app on a private test endpoint.
//!
//! They rely on the overrides `WIRECATCH_BRIDGE_TEST_ENDPOINT` and
//! `WIRECATCH_BRIDGE_TEST_LAUNCH` (see `src/app.rs`), which exist only with the
//! `test-overrides` feature; Cargo.toml makes this test require it. Run with
//! `cargo test -p wirecatch-bridge --features test-overrides`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{sleep, timeout};
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::frame::{self, read_async, write_async};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Hello, HelloReply, Message, Outcome, ProtocolError, Reply,
};

const BRIDGE: &str = env!("CARGO_BIN_EXE_wirecatch-bridge");
const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";
const CHROME_ORIGIN: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";
const FIREFOX_ID: &str = "extension@wirecatch.app";

/// Read by `fake_app_process`, which the bridge starts in place of the app.
const FAKE_APP_ENDPOINT: &str = "WIRECATCH_TEST_FAKE_APP_ENDPOINT";
const FAKE_APP_MODE: &str = "WIRECATCH_TEST_FAKE_APP_MODE";

const PATIENCE: Duration = Duration::from_secs(10);

/// A private endpoint for one test. On Linux it lives in a new 0700 folder.
struct TestEndpoint {
    endpoint: Endpoint,
    dir: Option<PathBuf>,
}

impl TestEndpoint {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let tag = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        #[cfg(windows)]
        {
            Self {
                endpoint: Endpoint::from_raw(format!(r"\\.\pipe\wirecatch-bridge-it-{tag}")),
                dir: None,
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            let dir = std::env::temp_dir().join(format!("wirecatch-bridge-it-{tag}"));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                endpoint: Endpoint::from_raw(dir.join("app.sock")),
                dir: Some(dir),
            }
        }
    }
}

impl Drop for TestEndpoint {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The bridge, pointed at `endpoint`. By default "starting the app" runs a
/// program that does not exist, so no test ever starts anything real.
fn bridge(args: &[&str], endpoint: &Endpoint) -> Command {
    let missing = std::env::temp_dir().join(format!(
        "wirecatch-no-such-app-{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    let launch = serde_json::json!([missing]).to_string();
    let mut command = Command::new(BRIDGE);
    command
        .args(args)
        .env(ENDPOINT_VAR, endpoint.as_os_str())
        .env(LAUNCH_VAR, launch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
}

fn start(mut command: Command) -> Running {
    let mut child = command.spawn().unwrap();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    Running {
        child,
        stdin,
        stdout,
    }
}

impl Running {
    async fn send(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).await.unwrap();
        stdin.flush().await.unwrap();
    }

    /// Closes stdin (what the browser does when the port closes) and waits for a
    /// clean exit.
    async fn close_and_wait(&mut self) {
        drop(self.stdin.take());
        let status = timeout(PATIENCE, self.child.wait())
            .await
            .expect("the bridge did not exit after stdin closed")
            .unwrap();
        assert!(status.success(), "{status}");
    }
}

fn hello(id: u64) -> Vec<u8> {
    frame::encode(&Message::Request {
        id,
        call: Call::Hello(Hello {
            client: ClientKind::Extension,
            client_version: "test".into(),
            protocol_version: PROTOCOL_VERSION,
            browser: None,
        }),
    })
    .unwrap()
}

fn raw(body: &str) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// Reads one whole frame (length prefix included), or `None` at end of stream.
async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Option<Vec<u8>> {
    timeout(PATIENCE, async {
        let mut header = [0u8; 4];
        match reader.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Err(e) => panic!("{e}"),
        }
        let len = frame::body_len(header).unwrap();
        let mut bytes = header.to_vec();
        bytes.resize(4 + len, 0);
        reader.read_exact(&mut bytes[4..]).await.unwrap();
        Some(bytes)
    })
    .await
    .expect("no frame arrived in time")
}

fn decode(bytes: &[u8]) -> Message {
    frame::decode(&bytes[4..]).unwrap()
}

fn assert_unavailable(bytes: &[u8], expected_id: u64) -> String {
    match decode(bytes) {
        Message::Response {
            id,
            outcome:
                Outcome::Err(ProtocolError {
                    code: ErrorCode::AppUnavailable,
                    message,
                }),
        } if id == expected_id => message,
        other => panic!("expected AppUnavailable for {expected_id}, got {other:?}"),
    }
}

#[tokio::test]
async fn messages_and_events_pass_through_unchanged() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));

    // Unusual spacing and a field this build does not know: the bridge must not
    // re-encode anything.
    let request = raw(
        r#"{"kind":"request", "id":1,"call":{"method":"hello","params":{"client":"extension","client_version":"1","protocol_version":1,"browser":{"name":"chrome","version":"130"}}},"from_a_newer_extension":true}"#,
    );
    bridge.send(&request).await;
    let mut app = timeout(PATIENCE, listener.accept()).await.unwrap().unwrap();
    assert_eq!(read_frame(&mut app).await, Some(request));

    let response = raw(
        r#"{"kind":"response","id":1,"outcome":{"status":"ok","value":{"type":"hello","data":{"app_version":"0.1.0","protocol_version":1}}}, "from_a_newer_app":[1,2]}"#,
    );
    let event = raw(r#"{"kind":"event","event":{"type":"queues_changed","data":[]}}"#);
    app.write_all(&response).await.unwrap();
    app.write_all(&event).await.unwrap();
    assert_eq!(read_frame(&mut bridge.stdout).await, Some(response));
    assert_eq!(read_frame(&mut bridge.stdout).await, Some(event));

    // More traffic in both directions after the handshake.
    for id in 2..=4 {
        let request = frame::encode(&Message::Request {
            id,
            call: Call::ListQueues,
        })
        .unwrap();
        bridge.send(&request).await;
        assert_eq!(read_frame(&mut app).await, Some(request));
        let response = frame::encode(&Message::Response {
            id,
            outcome: Outcome::Ok(Reply::Queues(Vec::new())),
        })
        .unwrap();
        app.write_all(&response).await.unwrap();
        assert_eq!(read_frame(&mut bridge.stdout).await, Some(response));
    }

    bridge.close_and_wait().await;
    assert_eq!(read_frame(&mut bridge.stdout).await, None);
    // The app sees the bridge leave.
    assert_eq!(read_frame(&mut app).await, None);
}

#[tokio::test]
async fn a_firefox_caller_is_served() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let manifest = std::env::temp_dir().join("app.wirecatch.bridge.json");
    let manifest = manifest.to_str().unwrap();
    let mut bridge = start(bridge(&[manifest, FIREFOX_ID], &test.endpoint));
    bridge.send(&hello(5)).await;
    let mut app = timeout(PATIENCE, listener.accept()).await.unwrap().unwrap();
    assert_eq!(read_frame(&mut app).await, Some(hello(5)));
    let answer = frame::encode(&Message::Response {
        id: 5,
        outcome: Outcome::Ok(Reply::Done),
    })
    .unwrap();
    app.write_all(&answer).await.unwrap();
    assert_eq!(read_frame(&mut bridge.stdout).await, Some(answer));
    bridge.close_and_wait().await;
}

#[tokio::test]
async fn callers_that_are_not_allowed_are_refused() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let manifest = std::env::temp_dir().join("app.wirecatch.bridge.json");
    let manifest = manifest.to_str().unwrap();
    let refused: &[&[&str]] = &[
        &[],
        &["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
        &["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki"],
        &[manifest, "evil@example.com"],
        &[manifest],
        &["moz-extension://extension@wirecatch.app/"],
    ];
    for args in refused {
        let mut bridge = start(bridge(args, &test.endpoint));
        // The bridge may already be gone, so a failed write is fine.
        if let Some(stdin) = bridge.stdin.as_mut() {
            let _ = stdin.write_all(&hello(1)).await;
        }
        let status = timeout(PATIENCE, bridge.child.wait())
            .await
            .expect("a refused bridge must exit")
            .unwrap();
        assert!(!status.success(), "{args:?} was not refused");
        let mut stdout = Vec::new();
        bridge.stdout.read_to_end(&mut stdout).await.unwrap();
        assert!(stdout.is_empty(), "{args:?} got an answer");
        let mut stderr = String::new();
        bridge
            .child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .await
            .unwrap();
        assert!(stderr.contains("refused to start"), "{stderr}");
    }
    // None of them reached the app.
    assert!(
        timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err()
    );
}

/// The bridge starts this test binary in place of the app (through
/// `WIRECATCH_BRIDGE_TEST_LAUNCH`). It does nothing in a normal test run, and it
/// exits as soon as its job is done so it never holds the test executable open
/// for long (a running copy blocks the next rebuild on Windows).
#[test]
#[ignore = "helper process started by the bridge in other tests"]
fn fake_app_process() {
    let Some(endpoint) = std::env::var_os(FAKE_APP_ENDPOINT) else {
        return;
    };
    // "never_listen": the app starts but never listens. Exiting at once is the
    // same thing as far as the bridge can tell.
    if std::env::var(FAKE_APP_MODE).as_deref() == Ok("never_listen") {
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        // A real app takes a moment to come up.
        sleep(Duration::from_millis(300)).await;
        let mut listener = Listener::bind(&Endpoint::from_raw(endpoint)).unwrap();
        let Ok(Ok(mut conn)) = timeout(PATIENCE, listener.accept()).await else {
            return;
        };
        while let Ok(Some(msg)) = read_async::<Message>(&mut conn).await {
            if let Message::Request { id, .. } = msg {
                let reply = Message::Response {
                    id,
                    outcome: Outcome::Ok(Reply::Hello(HelloReply {
                        app_version: "fake".into(),
                        protocol_version: PROTOCOL_VERSION,
                    })),
                };
                if write_async(&mut conn, &reply).await.is_err() {
                    break;
                }
            }
        }
        // Like the real app, keep running after the bridge leaves, until the test
        // has checked that (by connecting once more), then exit.
        let _ = timeout(PATIENCE, listener.accept()).await;
    });
}

fn launch_fake_app(mode: &str, endpoint: &Endpoint, command: &mut Command) {
    let test_exe = std::env::current_exe().unwrap();
    let launch = serde_json::json!([
        test_exe,
        "fake_app_process",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1"
    ]);
    command
        .env(LAUNCH_VAR, launch.to_string())
        .env(FAKE_APP_ENDPOINT, endpoint.as_os_str())
        .env(FAKE_APP_MODE, mode);
}

/// The bridge's own 5 second budget, plus room for process start-up on a slow
/// or cold machine. The bridge measures its budget itself and gives up with
/// `AppUnavailable` when it runs out, so a successful answer already proves the
/// app was reached within 5 seconds; the wall clock bound only catches hangs.
const BUDGET_WITH_STARTUP_MARGIN: Duration = Duration::from_secs(8);

#[tokio::test]
async fn the_app_is_started_when_it_is_not_running() {
    let test = TestEndpoint::new();
    let mut command = bridge(&[CHROME_ORIGIN], &test.endpoint);
    launch_fake_app("serve", &test.endpoint, &mut command);
    let mut bridge = start(command);

    let started = Instant::now();
    bridge.send(&hello(7)).await;
    let answer = read_frame(&mut bridge.stdout).await.unwrap();
    let waited = started.elapsed();
    // A real answer, not AppUnavailable: the bridge connected within its budget.
    assert_eq!(
        decode(&answer),
        Message::Response {
            id: 7,
            outcome: Outcome::Ok(Reply::Hello(HelloReply {
                app_version: "fake".into(),
                protocol_version: PROTOCOL_VERSION,
            })),
        }
    );
    assert!(waited < BUDGET_WITH_STARTUP_MARGIN, "took {waited:?}");
    eprintln!("first answer through a freshly started app took {waited:?}");

    // When the browser closes the port, the browser side of the pipe closes even
    // though the app the bridge started keeps running (it must not inherit the
    // browser's pipe handles).
    bridge.close_and_wait().await;
    let eof = timeout(Duration::from_secs(2), read_frame(&mut bridge.stdout)).await;
    assert_eq!(eof, Ok(None), "stdout must close when the bridge exits");
    // Still running: it accepts this connection, and then exits.
    assert!(
        wirecatch_ipc::connect(&test.endpoint).await.is_ok(),
        "the fake app should still be running"
    );
}

#[tokio::test]
async fn app_unavailable_when_the_app_cannot_be_started() {
    let test = TestEndpoint::new();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));
    let started = Instant::now();
    bridge.send(&hello(1)).await;
    bridge.send(&hello(2)).await;
    let first = read_frame(&mut bridge.stdout).await.unwrap();
    let second = read_frame(&mut bridge.stdout).await.unwrap();
    let message = assert_unavailable(&first, 1);
    assert_unavailable(&second, 2);
    assert!(message.contains("Reinstall Wirecatch"), "{message}");
    assert!(started.elapsed() < Duration::from_secs(2), "fails fast");
    // No relaunch loop: the bridge stays up answering until the browser leaves.
    bridge.send(&hello(3)).await;
    assert_unavailable(&read_frame(&mut bridge.stdout).await.unwrap(), 3);
    bridge.close_and_wait().await;
}

#[tokio::test]
async fn app_unavailable_when_the_app_never_starts_listening() {
    let test = TestEndpoint::new();
    let mut command = bridge(&[CHROME_ORIGIN], &test.endpoint);
    launch_fake_app("never_listen", &test.endpoint, &mut command);
    let mut bridge = start(command);
    let started = Instant::now();
    bridge.send(&hello(3)).await;
    let answer = read_frame(&mut bridge.stdout).await.unwrap();
    let waited = started.elapsed();
    let message = assert_unavailable(&answer, 3);
    assert!(message.contains("did not respond"), "{message}");
    assert!(
        waited >= Duration::from_millis(4900) && waited < BUDGET_WITH_STARTUP_MARGIN,
        "answered after {waited:?}, expected about 5 seconds"
    );
    bridge.close_and_wait().await;
}

#[tokio::test]
async fn closing_the_connection_while_the_app_starts_exits_promptly() {
    let test = TestEndpoint::new();
    let mut command = bridge(&[CHROME_ORIGIN], &test.endpoint);
    launch_fake_app("never_listen", &test.endpoint, &mut command);
    let mut bridge = start(command);
    bridge.send(&hello(1)).await;
    // Give the bridge time to read the message and start waiting for the app.
    sleep(Duration::from_millis(300)).await;
    let closed = Instant::now();
    bridge.close_and_wait().await;
    let took = closed.elapsed();
    assert!(
        took < Duration::from_secs(2),
        "took {took:?}; it must not sit out the 5 second start budget"
    );
}

#[tokio::test]
async fn the_bridge_exits_when_the_app_goes_away_so_the_extension_can_reconnect() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));
    bridge.send(&hello(1)).await;
    let mut app = timeout(PATIENCE, listener.accept()).await.unwrap().unwrap();
    read_frame(&mut app).await.unwrap();
    // The user quits Wirecatch with the request unanswered.
    drop(app);
    drop(listener);
    assert_unavailable(&read_frame(&mut bridge.stdout).await.unwrap(), 1);
    // Then the bridge exits by itself, with stdin still open, so the browser
    // sees the port disconnect.
    let status = timeout(PATIENCE, bridge.child.wait())
        .await
        .expect("the bridge must exit after the app goes away")
        .unwrap();
    assert!(status.success(), "{status}");
    assert_eq!(read_frame(&mut bridge.stdout).await, None);
}

#[tokio::test]
async fn an_oversized_message_is_skipped_and_the_session_goes_on() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));

    let len = frame::MAX_FRAME_LEN + 1;
    let mut oversized = (len as u32).to_le_bytes().to_vec();
    oversized.resize(4 + len, b' ');
    bridge.send(&oversized).await;
    bridge.send(&hello(2)).await;

    // Only the message within the limit reaches the app, and it is answered.
    let mut app = timeout(PATIENCE, listener.accept()).await.unwrap().unwrap();
    assert_eq!(read_frame(&mut app).await, Some(hello(2)));
    let answer = frame::encode(&Message::Response {
        id: 2,
        outcome: Outcome::Ok(Reply::Done),
    })
    .unwrap();
    app.write_all(&answer).await.unwrap();
    assert_eq!(read_frame(&mut bridge.stdout).await, Some(answer));
    bridge.close_and_wait().await;
}

#[tokio::test]
async fn exits_when_the_browser_closes_the_connection_before_any_message() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));
    bridge.close_and_wait().await;
    assert!(
        timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "the bridge must not contact the app without a message"
    );
}

/// Working set and private (committed, not shared) bytes of a process.
#[cfg(windows)]
fn memory_of(pid: u32) -> (usize, usize) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    // SAFETY: plain Win32 calls on a handle we open and close here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        assert!(!process.is_null());
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let ok = K32GetProcessMemoryInfo(
            process,
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        CloseHandle(process);
        assert_ne!(ok, 0);
        (counters.WorkingSetSize, counters.PagefileUsage)
    }
}

/// Resident and anonymous (private) resident bytes of a process.
#[cfg(target_os = "linux")]
fn memory_of(pid: u32) -> (usize, usize) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let field = |name: &str| -> usize {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kb| kb.parse::<usize>().ok())
            .unwrap()
            * 1024
    };
    (field("VmRSS:"), field("RssAnon:"))
}

#[tokio::test]
async fn idle_memory_stays_under_10_mb() {
    let test = TestEndpoint::new();
    let mut listener = Listener::bind(&test.endpoint).unwrap();
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test.endpoint));
    bridge.send(&hello(1)).await;
    let mut app = timeout(PATIENCE, listener.accept()).await.unwrap().unwrap();
    read_frame(&mut app).await.unwrap();
    let answer = frame::encode(&Message::Response {
        id: 1,
        outcome: Outcome::Ok(Reply::Done),
    })
    .unwrap();
    app.write_all(&answer).await.unwrap();
    read_frame(&mut bridge.stdout).await.unwrap();

    // Connected and idle.
    sleep(Duration::from_secs(1)).await;
    let pid = bridge.child.id().unwrap();
    let (resident, private) = memory_of(pid);
    let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
    eprintln!(
        "idle bridge ({} build): resident {:.2} MiB, private {:.2} MiB",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        mib(resident),
        mib(private)
    );
    const LIMIT: usize = 10 * 1024 * 1024;
    assert!(resident < LIMIT, "resident {resident} bytes");
    assert!(private < LIMIT, "private {private} bytes");
    bridge.close_and_wait().await;
}

/// When the launcher itself cannot be set up, the request still gets an answer.
#[tokio::test]
async fn a_broken_launch_override_still_answers() {
    let test = TestEndpoint::new();
    let mut command = bridge(&[CHROME_ORIGIN], &test.endpoint);
    command.env(LAUNCH_VAR, OsString::from("[]"));
    let mut bridge = start(command);
    bridge.send(&hello(9)).await;
    assert_unavailable(&read_frame(&mut bridge.stdout).await.unwrap(), 9);
    bridge.close_and_wait().await;
}
