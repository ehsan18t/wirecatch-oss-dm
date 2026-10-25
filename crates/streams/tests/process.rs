//! Running the add-on's programs: time limits, cancelling, long lines, and programs that leave others running.

use std::ffi::OsString;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::CancellationToken;
use wirecatch_streams::process::{
    self, MAX_REASON_CHARS, Output, RunError, RunOptions, STDERR_TAIL_BYTES,
};
use wirecatch_test_support::logs::Logs;
use wirecatch_test_support::{PATIENCE, TestResult};

/// A time limit short enough for a test. The tests that expect a run to go on past it move a paused clock themselves ([`Stepped`]); a run expected to stop at it may take longer on a loaded machine and still pass.
const SHORT: Duration = Duration::from_millis(300);

/// The stand-in program `cargo test` builds from `examples/fake_program.rs`. Missing only when the tests are built alone, which skips the examples; that fails loudly.
fn fake_program() -> TestResult<PathBuf> {
    let exe = std::env::current_exe()?;
    let debug = exe
        .parent()
        .and_then(Path::parent)
        .ok_or("the test binary is not in a build folder")?;
    let path = debug
        .join("examples")
        .join(format!("fake_program{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        return Err(format!(
            "{} is missing: run `cargo test -p wirecatch-streams`, which builds the examples",
            path.display()
        )
        .into());
    }
    Ok(path)
}

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// Runs the fake program with `words`, failing the test if it does not end within [`PATIENCE`].
async fn run(
    words: &[&str],
    options: RunOptions,
    cancel: &CancellationToken,
    on_line: impl FnMut(Output, &str),
) -> TestResult<Result<process::Ran, RunError>> {
    let program = fake_program()?;
    let dir = std::env::temp_dir();
    let ran = within_patience(process::run(
        &program,
        &args(words),
        &dir,
        options,
        cancel,
        on_line,
    ))
    .await
    .map_err(|_| format!("the run of {words:?} did not end within {PATIENCE:?}"))?;
    Ok(ran)
}

#[tokio::test]
async fn a_program_that_stops_writing_is_stopped_as_stalled() -> TestResult {
    let options = RunOptions::lines().with_idle_timeout(SHORT);
    let ran = run(&["hang"], options, &CancellationToken::new(), |_, _| {}).await?;
    assert!(
        matches!(ran, Err(RunError::Stalled { quiet, .. }) if quiet == SHORT),
        "{ran:?}"
    );
    Ok(())
}

/// Lines [`STEP`] apart, past the idle limit in all, so each line must reset it.
#[tokio::test(start_paused = true)]
async fn a_program_that_keeps_writing_is_not_stalled() -> TestResult {
    let options = RunOptions::lines().with_idle_timeout(SHORT);
    let mut run = Stepped::start(&["ticks"], options).await?;
    for n in 0..STEPS {
        assert!(run.step().await?, "the run ended at step {n}");
        assert_eq!(run.line().await?, format!("tick {n}"));
    }
    let ran = run.end().await??;
    assert!(ran.success());
    Ok(())
}

#[tokio::test]
async fn a_program_past_its_time_limit_is_stopped() -> TestResult {
    let options = RunOptions::collect().with_timeout(SHORT);
    let ran = run(&["hang"], options, &CancellationToken::new(), |_, _| {}).await?;
    assert!(
        matches!(ran, Err(RunError::TimedOut { limit, .. }) if limit == SHORT),
        "{ran:?}"
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_stops_a_program_while_it_runs() -> TestResult {
    let cancel = CancellationToken::new();
    let ran = run(&["hang"], RunOptions::lines(), &cancel, |_, line| {
        if line == "ready" {
            cancel.cancel();
        }
    })
    .await?;
    assert!(matches!(ran, Err(RunError::Cancelled)), "{ran:?}");
    Ok(())
}

#[tokio::test]
async fn long_lines_are_cut_at_a_character_boundary() -> TestResult {
    let limit = 16 * wirecatch_engine::units::KIB_USIZE;
    let mut seen = Vec::new();
    let length = (limit - 1).to_string();
    let ran = run(
        &["long-line", &length],
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, line| {
            seen.push(line.to_string());
        },
    )
    .await??;
    assert!(ran.success());
    // The three bytes of `€` would pass the limit, so the line ends before it.
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].len(), limit - 1);
    assert!(seen[0].bytes().all(|b| b == b'a'));
    Ok(())
}

#[tokio::test]
async fn a_failure_keeps_its_error_line() -> TestResult {
    let ran = run(
        &["fail"],
        RunOptions::collect(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await??;
    assert!(!ran.success());
    assert_eq!(ran.error_line().as_deref(), Some("ERROR: it went wrong"));
    assert_eq!(ran.status.and_then(|s| s.code()), Some(1));
    Ok(())
}

/// A site controls what yt-dlp and ffmpeg print, and they print the links they read: what a failed run keeps of standard error is bounded and holds no secret, and its reason for a person is short.
#[tokio::test]
async fn a_failure_keeps_a_short_reason_without_secrets() -> TestResult {
    // Long, but within what the tail keeps of a line, so only the cut for a person can make the reason short.
    let said = format!(
        "ERROR: 403 for https://ada:pa55@cdn.example.com/v.mp4?token=T0KEN {}, sent Cookie: SID=C00KIE",
        "and more ".repeat(250)
    );
    let ran = run(
        &["fail-after", "1000", "100", "3", &said],
        RunOptions::collect(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await??;
    assert!(!ran.success());
    let kept = ran.stderr.len_bytes();
    assert!(
        kept <= STDERR_TAIL_BYTES,
        "{kept} bytes of standard error kept"
    );
    assert_eq!(ran.status.and_then(|s| s.code()), Some(3));
    let reason = ran.error_line().ok_or("no error line")?;
    assert!(
        reason.chars().count() <= MAX_REASON_CHARS,
        "{} characters",
        reason.chars().count()
    );
    let tail = ran.stderr.to_string();
    for secret in ["pa55", "T0KEN", "C00KIE"] {
        assert!(!reason.contains(secret), "{reason}");
        assert!(!tail.contains(secret), "{tail}");
    }
    Ok(())
}

/// A failed run is logged with the end of what the program wrote, its secrets hidden, so the log carries the reason even when a person's message is short.
#[tokio::test]
async fn a_failure_is_logged_with_what_the_program_wrote() -> TestResult {
    let (logs, _capturing) = Logs::capture();
    let ran = run(
        &[
            "fail-after",
            "2",
            "10",
            "4",
            "ERROR: no video at https://h/v?sig=S1G",
        ],
        RunOptions::collect(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await??;
    assert!(!ran.success());
    let failed = logs
        .events()
        .into_iter()
        .find(|e| e.level == tracing::Level::WARN && e.message == "a program failed")
        .ok_or("the failure was not logged")?;
    let stderr = failed.field("stderr").ok_or("no stderr field")?;
    assert!(
        stderr.contains("ERROR: no video at https://h/v?sig=***"),
        "{stderr}"
    );
    assert!(!logs.text().contains("S1G"), "{}", logs.text());
    Ok(())
}

/// A program stopped for writing nothing still says what it last wrote, in the error and in the log.
#[tokio::test]
async fn a_stalled_program_keeps_what_it_wrote() -> TestResult {
    let (logs, _capturing) = Logs::capture();
    let options = RunOptions::lines().with_idle_timeout(SHORT);
    let ran = run(
        &["hang", "Opening input https://h/live.m3u8?token=T0K"],
        options,
        &CancellationToken::new(),
        |_, _| {},
    )
    .await?;
    let Err(RunError::Stalled { stderr, .. }) = ran else {
        return Err(format!("{ran:?}").into());
    };
    assert_eq!(
        stderr.error_line(),
        Some("Opening input https://h/live.m3u8?token=***")
    );
    assert!(
        logs.text().contains("live.m3u8?token=***"),
        "{}",
        logs.text()
    );
    Ok(())
}

#[tokio::test]
async fn a_missing_program_is_a_start_error() -> TestResult {
    let dir = std::env::temp_dir();
    let missing = dir.join("no-such-program-for-wirecatch");
    let ran = process::run(
        &missing,
        &[],
        &dir,
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await;
    assert!(
        matches!(ran, Err(RunError::Start { ref program, .. }) if program == &missing),
        "{ran:?}"
    );
    Ok(())
}

/// A program that has reported progress and then only writes its output (ffmpeg's `+faststart` pass, yt-dlp's merge) is not stuck while the output grows.
#[tokio::test(start_paused = true)]
async fn a_silent_program_whose_output_grows_is_not_stalled() -> TestResult {
    a_silent_program_is_watched_through("write-quietly", |output| output).await
}

/// ffmpeg's `+faststart` pass moves the data within the finished file, so the file changes for a long time without growing and without a line; the run is not stuck meanwhile.
#[tokio::test(start_paused = true)]
async fn a_silent_program_rewriting_its_output_in_place_is_not_stalled() -> TestResult {
    a_silent_program_is_watched_through("rewrite-quietly", |output| output).await
}

/// yt-dlp's runs watch their work folder, where ffmpeg writes the joined file for minutes without a line. On Windows a folder's listing shows a file's size and write time as they were when it was last closed, so the files in it are looked at one by one.
#[tokio::test(start_paused = true)]
async fn a_silent_program_writing_into_a_watched_folder_is_not_stalled() -> TestResult {
    for command in ["write-quietly", "rewrite-quietly"] {
        a_silent_program_is_watched_through(command, |output| {
            output.parent().map(Path::to_path_buf).unwrap_or_default()
        })
        .await
        .map_err(|e| format!("{command}: {e}"))?;
    }
    Ok(())
}

/// How many steps a stepped run takes: [`STEP`] apart, four times the idle limit in all, so the limit must be reset to get through them.
const STEPS: u32 = 8;

/// Runs `command`, which writes `video.temp.mp4` and nothing to its outputs, for [`STEPS`] steps while watching what `watched` makes of the file's path: the run is not stuck. The same run without the output to watch is stopped as stuck.
async fn a_silent_program_is_watched_through(
    command: &str,
    watched: impl Fn(PathBuf) -> PathBuf,
) -> TestResult {
    let dir = tempfile::tempdir()?;
    let output = dir.path().join("video.temp.mp4");
    let path = output.to_string_lossy().into_owned();
    let words = [command, path.as_str()];
    let options = RunOptions::lines()
        .with_idle_timeout(SHORT)
        .watching(watched(output));
    let mut run = Stepped::start(&words, options).await?;
    for n in 0..STEPS {
        assert!(run.step().await?, "the run ended at step {n}");
    }
    let ran = run.end().await?;
    assert!(matches!(ran, Ok(ref ran) if ran.success()), "{ran:?}");

    let unwatched = RunOptions::lines().with_idle_timeout(SHORT);
    let mut run = Stepped::start(&words, unwatched).await?;
    let mut took = 0;
    while run.step().await? {
        took += 1;
        assert!(
            took < STEPS,
            "the run without a watched output was not stopped"
        );
    }
    let ran = run.end().await?;
    assert!(
        matches!(ran, Err(RunError::Stalled { quiet, .. }) if quiet == SHORT),
        "{ran:?}"
    );
    Ok(())
}

/// Holds a paused clock still until thawed or dropped. Tokio moves a paused clock on by itself whenever nothing is ready to run, which a run waiting on its program always is, unless a blocking task is running: this is one. Reading a program's outputs runs blocking tasks too, which a program that hangs never lets end, so the waits of a test on a paused clock are bounded by [`within_patience`], in real time, not by the paused clock.
struct Frozen(std::sync::mpsc::Sender<()>);

impl Frozen {
    fn clock() -> Self {
        let (release, released) = std::sync::mpsc::channel::<()>();
        tokio::task::spawn_blocking(move || thawed(released.recv()));
        Self(release)
    }

    /// Lets the clock move on by itself again.
    fn thaw(self) {
        drop(self.0);
    }
}

/// Takes the outcome of holding the clock: the test thawed it or ended; either way the clock may move on by itself again.
fn thawed(held: Result<(), std::sync::mpsc::RecvError>) {
    let (Ok(()) | Err(_)) = held;
}

/// [`PATIENCE`] of real time ran out.
#[derive(Debug)]
struct PatienceRanOut;

/// Waits for `future` for at most [`PATIENCE`] of real time, which neither a paused clock nor a blocking task that never ends holds back.
async fn within_patience<T>(future: impl Future<Output = T>) -> Result<T, PatienceRanOut> {
    let (expired, expiry) = tokio::sync::oneshot::channel::<()>();
    let (_done, done_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(PATIENCE) {
            waiter_may_have_ended(expired.send(()));
        }
    });
    tokio::select! {
        value = future => Ok(value),
        _ = expiry => Err(PatienceRanOut),
    }
}

/// Takes the outcome of telling a wait that [`PATIENCE`] ran out: a wait that already ended no longer listens, which changes nothing.
fn waiter_may_have_ended(told: Result<(), ()>) {
    let (Ok(()) | Err(())) = told;
}

/// Takes the outcome of passing a line to the test: a test that has ended no longer reads them, which changes nothing.
fn test_may_have_ended(passed: &Result<(), tokio::sync::mpsc::error::SendError<String>>) {
    let (Ok(()) | Err(_)) = passed;
}

/// How much of the run's clock passes before each step of a stepped run: half the idle limit. The limit cannot run out between two steps, and it runs out only after two, so whenever the run looks at its watched output the program has taken a step since it last looked.
const STEP: Duration = SHORT.checked_div(2).expect("the idle limit halves");

/// A run of a fake program command that takes a step only when the test says (see `examples/fake_program.rs`), on a paused clock only the test moves: how slow the machine is changes nothing the run sees.
struct Stepped {
    connection: TcpStream,
    lines: tokio::sync::mpsc::UnboundedReceiver<String>,
    ran: tokio::task::JoinHandle<Result<process::Ran, RunError>>,
    frozen: Frozen,
}

impl Stepped {
    /// Starts the fake program with `words` and the address it steps on; the test must be on a paused clock.
    async fn start(words: &[&str], options: RunOptions) -> TestResult<Self> {
        let frozen = Frozen::clock();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let mut words = args(words);
        words.push(listener.local_addr()?.to_string().into());
        let program = fake_program()?;
        let (heard, lines) = tokio::sync::mpsc::unbounded_channel();
        let ran = tokio::spawn(async move {
            let on_line = |from, line: &str| {
                if from == Output::Stdout {
                    test_may_have_ended(&heard.send(line.to_owned()));
                }
            };
            let dir = std::env::temp_dir();
            let cancel = CancellationToken::new();
            process::run(&program, &words, &dir, options, &cancel, on_line).await
        });
        let mut ran = ran;
        let connected = within_patience(async {
            tokio::select! {
                accepted = listener.accept() => Ok(accepted),
                ended = &mut ran => Err(format!("the run ended before the program connected: {ended:?}")),
            }
        });
        let (connection, _) = connected
            .await
            .map_err(|_| format!("the program did not connect within {PATIENCE:?}"))???;
        Ok(Self {
            connection,
            lines,
            ran,
            frozen,
        })
    }

    /// Lets [`STEP`] pass on the run's clock, then has the program take a step and waits until it has; false once the program has been stopped.
    async fn step(&mut self) -> TestResult<bool> {
        tokio::time::advance(STEP).await;
        if self.connection.write_all(&[1]).await.is_err() {
            return Ok(false);
        }
        let mut done = [0];
        let answered = within_patience(self.connection.read(&mut done))
            .await
            .map_err(|_| format!("the program did not take its step within {PATIENCE:?}"))?;
        Ok(matches!(answered, Ok(1)))
    }

    /// The next line the program wrote to its standard output, once the run has read it.
    async fn line(&mut self) -> TestResult<String> {
        let line = within_patience(self.lines.recv())
            .await
            .map_err(|_| format!("no line within {PATIENCE:?}"))?;
        Ok(line.ok_or("the program's output ended")?)
    }

    /// Has the program end, by closing its connection, and returns how the run ended.
    async fn end(self) -> TestResult<Result<process::Ran, RunError>> {
        let Self {
            connection,
            ran,
            frozen,
            ..
        } = self;
        drop(connection);
        let ran = within_patience(ran)
            .await
            .map_err(|_| format!("the run did not end within {PATIENCE:?}"))??;
        frozen.thaw();
        Ok(ran)
    }
}

/// Listens for the connection the fake program's `hold` keeps open while it runs, returning the address to pass to `start-holder` and the connection once made.
async fn holder_listener()
-> TestResult<(String, tokio::task::JoinHandle<std::io::Result<TcpStream>>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?.to_string();
    let accepted = tokio::spawn(async move { listener.accept().await.map(|(stream, _)| stream) });
    Ok((address, accepted))
}

/// Waits until the process holding `connection` has ended, which closes its end; fails if it is still running after [`PATIENCE`].
async fn holder_ended(mut connection: TcpStream) -> TestResult {
    let mut byte = [0];
    match within_patience(connection.read(&mut byte)).await {
        Ok(Ok(0) | Err(_)) => Ok(()),
        Ok(Ok(_)) => Err("the holder wrote to its connection".into()),
        Err(_) => Err(format!(
            "the program's child was still running {PATIENCE:?} after the run ended"
        )
        .into()),
    }
}

/// The child outlives the program unless the run kills them together: as a process group on Unix, as a job object on Windows.
#[tokio::test]
async fn cancelling_a_run_ends_what_the_program_started() -> TestResult {
    let (address, accepted) = holder_listener().await?;
    let cancel = CancellationToken::new();
    let ran = run(
        &["start-holder", &address],
        RunOptions::lines(),
        &cancel,
        |_, line| {
            if line == "holding" {
                cancel.cancel();
            }
        },
    )
    .await?;
    assert!(matches!(ran, Err(RunError::Cancelled)), "{ran:?}");
    holder_ended(accepted.await??).await
}

/// The time limit runs out only once the holder has started, on a paused clock the test moves, however slow the machine is to start programs.
#[tokio::test(start_paused = true)]
async fn a_run_past_its_time_limit_ends_what_the_program_started() -> TestResult {
    let frozen = Frozen::clock();
    let (address, accepted) = holder_listener().await?;
    let program = fake_program()?;
    let (heard, mut lines) = tokio::sync::mpsc::unbounded_channel();
    let ran = tokio::spawn(async move {
        let dir = std::env::temp_dir();
        let words = args(&["start-holder", &address]);
        let options = RunOptions::lines().with_timeout(SHORT);
        let on_line = |_, line: &str| test_may_have_ended(&heard.send(line.to_owned()));
        process::run(
            &program,
            &words,
            &dir,
            options,
            &CancellationToken::new(),
            on_line,
        )
        .await
    });
    let connection = within_patience(accepted)
        .await
        .map_err(|_| format!("the holder did not connect within {PATIENCE:?}"))???;
    let said = within_patience(lines.recv())
        .await
        .map_err(|_| format!("the holder said nothing within {PATIENCE:?}"))?;
    assert_eq!(said.as_deref(), Some("holding"));
    tokio::time::advance(SHORT).await;
    let ran = within_patience(ran)
        .await
        .map_err(|_| format!("the run did not end within {PATIENCE:?}"))??;
    assert!(
        matches!(ran, Err(RunError::TimedOut { limit, .. }) if limit == SHORT),
        "{ran:?}"
    );
    // Still on the held clock: the holder's end is an event, not a time.
    holder_ended(connection).await?;
    frozen.thaw();
    Ok(())
}

/// A program that exits and leaves a child running: the run ends the child as soon as the program has ended. The child shares standard error, so if it were left running the run would read on until its grace ran out and say so in the log; it would still end the child afterwards, which is why the log is what tells.
#[tokio::test]
async fn what_a_program_leaves_running_ends_with_it() -> TestResult {
    let (logs, _capturing) = Logs::capture();
    let (address, accepted) = holder_listener().await?;
    let mut holding = false;
    let ran = run(
        &["leave-holder", &address],
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, line| holding |= line == "holding",
    )
    .await??;
    assert!(ran.success(), "{:?}", ran.stderr);
    assert!(holding, "the holder did not start");
    let logged = logs.text();
    assert!(
        !logged.contains("outputs stayed open"),
        "the child was not ended with the program: {logged}"
    );
    holder_ended(accepted.await??).await
}

/// A child that left the program's group (as after `setsid`) keeps the outputs open; the run still ends once the program itself has, after a short grace.
#[cfg(unix)]
#[tokio::test]
async fn a_child_that_left_the_group_does_not_hold_the_run() -> TestResult {
    let ran = run(
        &["leave-detached-child"],
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, _| {},
    )
    .await??;
    assert!(ran.success());
    Ok(())
}

#[tokio::test]
async fn the_program_gets_the_environment_it_is_given_and_its_debug_hides_the_values() -> TestResult
{
    let options = RunOptions::collect().with_environment(vec![
        (
            "WIRECATCH_TEST_GIVEN".into(),
            Some("socks5h://u:secret@proxy:1080".into()),
        ),
        ("PATH".into(), None),
    ]);
    let shown = format!("{options:?}");
    assert!(
        !shown.contains("secret") && shown.contains("WIRECATCH_TEST_GIVEN"),
        "{shown}"
    );
    let ran = run(
        &["env", "WIRECATCH_TEST_GIVEN", "PATH"],
        options,
        &CancellationToken::new(),
        |_, _| {},
    )
    .await??;
    let stdout = String::from_utf8(ran.stdout)?;
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "WIRECATCH_TEST_GIVEN=socks5h://u:secret@proxy:1080",
            "PATH unset"
        ]
    );
    Ok(())
}
