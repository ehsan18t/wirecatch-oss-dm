//! HTTP/3 against HTTP/2 on localhost.
//!
//! `cargo run --release -p wirecatch-engine --features http3 --example bench_http3`
//!
//! Serves a 256 MiB file over HTTPS on TCP (HTTP/2), advertising an HTTP/3
//! server on the same machine, and downloads it with HTTP/3 off and on, with 4
//! and 8 connections. Prints wall time, throughput, the processor time the
//! whole process used (client and server together, since both run here) and
//! how much of the file came over HTTP/3. The first request always goes over
//! TCP, so part of every download does.
//!
//! The servers use a self-signed certificate, which the engine is told to
//! trust through `SSL_CERT_FILE` (read by the system certificate loader).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bytes::Bytes;
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, Url, download};
use wirecatch_test_support::TestCertificate;
use wirecatch_test_support::http3::{self as servers, Counts};

const MIB: u64 = 1024 * 1024;

fn main() {
    let cert = TestCertificate::new();
    let pem_path =
        std::env::temp_dir().join(format!("wirecatch-bench-h3-{}.pem", std::process::id()));
    std::fs::write(&pem_path, cert.pem()).unwrap();
    // SAFETY: no other thread exists yet; the runtime is built afterwards.
    unsafe { std::env::set_var("SSL_CERT_FILE", &pem_path) };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(cert));
    let _ = std::fs::remove_file(&pem_path);
}

async fn run(cert: TestCertificate) {
    let size = 256 * MIB;
    let data = Bytes::from(
        (0..size as usize)
            .map(|i| (i * 31 + i / 509) as u8)
            .collect::<Vec<u8>>(),
    );
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let (tcp_port, tcp) =
        servers::start_tcp(data.clone(), &cert, Some(format!(r#"h3=":{h3_port}""#))).await;
    let url = Url::parse(&format!("https://localhost:{tcp_port}/bench.bin")).unwrap();

    println!(
        "File: {} MiB over localhost. CPU is the whole process (client and server).\n",
        size / MIB
    );
    println!(
        "{:>8}  {:>11}  {:>8}  {:>8}  {:>8}  {:>9}",
        "protocol", "connections", "seconds", "MiB/s", "CPU s", "via HTTP/3"
    );
    // A warm-up run so certificate loading and first allocations are not timed.
    timed(&url, &data, 4, false).await;
    for connections in [4, 8] {
        for http3 in [false, true] {
            let mut best: Option<(f64, f64, f64)> = None;
            for _ in 0..3 {
                let before = (bytes(&tcp), bytes(&h3));
                let (secs, cpu) = timed(&url, &data, connections, http3).await;
                let (via_tcp, via_h3) = (bytes(&tcp) - before.0, bytes(&h3) - before.1);
                let share = via_h3 as f64 / (via_tcp + via_h3).max(1) as f64;
                if best.is_none_or(|(s, _, _)| secs < s) {
                    best = Some((secs, cpu, share));
                }
            }
            let (secs, cpu, share) = best.unwrap();
            println!(
                "{:>8}  {connections:>11}  {secs:>8.2}  {:>8.1}  {cpu:>8.2}  {:>8.0}%",
                if http3 { "HTTP/3" } else { "HTTP/2" },
                size as f64 / MIB as f64 / secs,
                share * 100.0
            );
        }
    }
}

fn bytes(counts: &Counts) -> u64 {
    counts.bytes.load(Ordering::SeqCst)
}

/// Wall time and process CPU time of one download, in seconds.
async fn timed(url: &Url, data: &Bytes, connections: usize, http3: bool) -> (f64, f64) {
    let dir = std::env::temp_dir().join(format!("wirecatch-bench-h3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut request = DownloadRequest::new(url.clone(), &dir);
    request.options.connections = connections;
    request.options.http3 = http3;

    let cpu_before = cpu_time();
    let start = Instant::now();
    let outcome = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .expect("download failed");
    let secs = start.elapsed().as_secs_f64();
    let cpu = (cpu_time() - cpu_before).as_secs_f64();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed")
    };
    assert!(
        std::fs::read(&path).unwrap() == data[..],
        "downloaded bytes differ from the source"
    );
    std::fs::remove_dir_all(&dir).unwrap();
    (secs, cpu)
}

/// User plus system time this process has used so far.
#[cfg(windows)]
fn cpu_time() -> Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the pseudo handle is always valid and all four pointers are to
    // live FILETIME values.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    assert!(ok != 0, "GetProcessTimes failed");
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // FILETIME counts 100 ns ticks.
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// Not measured on this system.
#[cfg(not(any(windows, target_os = "linux")))]
fn cpu_time() -> Duration {
    Duration::ZERO
}

/// User plus system time this process has used so far.
#[cfg(target_os = "linux")]
fn cpu_time() -> Duration {
    // SAFETY: getrusage fills the struct it is given.
    let usage = unsafe {
        let mut usage = std::mem::zeroed::<libc::rusage>();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    let time = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    time(usage.ru_utime) + time(usage.ru_stime)
}
