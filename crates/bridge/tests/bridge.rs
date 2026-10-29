//! End-to-end tests of the `wirecatch-bridge` executable: a real process with
//! piped stdin and stdout, talking to a fake app on a private test endpoint.
//!
//! They rely on the overrides `WIRECATCH_BRIDGE_TEST_ENDPOINT`,
//! `WIRECATCH_BRIDGE_TEST_LAUNCH` and `WIRECATCH_BRIDGE_TEST_START_BUDGET_MS`
//! (see `src/app.rs`), which exist only with the
//! `test-overrides` feature, so the file is empty without it and
//! `needs_overrides.rs` fails instead. Run with
//! `cargo test -p wirecatch-bridge --features test-overrides`.
#![cfg(feature = "test-overrides")]

use std::ffi::OsString;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{sleep, timeout};
use wirecatch_ipc::{Endpoint, Listener};
use wirecatch_product::{
    BRIDGE_EXECUTABLE, CHROMIUM_EXTENSION_ID, CHROMIUM_EXTENSION_ORIGIN, FIREFOX_EXTENSION_ID,
    LOG_FOLDER_VARIABLE, NATIVE_HOST_NAME,
};
use wirecatch_protocol::frame::{self, read_async, write_async};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Hello, HelloReply, Message, Outcome, ProtocolError, Reply,
};
use wirecatch_protocol::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION};
use wirecatch_test_support::{PATIENCE, PrivateDir, TestResult, private_dir};

const BRIDGE: &str = env!("CARGO_BIN_EXE_wirecatch-bridge");
const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";
const START_BUDGET_VAR: &str = "WIRECATCH_BRIDGE_TEST_START_BUDGET_MS";
const CHROME_ORIGIN: &str = CHROMIUM_EXTENSION_ORIGIN;
const FIREFOX_ID: &str = FIREFOX_EXTENSION_ID;

/// Read by `fake_app_process`, which the bridge starts in place of the app.
const FAKE_APP_ENDPOINT: &str = "WIRECATCH_TEST_FAKE_APP_ENDPOINT";
const FAKE_APP_MODE: &str = "WIRECATCH_TEST_FAKE_APP_MODE";
/// A file `fake_app_process` creates as it starts, so a test knows the bridge has started it.
const FAKE_APP_STARTED: &str = "WIRECATCH_TEST_FAKE_APP_STARTED";

/// How long the fake app stays after the bridge has left, for the test's last check, which may never come; a helper process must not outlive its test by long.
const LINGER: Duration = Duration::from_secs(10);

/// The most memory an idle bridge may hold, resident or private.
const IDLE_MEMORY_LIMIT: usize = 10 * 1024 * 1024;

/// A private endpoint for one test, in a private folder of its own that is
/// removed on drop. On every platform the program the bridge "starts" by
/// default is a missing file inside that folder.
struct TestEndpoint {
    endpoint: Endpoint,
    dir: PrivateDir,
}

impl TestEndpoint {
    fn new() -> TestResult<Self> {
        let dir = private_dir("bridge-it")?;
        Ok(Self {
            endpoint: Endpoint::from_raw(dir.endpoint_name()),
            dir,
        })
    }
}

/// The bridge, pointed at the test's endpoint. By default "starting the app"
/// runs a program that does not exist, so no test ever starts anything real.
fn bridge(args: &[&str], test: &TestEndpoint) -> Command {
    let missing = test
        .dir
        .path()
        .join(format!("no-such-app{}", std::env::consts::EXE_SUFFIX));
    let launch = serde_json::json!([missing]).to_string();
    let mut command = Command::new(BRIDGE);
    command
        .args(args)
        .env(ENDPOINT_VAR, test.endpoint.as_os_str())
        .env(LAUNCH_VAR, launch)
        // Its log file goes in the test's folder, not the person's.
        .env(LOG_FOLDER_VARIABLE, test.dir.path())
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

fn start(mut command: Command) -> TestResult<Running> {
    let mut child = command.spawn()?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("the bridge has no stdout pipe")?;
    Ok(Running {
        child,
        stdin,
        stdout,
    })
}

impl Running {
    async fn send(&mut self, bytes: &[u8]) -> TestResult {
        let stdin = self.stdin.as_mut().ok_or("stdin is already closed")?;
        stdin.write_all(bytes).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Closes stdin (what the browser does when the port closes) and waits for a
    /// clean exit.
    async fn close_and_wait(&mut self) -> TestResult {
        drop(self.stdin.take());
        let status = timeout(PATIENCE, self.child.wait())
            .await
            .map_err(|_| "the bridge did not exit after stdin closed")??;
        assert!(status.success(), "{status}");
        Ok(())
    }
}

fn hello(id: u64) -> TestResult<Vec<u8>> {
    Ok(frame::encode(&Message::Request {
        id,
        call: Call::Hello(Hello {
            client: ClientKind::Extension,
            client_version: "test".into(),
            protocol_version: MIN_PROTOCOL_VERSION,
            max_protocol_version: Some(PROTOCOL_VERSION),
            browser: None,
            start_app: true,
        }),
    })?)
}

/// A frame holding `body` byte for byte, for JSON no message type produces.
fn raw(body: &str) -> TestResult<Vec<u8>> {
    let mut bytes = frame::length_prefix(body.len())?.to_vec();
    bytes.extend_from_slice(body.as_bytes());
    Ok(bytes)
}

/// Reads one whole frame (length prefix included), or `None` at end of stream.
async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> TestResult<Option<Vec<u8>>> {
    timeout(PATIENCE, async {
        let Some(body) = frame::read_body_async(reader).await? else {
            return Ok(None);
        };
        let mut bytes = frame::length_prefix(body.len())?.to_vec();
        bytes.extend_from_slice(&body);
        Ok(Some(bytes))
    })
    .await
    .map_err(|_| "no frame arrived in time")?
}

fn decode(bytes: &[u8]) -> TestResult<Message> {
    Ok(frame::decode(&bytes[frame::HEADER_LEN..])?)
}

fn assert_unavailable(bytes: &[u8], expected_id: u64) -> TestResult<String> {
    Ok(match decode(bytes)? {
        Message::Response {
            id,
            outcome:
                Outcome::Err(ProtocolError {
                    code: ErrorCode::AppUnavailable,
                    message,
                }),
        } if id == expected_id => message,
        other => panic!("expected AppUnavailable for {expected_id}, got {other:?}"),
    })
}

#[tokio::test]
async fn messages_and_events_pass_through_unchanged() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;

    // Unusual spacing and a field this build does not know: the bridge must not
    // re-encode anything.
    let request = raw(
        r#"{"kind":"request", "id":1,"call":{"method":"hello","params":{"client":"extension","client_version":"1","protocol_version":1,"browser":{"name":"chrome","version":"130"}}},"from_a_newer_extension":true}"#,
    )?;
    bridge.send(&request).await?;
    let mut app = timeout(PATIENCE, listener.accept()).await??;
    assert_eq!(read_frame(&mut app).await?, Some(request));

    let response = raw(
        r#"{"kind":"response","id":1,"outcome":{"status":"ok","value":{"type":"hello","data":{"app_version":"0.1.0","protocol_version":1}}}, "from_a_newer_app":[1,2]}"#,
    )?;
    let event = raw(r#"{"kind":"event","event":{"type":"queues_changed","data":[]}}"#)?;
    app.write_all(&response).await?;
    app.write_all(&event).await?;
    assert_eq!(read_frame(&mut bridge.stdout).await?, Some(response));
    assert_eq!(read_frame(&mut bridge.stdout).await?, Some(event));

    // More traffic in both directions after the handshake.
    for id in 2..=4 {
        let request = frame::encode(&Message::Request {
            id,
            call: Call::ListQueues,
        })?;
        bridge.send(&request).await?;
        assert_eq!(read_frame(&mut app).await?, Some(request));
        let response = frame::encode(&Message::Response {
            id,
            outcome: Outcome::Ok(Reply::Queues(Vec::new())),
        })?;
        app.write_all(&response).await?;
        assert_eq!(read_frame(&mut bridge.stdout).await?, Some(response));
    }

    bridge.close_and_wait().await?;
    assert_eq!(read_frame(&mut bridge.stdout).await?, None);
    // The app sees the bridge leave.
    assert_eq!(read_frame(&mut app).await?, None);
    Ok(())
}

#[tokio::test]
async fn a_firefox_caller_is_served() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let manifest = std::env::temp_dir().join(format!("{NATIVE_HOST_NAME}.json"));
    let manifest = manifest.to_str().unwrap();
    let mut bridge = start(bridge(&[manifest, FIREFOX_ID], &test))?;
    bridge.send(&hello(5)?).await?;
    let mut app = timeout(PATIENCE, listener.accept()).await??;
    assert_eq!(read_frame(&mut app).await?, Some(hello(5)?));
    let answer = frame::encode(&Message::Response {
        id: 5,
        outcome: Outcome::Ok(Reply::Done),
    })?;
    app.write_all(&answer).await?;
    assert_eq!(read_frame(&mut bridge.stdout).await?, Some(answer));
    bridge.close_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn callers_that_are_not_allowed_are_refused() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let manifest = std::env::temp_dir().join(format!("{NATIVE_HOST_NAME}.json"));
    let manifest = manifest.to_str().unwrap();
    let no_slash = format!("chrome-extension://{CHROMIUM_EXTENSION_ID}");
    let firefox_as_origin = format!("moz-extension://{FIREFOX_EXTENSION_ID}/");
    let refused: &[&[&str]] = &[
        &[],
        &["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
        &[&no_slash],
        &[manifest, "evil@example.com"],
        &[manifest],
        &[&firefox_as_origin],
    ];
    for args in refused {
        let mut bridge = start(bridge(args, &test))?;
        if let Some(stdin) = bridge.stdin.as_mut() {
            match stdin.write_all(&hello(1)?).await {
                Ok(()) => {}
                // The bridge may already be gone.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(e) => return Err(e.into()),
            }
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
    Ok(())
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
    if let Some(started) = std::env::var_os(FAKE_APP_STARTED) {
        std::fs::write(started, b"").unwrap();
    }
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
        // Whether the check comes or the linger runs out, the fake app's job is over.
        tokio::select! {
            _ = listener.accept() => {}
            () = tokio::time::sleep(LINGER) => {}
        }
    });
}

fn launch_fake_app(mode: &str, endpoint: &Endpoint, command: &mut Command) -> TestResult {
    let test_exe = std::env::current_exe()?;
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
    Ok(())
}

/// Gives the bridge `budget` to reach an app it started, in place of its own 5 seconds.
fn with_start_budget(command: &mut Command, budget: Duration) {
    command.env(START_BUDGET_VAR, budget.as_millis().to_string());
}

#[tokio::test]
async fn the_app_is_started_when_it_is_not_running() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut command = bridge(&[CHROME_ORIGIN], &test);
    launch_fake_app("serve", &test.endpoint, &mut command)?;
    // However long a busy machine takes to start the app, the bridge waits for it.
    with_start_budget(&mut command, PATIENCE);
    let mut bridge = start(command)?;

    bridge.send(&hello(7)?).await?;
    let answer = read_frame(&mut bridge.stdout)
        .await?
        .ok_or("the stream ended")?;
    // A real answer, not AppUnavailable: the bridge started the app and reached it.
    assert_eq!(
        decode(&answer)?,
        Message::Response {
            id: 7,
            outcome: Outcome::Ok(Reply::Hello(HelloReply {
                app_version: "fake".into(),
                protocol_version: PROTOCOL_VERSION,
            })),
        }
    );

    // When the browser closes the port, the browser side of the pipe closes even
    // though the app the bridge started keeps running (it must not inherit the
    // browser's pipe handles).
    bridge.close_and_wait().await?;
    let eof = read_frame(&mut bridge.stdout).await?;
    assert_eq!(eof, None, "stdout must close when the bridge exits");
    // Still running: it accepts this connection, and then exits.
    assert!(
        wirecatch_ipc::connect(&test.endpoint).await.is_ok(),
        "the fake app should still be running"
    );
    Ok(())
}

#[tokio::test]
async fn app_unavailable_when_the_app_cannot_be_started() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;
    bridge.send(&hello(1)?).await?;
    bridge.send(&hello(2)?).await?;
    let first = read_frame(&mut bridge.stdout)
        .await?
        .ok_or("the stream ended")?;
    let second = read_frame(&mut bridge.stdout)
        .await?
        .ok_or("the stream ended")?;
    let message = assert_unavailable(&first, 1)?;
    assert_unavailable(&second, 2)?;
    // The launch failed, so the bridge gave up at once rather than waiting out its budget
    // (`app::tests::gives_up_when_the_app_cannot_be_started` proves that part).
    assert!(message.contains("Reinstall Wirecatch"), "{message}");
    // No relaunch loop: the bridge stays up answering until the browser leaves.
    bridge.send(&hello(3)?).await?;
    assert_unavailable(
        &read_frame(&mut bridge.stdout)
            .await?
            .ok_or("the stream ended")?,
        3,
    )?;
    bridge.close_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn app_unavailable_when_the_app_never_starts_listening() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut command = bridge(&[CHROME_ORIGIN], &test);
    launch_fake_app("never_listen", &test.endpoint, &mut command)?;
    let mut bridge = start(command)?;
    let started = Instant::now();
    bridge.send(&hello(3)?).await?;
    let answer = read_frame(&mut bridge.stdout)
        .await?
        .ok_or("the stream ended")?;
    let waited = started.elapsed();
    let message = assert_unavailable(&answer, 3)?;
    assert!(message.contains("did not respond"), "{message}");
    // The bridge's own budget, which only a lower bound can check without racing the machine.
    assert!(
        waited >= Duration::from_millis(4900),
        "answered after {waited:?}, before the 5 second budget ran out"
    );
    bridge.close_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn closing_the_connection_while_the_app_starts_exits_promptly() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut command = bridge(&[CHROME_ORIGIN], &test);
    launch_fake_app("never_listen", &test.endpoint, &mut command)?;
    // A budget the test does not wait out: a bridge that sat it out would not exit in time.
    with_start_budget(&mut command, 2 * PATIENCE);
    let started_app = test.dir.path().join("app-started");
    command.env(FAKE_APP_STARTED, &started_app);
    let mut bridge = start(command)?;
    bridge.send(&hello(1)?).await?;
    // Once the app is started, the bridge is waiting for it.
    let deadline = Instant::now() + PATIENCE;
    while !started_app.exists() {
        assert!(
            Instant::now() < deadline,
            "the bridge never started the app"
        );
        sleep(Duration::from_millis(10)).await;
    }
    bridge.close_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn the_bridge_exits_when_the_app_goes_away_so_the_extension_can_reconnect() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;
    bridge.send(&hello(1)?).await?;
    let mut app = timeout(PATIENCE, listener.accept()).await??;
    read_frame(&mut app).await?.ok_or("the stream ended")?;
    // The user quits Wirecatch with the request unanswered.
    drop(app);
    drop(listener);
    assert_unavailable(
        &read_frame(&mut bridge.stdout)
            .await?
            .ok_or("the stream ended")?,
        1,
    )?;
    // Then the bridge exits by itself, with stdin still open, so the browser
    // sees the port disconnect.
    let status = timeout(PATIENCE, bridge.child.wait())
        .await
        .expect("the bridge must exit after the app goes away")
        .unwrap();
    assert!(status.success(), "{status}");
    assert_eq!(read_frame(&mut bridge.stdout).await?, None);
    Ok(())
}

#[tokio::test]
async fn an_oversized_message_is_skipped_and_the_session_goes_on() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;

    let len = frame::MAX_FRAME_LEN + 1;
    // Over the limit, so `length_prefix` would refuse to write its header.
    let mut oversized = u32::try_from(len)?.to_le_bytes().to_vec();
    oversized.resize(4 + len, b' ');
    bridge.send(&oversized).await?;
    bridge.send(&hello(2)?).await?;

    // Only the message within the limit reaches the app, and it is answered.
    let mut app = timeout(PATIENCE, listener.accept()).await??;
    assert_eq!(read_frame(&mut app).await?, Some(hello(2)?));
    let answer = frame::encode(&Message::Response {
        id: 2,
        outcome: Outcome::Ok(Reply::Done),
    })?;
    app.write_all(&answer).await?;
    assert_eq!(read_frame(&mut bridge.stdout).await?, Some(answer));
    bridge.close_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn exits_when_the_browser_closes_the_connection_before_any_message() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;
    bridge.close_and_wait().await?;
    assert!(
        timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "the bridge must not contact the app without a message"
    );
    Ok(())
}

/// Working set and private (committed, not shared) bytes of a process.
#[cfg(windows)]
fn memory_of(pid: u32) -> TestResult<(usize, usize)> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>())?;
    // SAFETY: plain Win32 calls on a handle we open and close here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        assert!(!process.is_null());
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let ok = K32GetProcessMemoryInfo(process, &raw mut counters, size);
        CloseHandle(process);
        assert_ne!(ok, 0);
        Ok((counters.WorkingSetSize, counters.PagefileUsage))
    }
}

/// Resident and anonymous (private) resident bytes of a process.
#[cfg(target_os = "linux")]
fn memory_of(pid: u32) -> TestResult<(usize, usize)> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    let field = |name: &str| -> TestResult<usize> {
        let kib = status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .ok_or_else(|| format!("no {name} line in /proc/{pid}/status"))?;
        Ok(kib.parse::<usize>()? * 1024)
    };
    Ok((field("VmRSS:")?, field("RssAnon:")?))
}

#[tokio::test]
async fn idle_memory_stays_under_10_mb() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut listener = Listener::bind(&test.endpoint)?;
    let mut bridge = start(bridge(&[CHROME_ORIGIN], &test))?;
    bridge.send(&hello(1)?).await?;
    let mut app = timeout(PATIENCE, listener.accept()).await??;
    read_frame(&mut app).await?.ok_or("the stream ended")?;
    let answer = frame::encode(&Message::Response {
        id: 1,
        outcome: Outcome::Ok(Reply::Done),
    })?;
    app.write_all(&answer).await?;
    read_frame(&mut bridge.stdout)
        .await?
        .ok_or("the stream ended")?;

    // Connected and idle.
    sleep(Duration::from_secs(1)).await;
    let pid = bridge.child.id().unwrap();
    let (resident, private) = memory_of(pid)?;
    eprintln!(
        "idle bridge ({} build): resident {} KiB, private {} KiB",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        resident / 1024,
        private / 1024
    );
    assert!(resident < IDLE_MEMORY_LIMIT, "resident {resident} bytes");
    assert!(private < IDLE_MEMORY_LIMIT, "private {private} bytes");
    bridge.close_and_wait().await?;
    Ok(())
}

/// When the launcher itself cannot be set up, the request still gets an answer.
#[tokio::test]
async fn a_broken_launch_override_still_answers() -> TestResult {
    let test = TestEndpoint::new()?;
    let mut command = bridge(&[CHROME_ORIGIN], &test);
    command.env(LAUNCH_VAR, OsString::from("[]"));
    let mut bridge = start(command)?;
    bridge.send(&hello(9)?).await?;
    assert_unavailable(
        &read_frame(&mut bridge.stdout)
            .await?
            .ok_or("the stream ended")?,
        9,
    )?;
    bridge.close_and_wait().await?;
    Ok(())
}

#[test]
fn help_fails_when_the_usage_cannot_be_printed() -> TestResult {
    let logs = private_dir("bridge-help-logs")?;
    let (reader, writer) = std::io::pipe()?;
    drop(reader);
    let output = std::process::Command::new(BRIDGE)
        .arg("--help")
        .env(LOG_FOLDER_VARIABLE, logs.path())
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()?;
    assert!(
        !output.status.success(),
        "a usage nobody could read must not report success"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not print the usage"));
    Ok(())
}

/// What the bridge logs reaches its log file as well as stderr, since a browser keeps its stderr
/// nowhere a person can find it.
#[test]
fn the_bridge_logs_to_its_file() -> TestResult {
    let logs = private_dir("bridge-logs")?;
    let output = std::process::Command::new(BRIDGE)
        .env(LOG_FOLDER_VARIABLE, logs.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    assert!(
        !output.status.success(),
        "a bridge nobody started must refuse"
    );
    let file = logs.path().join(format!("{BRIDGE_EXECUTABLE}.log"));
    let logged = std::fs::read_to_string(&file)?;
    assert!(
        logged.contains("ERROR") && logged.contains("refused to start"),
        "{logged}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("refused to start"));
    Ok(())
}
