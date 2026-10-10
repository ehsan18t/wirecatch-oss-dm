//! Runs the download manager without any UI and prints every event as a JSON line.
//! Meant for trying the manager by hand.
//!
//! ```text
//! cargo run -p wirecatch-core --example headless -- [--data-dir DIR] [URL...]
//! ```
//!
//! Each URL is added to the default queue. Without `--data-dir` a new temporary
//! folder is used (and kept, so a second run can reuse it). Press Ctrl+C to stop:
//! running downloads are saved and continue on the next run with the same folder.
//! Set `WIRECATCH_LOG=debug` for more detail.

use std::path::PathBuf;

use anyhow::{Context, bail};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};
use wirecatch_protocol::app::AddRequest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("WIRECATCH_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let (data_dir, urls) = parse_args()?;
    let folders = KnownFolders::detect().context("could not find your home folder")?;
    eprintln!("Data folder: {}", data_dir.display());
    eprintln!("Downloads go to: {}", folders.downloads.display());

    let manager = Manager::open(ManagerConfig::new(&data_dir, folders)).await?;
    print_events(&manager);

    for url in urls {
        let request = AddRequest {
            url,
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        };
        match manager.add_download(request).await {
            Ok(id) => eprintln!("Added download {}", id.0),
            Err(e) => eprintln!("Could not add: {e}"),
        }
    }

    tokio::signal::ctrl_c().await?;
    eprintln!("Stopping; running downloads continue on the next run.");
    manager.shutdown().await;
    Ok(())
}

fn parse_args() -> anyhow::Result<(PathBuf, Vec<String>)> {
    let mut data_dir = None;
    let mut urls = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                data_dir = Some(PathBuf::from(
                    args.next().context("--data-dir needs a folder")?,
                ));
            }
            flag if flag.starts_with("--") => bail!("unknown option {flag}"),
            _ => urls.push(arg),
        }
    }
    let data_dir = match data_dir {
        Some(dir) => dir,
        None => tempfile::Builder::new()
            .prefix("wirecatch-headless-")
            .tempdir()?
            .keep(),
    };
    Ok((data_dir, urls))
}

/// Prints UI events and socket events, one JSON object per line.
fn print_events(manager: &Manager) {
    tokio::spawn(print_all("ui", manager.subscribe()));
    tokio::spawn(print_all("socket", manager.subscribe_events()));
}

async fn print_all<T: Clone + Serialize>(label: &'static str, mut events: broadcast::Receiver<T>) {
    loop {
        match events.recv().await {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(json) => println!("{label} {json}"),
                Err(e) => eprintln!("could not print an event: {e}"),
            },
            Err(RecvError::Lagged(missed)) => eprintln!("({missed} {label} events skipped)"),
            Err(RecvError::Closed) => return,
        }
    }
}
