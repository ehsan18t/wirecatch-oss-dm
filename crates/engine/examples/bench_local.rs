//! Local benchmark and manual test server.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local`
//!     Measures throughput for 1 to 16 connections against a local server that
//!     caps each connection's speed (the way many CDNs and file hosts do), then
//!     against an uncapped server to show the engine's own ceiling, with and
//!     without a 10 MiB limit on the size of each request (as video sites need).
//!
//! `cargo run --release -p wirecatch-engine --example bench_local -- serve [port] [MiB/s per connection]`
//!     Keeps a capped server running for trying the CLI by hand.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local -- serve-ftp [port] [MiB/s per connection]`
//!     The same with an FTP server (anonymous login, at most 4 sessions).

use std::error::Error;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wirecatch_engine::numeric::approx_f64;
use wirecatch_engine::units::{MIB, MIB_USIZE};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, download};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::{ftp, test_data};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
/// The file the benchmark downloads.
const BENCH_SIZE: usize = 256 * MIB_USIZE;
/// Each connection's speed on the capped server.
const BENCH_CAP: u64 = 16 * MIB;
/// The request size limit of the last run, as video sites need.
const BENCH_MAX_REQUEST: u64 = 10 * MIB;
/// Connection counts for the capped and the uncapped runs.
const CAPPED_CONNECTIONS: [usize; 5] = [1, 2, 4, 8, 16];
const UNCAPPED_CONNECTIONS: [usize; 2] = [1, 8];

/// What `serve` and `serve-ftp` use when not told otherwise.
const DEFAULT_HTTP_PORT: u16 = 8765;
const DEFAULT_FTP_PORT: u16 = 2121;
const DEFAULT_CAP_MIB: u64 = 4;
const SERVE_HTTP_SIZE: usize = 512 * MIB_USIZE;
const SERVE_FTP_SIZE: usize = 256 * MIB_USIZE;
const SERVE_FTP_SESSIONS: usize = 4;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = |default| args.get(1).and_then(|p| p.parse().ok()).unwrap_or(default);
    let cap: u64 = args
        .get(2)
        .and_then(|c| c.parse().ok())
        .unwrap_or(DEFAULT_CAP_MIB);
    match args.first().map(String::as_str) {
        Some("serve") => serve(port(DEFAULT_HTTP_PORT), cap).await,
        Some("serve-ftp") => serve_ftp(port(DEFAULT_FTP_PORT), cap).await,
        _ => bench().await,
    }
}

async fn bench() -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "File: {} MiB. Server caps each connection at {} MiB/s.\n",
        BENCH_SIZE / MIB_USIZE,
        BENCH_CAP / MIB
    )?;
    let capped = TestServer::start(
        test_data(BENCH_SIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(BENCH_CAP),
            ..Default::default()
        },
    )
    .await;
    writeln!(
        out,
        "{:>12}  {:>9}  {:>10}  {:>8}",
        "connections", "seconds", "MiB/s", "speedup"
    )?;
    let mut baseline = None;
    for connections in CAPPED_CONNECTIONS {
        let secs = timed_download(&capped, connections, None).await?;
        let base = *baseline.get_or_insert(secs);
        writeln!(
            out,
            "{connections:>12}  {secs:>9.2}  {:>10.1}  {:>7.1}x",
            mib_per_second(secs),
            base / secs
        )?;
    }

    writeln!(
        out,
        "\nSame file, uncapped localhost (engine and disk ceiling):\n"
    )?;
    let open = TestServer::start(
        test_data(BENCH_SIZE),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    for max_request in [None, Some(BENCH_MAX_REQUEST)] {
        if max_request.is_some() {
            writeln!(out, "\nSame, with requests of at most 10 MiB each:\n")?;
        }
        writeln!(
            out,
            "{:>12}  {:>9}  {:>10}",
            "connections", "seconds", "MiB/s"
        )?;
        for connections in UNCAPPED_CONNECTIONS {
            let secs = timed_download(&open, connections, max_request).await?;
            writeln!(
                out,
                "{connections:>12}  {secs:>9.2}  {:>10.1}",
                mib_per_second(secs)
            )?;
        }
    }
    Ok(())
}

/// The benchmark file's size over `secs`, in MiB a second.
fn mib_per_second(secs: f64) -> f64 {
    approx_f64((BENCH_SIZE / MIB_USIZE) as u64) / secs
}

async fn timed_download(
    server: &TestServer,
    connections: usize,
    max_request: Option<u64>,
) -> Result<f64> {
    let dir = std::env::temp_dir().join(format!("wirecatch-bench-{}", std::process::id()));
    // Left over from an interrupted run, if it exists at all.
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let mut request = DownloadRequest::new(server.url("bench.bin"), &dir);
    request.options.connections = connections;
    request.options.max_request_bytes = max_request;

    let start = Instant::now();
    let outcome = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await?;
    let secs = start.elapsed().as_secs_f64();

    let Outcome::Completed { path, bytes, .. } = outcome else {
        return Err(format!("the download did not complete: {outcome:?}").into());
    };
    if bytes != server.data.len() as u64 || std::fs::read(&path)? != server.data {
        return Err("downloaded bytes differ from the source".into());
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(secs)
}

async fn serve(port: u16, cap_mib: u64) -> Result<()> {
    let server = TestServer::start(
        test_data(SERVE_HTTP_SIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(cap_mib * MIB),
            etag: Some("\"bench\"".into()),
            ..Default::default()
        },
    )
    .await;
    // The test server binds a random port; forward a fixed one to it for convenience.
    let target = server.base.trim_start_matches("http://").to_string();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    writeln!(
        std::io::stdout(),
        "Serving {} MiB at http://127.0.0.1:{port}/file.bin ({cap_mib} MiB/s per connection). Ctrl+C to stop.",
        SERVE_HTTP_SIZE / MIB_USIZE
    )?;
    loop {
        let (mut inbound, _) = listener.accept().await?;
        let target = target.clone();
        tokio::spawn(async move {
            if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                // Either side closing ends the forwarding; there is nothing to report.
                if let Err(error) = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await
                {
                    drop(error);
                }
            }
        });
        tokio::time::sleep(Duration::ZERO).await;
    }
}

async fn serve_ftp(port: u16, cap_mib: u64) -> Result<()> {
    let _server = ftp::FtpServer::start_on(
        port,
        ftp::FtpConfig {
            files: vec![("file.bin".to_string(), Arc::new(test_data(SERVE_FTP_SIZE)))],
            mdtm: Some("20260101000000".to_string()),
            max_sessions: Some((SERVE_FTP_SESSIONS, ftp::Refuse::Greeting421)),
            data_bps: Some(cap_mib * MIB),
            ..Default::default()
        },
    )
    .await;
    writeln!(
        std::io::stdout(),
        "Serving {} MiB at ftp://127.0.0.1:{port}/file.bin ({cap_mib} MiB/s per connection, at most {SERVE_FTP_SESSIONS} sessions). Ctrl+C to stop.",
        SERVE_FTP_SIZE / MIB_USIZE
    )?;
    tokio::signal::ctrl_c().await?;
    Ok(())
}
