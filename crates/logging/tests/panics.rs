//! A panic, in a real process with its logging set up as a binary sets it up. The test runs itself as that process (it has no test harness), so the code under test is always the code just built.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, ExitCode};

use wirecatch_logging::{LOG_FILTER_VARIABLE, LOG_FOLDER_VARIABLE};

/// Set in the process the test starts, which logs and then panics.
const CHILD_VARIABLE: &str = "WIRECATCH_PANICS_TEST_CHILD";

fn main() -> ExitCode {
    if std::env::var_os(CHILD_VARIABLE).is_some() {
        panic_with_a_secret();
    }
    // cargo-nextest first asks every test binary for its tests, in the standard harness's form, and then runs each by name.
    if std::env::args().any(|arg| arg == "--list") {
        if std::env::args().any(|arg| arg == "--ignored") {
            return ExitCode::SUCCESS;
        }
        return match writeln!(std::io::stdout(), "panics: test") {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    // No filter, then filters that leave the panic's own target out: the report reaches both sinks whatever the filter.
    for filter in [
        None,
        Some("wirecatch_core=debug"),
        Some("warn,wirecatch_logging=off"),
        Some("off"),
    ] {
        if let Err(problem) = a_panic_reaches_stderr_and_the_log_file_without_its_secrets(filter) {
            eprintln!("with the filter {filter:?}: {problem}");
            return ExitCode::FAILURE;
        }
    }
    // As the standard report does, `RUST_BACKTRACE` alone decides, whatever `RUST_LIB_BACKTRACE` says.
    for (panics, errors, shown) in [
        (Some("1"), Some("0"), true),
        (Some("full"), None, true),
        (None, Some("1"), false),
        (Some("0"), Some("1"), false),
    ] {
        if let Err(problem) = a_panic_carries_a_backtrace_when_asked(panics, errors, shown) {
            eprintln!(
                "with RUST_BACKTRACE={panics:?} and RUST_LIB_BACKTRACE={errors:?}: {problem}"
            );
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Sets `name` in `child` to `value`, or removes it.
fn set(child: &mut Command, name: &str, value: Option<&str>) {
    match value {
        Some(value) => child.env(name, value),
        None => child.env_remove(name),
    };
}

/// What the panicking child wrote to stderr and to its log file, with `RUST_BACKTRACE` set to `backtrace` and the log filter to `filter`.
fn run_child(
    filter: Option<&str>,
    backtrace: Option<&str>,
    lib_backtrace: Option<&str>,
) -> Result<(String, String), String> {
    let folder = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    child
        .env(CHILD_VARIABLE, "1")
        .env(LOG_FOLDER_VARIABLE, folder.path());
    set(&mut child, LOG_FILTER_VARIABLE, filter);
    set(&mut child, "RUST_BACKTRACE", backtrace);
    set(&mut child, "RUST_LIB_BACKTRACE", lib_backtrace);
    let ran = child.output().map_err(|e| e.to_string())?;
    if ran.status.success() {
        return Err("the child did not fail".into());
    }
    let stderr = String::from_utf8_lossy(&ran.stderr).into_owned();
    let file = std::fs::read_to_string(folder.path().join("panics.log"))
        .map_err(|e| format!("no log file: {e}; stderr:\n{stderr}"))?;
    Ok((stderr, file))
}

/// Whether `text` holds a backtrace: a line that starts with a frame's number (`  12: main`).
fn has_frames(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start()
            .split_once(": ")
            .is_some_and(|(number, _)| {
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
            })
    })
}

fn a_panic_carries_a_backtrace_when_asked(
    backtrace: Option<&str>,
    lib_backtrace: Option<&str>,
    shown: bool,
) -> Result<(), String> {
    let (stderr, file) = run_child(None, backtrace, lib_backtrace)?;
    for (sink, text) in [("stderr", &*stderr), ("the log file", &*file)] {
        if has_frames(text) != shown {
            let what = if shown { "no backtrace" } else { "a backtrace" };
            return Err(format!("{sink} holds {what}:\n{text}"));
        }
    }
    Ok(())
}

fn panic_with_a_secret() -> ! {
    let logging = wirecatch_logging::start("panics", "info");
    // The test names the folder through the log folder variable.
    logging.write_to(Some(&wirecatch_logging::folder_in(Path::new("."))));
    tracing::info!("about to fail");
    panic!("could not fetch https://ada:PANICPW@h.example/f?sig=PANICSIG");
}

fn a_panic_reaches_stderr_and_the_log_file_without_its_secrets(
    filter: Option<&str>,
) -> Result<(), String> {
    let (stderr, file) = run_child(filter, Some("1"), None)?;
    for (sink, text) in [("stderr", &*stderr), ("the log file", &*file)] {
        for secret in ["PANICPW", "PANICSIG"] {
            if text.contains(secret) {
                return Err(format!("{secret} reached {sink}:\n{text}"));
            }
        }
        if !text.contains("could not fetch https://ada:***@h.example/f?sig=***") {
            return Err(format!("the panic is missing from {sink}:\n{text}"));
        }
        if filter.is_none() && !text.contains("about to fail") {
            return Err(format!(
                "the line before the panic is missing from {sink}:\n{text}"
            ));
        }
    }
    Ok(())
}
