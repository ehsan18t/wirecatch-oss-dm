//! Running the add-on's programs: time limits, cancelling, long lines, and programs that leave others running.

use std::ffi::OsString;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::CancellationToken;
use wirecatch_streams::process::{self, Output, RunError, RunOptions};
use wirecatch_test_support::TestResult;

/// How long a run that should end promptly may take before the test fails, generous for a loaded runner.
const PATIENCE: Duration = Duration::from_secs(20);
/// A time limit short enough for a test.
const SHORT: Duration = Duration::from_millis(300);
/// A time limit long enough for the fake program to start the child that holds a connection, even on a loaded runner.
const HOLDER_LIMIT: Duration = Duration::from_secs(5);

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
    let ran = tokio::time::timeout(
        PATIENCE,
        process::run(&program, &args(words), &dir, options, cancel, on_line),
    )
    .await
    .map_err(|_| format!("the run of {words:?} did not end within {PATIENCE:?}"))?;
    Ok(ran)
}

#[tokio::test]
async fn a_program_that_stops_writing_is_stopped_as_stalled() -> TestResult {
    let options = RunOptions::lines().with_idle_timeout(SHORT);
    let ran = run(&["hang"], options, &CancellationToken::new(), |_, _| {}).await?;
    assert!(
        matches!(ran, Err(RunError::Stalled(d)) if d == SHORT),
        "{ran:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_program_that_keeps_writing_is_not_stalled() -> TestResult {
    // Eight lines 100 ms apart: longer than the idle limit, so each line must reset it.
    let options = RunOptions::lines().with_idle_timeout(SHORT);
    let mut lines = 0;
    let ran = run(
        &["ticks", "8"],
        options,
        &CancellationToken::new(),
        |from, _| {
            lines += usize::from(from == Output::Stdout);
        },
    )
    .await??;
    assert!(ran.success);
    assert_eq!(lines, 8);
    Ok(())
}

#[tokio::test]
async fn a_program_past_its_time_limit_is_stopped() -> TestResult {
    let options = RunOptions::collect().with_timeout(SHORT);
    let ran = run(&["hang"], options, &CancellationToken::new(), |_, _| {}).await?;
    assert!(
        matches!(ran, Err(RunError::TimedOut(d)) if d == SHORT),
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
    assert!(ran.success);
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
    assert!(!ran.success);
    assert_eq!(ran.error_line(), Some("ERROR: it went wrong"));
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
#[tokio::test]
async fn a_silent_program_whose_output_grows_is_not_stalled() -> TestResult {
    let dir = tempfile::tempdir()?;
    let output = dir.path().join("out.bin");
    let path = output.to_string_lossy().into_owned();
    let words = ["write-quietly", path.as_str(), "8"];
    let watched = RunOptions::lines()
        .with_idle_timeout(SHORT)
        .watching(output.clone());
    let ran = run(&words, watched, &CancellationToken::new(), |_, _| {}).await??;
    assert!(ran.success);
    // The same run without the output to watch is stopped as stuck.
    let unwatched = RunOptions::lines().with_idle_timeout(SHORT);
    let ran = run(&words, unwatched, &CancellationToken::new(), |_, _| {}).await?;
    assert!(matches!(ran, Err(RunError::Stalled(_))), "{ran:?}");
    Ok(())
}

/// ffmpeg's `+faststart` pass moves the data within the finished file, so the file changes for a long time without growing and without a line; the run is not stuck meanwhile.
#[tokio::test]
async fn a_silent_program_rewriting_its_output_in_place_is_not_stalled() -> TestResult {
    let dir = tempfile::tempdir()?;
    let output = dir.path().join("out.bin");
    let path = output.to_string_lossy().into_owned();
    let words = ["rewrite-quietly", path.as_str(), "8"];
    let watched = RunOptions::lines()
        .with_idle_timeout(SHORT)
        .watching(output.clone());
    let ran = run(&words, watched, &CancellationToken::new(), |_, _| {}).await??;
    assert!(ran.success);
    // The same run without the output to watch is stopped as stuck.
    let unwatched = RunOptions::lines().with_idle_timeout(SHORT);
    let ran = run(&words, unwatched, &CancellationToken::new(), |_, _| {}).await?;
    assert!(matches!(ran, Err(RunError::Stalled(_))), "{ran:?}");
    Ok(())
}

/// yt-dlp's runs watch their work folder, where ffmpeg writes the joined file for minutes without a line. On Windows a folder's listing shows a file's size and write time as they were when it was last closed, so the files in it are looked at one by one.
#[tokio::test]
async fn a_silent_program_writing_into_a_watched_folder_is_not_stalled() -> TestResult {
    for command in ["write-quietly", "rewrite-quietly"] {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("video.temp.mp4");
        let path = output.to_string_lossy().into_owned();
        let words = [command, path.as_str(), "8"];
        let watched = RunOptions::lines()
            .with_idle_timeout(SHORT)
            .watching(dir.path().to_path_buf());
        let ran = run(&words, watched, &CancellationToken::new(), |_, _| {}).await?;
        assert!(
            matches!(ran, Ok(ref ran) if ran.success),
            "{command}: {ran:?}"
        );
    }
    Ok(())
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
    match tokio::time::timeout(PATIENCE, connection.read(&mut byte)).await {
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

#[tokio::test]
async fn a_run_past_its_time_limit_ends_what_the_program_started() -> TestResult {
    let (address, accepted) = holder_listener().await?;
    let mut holding = false;
    let ran = run(
        &["start-holder", &address],
        RunOptions::lines().with_timeout(HOLDER_LIMIT),
        &CancellationToken::new(),
        |_, line| holding |= line == "holding",
    )
    .await?;
    assert!(matches!(ran, Err(RunError::TimedOut(_))), "{ran:?}");
    assert!(holding, "the holder did not start within {HOLDER_LIMIT:?}");
    holder_ended(accepted.await??).await
}

/// What the run logs, kept for a test to read.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Log {
    fn text(&self) -> String {
        let bytes = self.0.lock().map(|b| b.clone()).unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::io::Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("the log lock is poisoned"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;

    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

/// A program that exits and leaves a child running: the run ends the child as soon as the program has ended. The child shares standard error, so if it were left running the run would read on until its grace ran out and say so in the log; it would still end the child afterwards, which is why the log is what tells.
#[tokio::test]
async fn what_a_program_leaves_running_ends_with_it() -> TestResult {
    let log = Log::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let (address, accepted) = holder_listener().await?;
    let mut holding = false;
    let ran = run(
        &["leave-holder", &address],
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, line| holding |= line == "holding",
    )
    .await??;
    assert!(ran.success, "{:?}", ran.stderr);
    assert!(holding, "the holder did not start");
    let logged = log.text();
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
    assert!(ran.success);
    Ok(())
}
