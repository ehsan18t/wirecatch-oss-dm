//! A stand-in for the app, used by the tests of `wirecatch_streams::process` (`cargo test -p wirecatch-streams` builds it): runs the program and arguments it is given through `process::run` until it is ended from outside, and passes on each line the program writes.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use wirecatch_engine::CancellationToken;
use wirecatch_streams::process::{self, RunOptions};

fn main() -> ExitCode {
    let mut words = std::env::args_os().skip(1);
    let Some(program) = words.next() else {
        return ExitCode::FAILURE;
    };
    let args: Vec<_> = words.collect();
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::FAILURE;
    };
    let ran = runtime.block_on(process::run(
        Path::new(&program),
        &args,
        &std::env::temp_dir(),
        RunOptions::lines(),
        &CancellationToken::new(),
        |_, line| {
            let mut out = std::io::stdout();
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                std::process::abort();
            }
        },
    ));
    if ran.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
