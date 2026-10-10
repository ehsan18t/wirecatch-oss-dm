//! Local benchmark and manual test server.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local`
//!     Measures throughput for 1 to 16 connections against a local server that
//!     caps each connection's speed (the way many CDNs and file hosts do), then
//!     against an uncapped server to show the engine's own ceiling.
//!
//! `cargo run --release -p wirecatch-engine --example bench_local -- serve [port] [MiB/s per connection]`
//!     Keeps a capped server running for trying the CLI by hand.

#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Config, TestServer, test_data};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, download};

const MIB: u64 = 1024 * 1024;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        let port: u16 = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(8765);
        let cap: u64 = args.get(2).and_then(|c| c.parse().ok()).unwrap_or(4);
        serve(port, cap).await;
        return;
    }

    let size = 256 * MIB;
    let cap = 16 * MIB;
    println!(
        "File: {} MiB. Server caps each connection at {} MiB/s.\n",
        size / MIB,
        cap / MIB
    );
    let capped = TestServer::start(
        test_data(size as usize),
        Config {
            ranges: true,
            per_connection_bps: Some(cap),
            ..Default::default()
        },
    )
    .await;
    println!(
        "{:>12}  {:>9}  {:>10}  {:>8}",
        "connections", "seconds", "MiB/s", "speedup"
    );
    let mut baseline = None;
    for connections in [1, 2, 4, 8, 16] {
        let secs = timed_download(&capped, connections).await;
        let base = *baseline.get_or_insert(secs);
        println!(
            "{connections:>12}  {secs:>9.2}  {:>10.1}  {:>7.1}x",
            size as f64 / MIB as f64 / secs,
            base / secs
        );
    }

    println!("\nSame file, uncapped localhost (engine and disk ceiling):\n");
    let open = TestServer::start(
        test_data(size as usize),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    println!("{:>12}  {:>9}  {:>10}", "connections", "seconds", "MiB/s");
    for connections in [1, 8] {
        let secs = timed_download(&open, connections).await;
        println!(
            "{connections:>12}  {secs:>9.2}  {:>10.1}",
            size as f64 / MIB as f64 / secs
        );
    }
}

async fn timed_download(server: &TestServer, connections: usize) -> f64 {
    let dir = std::env::temp_dir().join(format!("wirecatch-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut request = DownloadRequest::new(server.url("bench.bin"), &dir);
    request.options.connections = connections;

    let start = Instant::now();
    let outcome = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .expect("download failed");
    let secs = start.elapsed().as_secs_f64();

    let Outcome::Completed { path, bytes, .. } = outcome else {
        panic!("not completed")
    };
    assert_eq!(bytes, server.data.len() as u64);
    assert!(
        std::fs::read(&path).unwrap() == server.data,
        "downloaded bytes differ from the source"
    );
    std::fs::remove_dir_all(&dir).unwrap();
    secs
}

async fn serve(port: u16, cap_mib: u64) {
    let server = TestServer::start(
        test_data(512 * MIB as usize),
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
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("port in use");
    println!(
        "Serving 512 MiB at http://127.0.0.1:{port}/file.bin ({cap_mib} MiB/s per connection). Ctrl+C to stop."
    );
    loop {
        let (mut inbound, _) = listener.accept().await.unwrap();
        let target = target.clone();
        tokio::spawn(async move {
            if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            }
        });
        tokio::time::sleep(Duration::ZERO).await;
    }
}
