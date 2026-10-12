//! Local benchmark and manual test server.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local [-- bench [runs]]`
//!     Measures throughput for 1 to 16 connections against a local server that
//!     caps each connection's speed (the way many CDNs and file hosts do), then
//!     against an uncapped server to show the engine's own ceiling, with and
//!     without a 10 MiB limit on the size of each request (as video sites need).
//!     Each case runs `runs` times (default 5) and reports the median time, with
//!     the fastest and slowest run as the spread: one run of the uncapped cases
//!     varies by about 40 percent, too much to show a regression.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local -- serve [port] [MiB/s per connection]`
//!     Keeps a capped server running for trying the CLI by hand.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local -- serve-ftp [port] [MiB/s per connection]`
//!     The same with an FTP server (anonymous login, at most 4 sessions).

use std::error::Error;
use std::io::Write;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wirecatch_engine::numeric::{approx_f64, saturating_cast};
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
/// Runs of each case when the command line does not say: enough for the median to
/// stand still while one run is disturbed by the rest of the machine.
const DEFAULT_RUNS: NonZeroUsize = NonZeroUsize::new(5).unwrap();
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
        Some("bench") | None => {
            let runs = match args.get(1) {
                Some(runs) => runs
                    .parse()
                    .map_err(|e| format!("runs {runs:?} must be a whole number from 1: {e}"))?,
                None => DEFAULT_RUNS,
            };
            bench(runs).await
        }
        Some(other) => Err(format!(
            "unknown command {other:?}; use bench [runs], serve [port] [MiB/s] or serve-ftp [port] [MiB/s]"
        )
        .into()),
    }
}

async fn bench(runs: NonZeroUsize) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "File: {} MiB. Server caps each connection at {} MiB/s. Each case runs {runs} times: the median, and the fastest and slowest run.\n",
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
        "{:>12}  {:>9}  {:>15}  {:>10}  {:>8}",
        "connections", "seconds", "spread", "MiB/s", "speedup"
    )?;
    let mut baseline = None;
    for connections in CAPPED_CONNECTIONS {
        let times = Times::measure(runs, || timed_download(&capped, connections, None)).await?;
        let base = *baseline.get_or_insert(times.median);
        writeln!(
            out,
            "{connections:>12}  {:>9.2}  {:>15}  {:>10.1}  {:>7.1}x",
            times.median,
            times.spread(),
            mib_per_second(times.median),
            base / times.median
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
            "{:>12}  {:>9}  {:>15}  {:>10}",
            "connections", "seconds", "spread", "MiB/s"
        )?;
        for connections in UNCAPPED_CONNECTIONS {
            let times =
                Times::measure(runs, || timed_download(&open, connections, max_request)).await?;
            writeln!(
                out,
                "{connections:>12}  {:>9.2}  {:>15}  {:>10.1}",
                times.median,
                times.spread(),
                mib_per_second(times.median)
            )?;
        }
    }
    Ok(())
}

/// The times, in seconds, of the runs of one case.
struct Times {
    median: f64,
    fastest: f64,
    slowest: f64,
}

impl Times {
    /// Runs `case` `runs` times, one after another.
    async fn measure<F, Fut>(runs: NonZeroUsize, mut case: F) -> Result<Self>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<f64>>,
    {
        // At least one run, so every index below is in range.
        let mut secs = vec![case().await?];
        for _ in 1..runs.get() {
            secs.push(case().await?);
        }
        secs.sort_by(f64::total_cmp);
        let middle = secs.len() / 2;
        let median = if secs.len() % 2 == 0 {
            f64::midpoint(secs[middle - 1], secs[middle])
        } else {
            secs[middle]
        };
        Ok(Self {
            median,
            fastest: secs.iter().copied().fold(f64::INFINITY, f64::min),
            slowest: secs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        })
    }

    /// The fastest and the slowest run, as `0.81 to 1.13`.
    fn spread(&self) -> String {
        format!("{:.2} to {:.2}", self.fastest, self.slowest)
    }
}

/// The benchmark file's size over `secs`, in MiB a second.
fn mib_per_second(secs: f64) -> f64 {
    approx_f64(saturating_cast(BENCH_SIZE / MIB_USIZE)) / secs
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
    if bytes != saturating_cast::<u64, _>(server.data.len()) || std::fs::read(&path)? != server.data
    {
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
                either_side_may_close(
                    &tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await,
                );
            }
        });
        tokio::time::sleep(Duration::ZERO).await;
    }
}

/// Takes the outcome of forwarding one connection. Either side closing ends
/// it, which is how every forwarding ends, so there is nothing to report.
fn either_side_may_close(forwarded: &std::io::Result<(u64, u64)>) {
    let (Ok(_) | Err(_)) = forwarded;
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
