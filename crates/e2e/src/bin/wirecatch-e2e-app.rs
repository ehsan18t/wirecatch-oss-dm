//! Stands in for `wirecatch-app --background` in the end-to-end tests: the bridge
//! starts it (through its test launch override) when nothing listens on the test
//! endpoint.
//!
//! ```text
//! wirecatch-e2e-app --endpoint <pipe or socket> --root <folder> [--background]
//! ```
//!
//! It runs the manager with no UI, data in `<root>/data` and downloads under
//! `<root>/home`, serves the endpoint, and exits once its first client has gone
//! away (or after a minute, so a failed test never leaves it running). Just before
//! exiting it creates `<root>/app-exited`, so the test knows the folder is free.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use wirecatch_core::KnownFolders;
use wirecatch_e2e::app::App;
use wirecatch_ipc::Endpoint;

/// The longest this process runs, whatever happens.
const LIFETIME: Duration = Duration::from_secs(60);

/// Created in the root folder just before the process exits.
const EXITED_MARKER: &str = "app-exited";

fn main() -> ExitCode {
    let (endpoint, root) = match parse_args() {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("wirecatch-e2e-app: {e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("wirecatch-e2e-app: could not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let served = runtime.block_on(async {
        let folders = KnownFolders::under_home(root.join("home"));
        let app = App::start_at(&endpoint, &root.join("data"), folders).await?;
        let mut ended = app.ended();
        let _ = tokio::time::timeout(LIFETIME, ended.wait_for(|n| *n >= 1)).await;
        app.quit().await;
        anyhow::Ok(())
    });
    drop(runtime);
    let _ = std::fs::write(root.join(EXITED_MARKER), b"");
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wirecatch-e2e-app: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> anyhow::Result<(Endpoint, PathBuf)> {
    let mut endpoint = None;
    let mut root = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--endpoint") => endpoint = args.next().map(Endpoint::from_raw),
            Some("--root") => root = args.next().map(PathBuf::from),
            Some("--background") => {}
            _ => anyhow::bail!("unknown argument {arg:?}"),
        }
    }
    let missing = |what: &str| anyhow::anyhow!("{what} is required");
    Ok((
        endpoint.ok_or_else(|| missing("--endpoint"))?,
        root.ok_or_else(|| missing("--root"))?,
    ))
}
