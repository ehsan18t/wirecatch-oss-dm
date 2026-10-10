//! HTTP/3 against local servers with a test certificate: used when
//! advertised, ignored when switched off, and abandoned for TCP when QUIC is
//! blocked or breaks mid-download.

#[path = "../../tests/common/h3_server.rs"]
mod h3_server;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::download::{DownloadRequest, Outcome, download_with};
use crate::http::Clients;
use crate::progress::Progress;
use h3_server::{Cert, Counts};

#[test]
fn alt_svc_values() {
    let host = "files.example";
    assert_eq!(h3_port(r#"h3=":443"; ma=86400"#, host), Some(443));
    assert_eq!(
        h3_port(r#"h3-29=":8443", h3=":4433"; ma=60"#, host),
        Some(4433)
    );
    assert_eq!(h3_port(r#"h3="files.example:443""#, host), Some(443));
    assert_eq!(h3_port(r#"h3="FILES.example:443""#, host), Some(443));
    // Another host would need its own trust decisions; not used.
    assert_eq!(h3_port(r#"h3="cdn.example:443""#, host), None);
    assert_eq!(h3_port(r#"h3-29=":443""#, host), None);
    assert_eq!(h3_port("clear", host), None);
    assert_eq!(h3_port(r#"h3=":0""#, host), None);
    assert_eq!(h3_port(r#"h2=":443""#, host), None);
}

fn test_data(len: usize) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|i| (i * 31 + i / 509) as u8)
            .collect::<Vec<u8>>(),
    )
}

const MIB: usize = 1024 * 1024;

/// Downloads from the TCP server on `tcp_port` with HTTP/3 switched on or off.
async fn fetch(cert: &Cert, tcp_port: u16, http3: bool, test: &str) -> (Vec<u8>, Duration) {
    let dir = std::env::temp_dir().join(format!("wirecatch-h3-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let url = Url::parse(&format!("https://127.0.0.1:{tcp_port}/file.bin")).unwrap();
    let mut request = DownloadRequest::new(url, &dir);
    request.options.connections = 4;
    request.options.http3 = http3;
    let clients = Clients::with_roots(&request.options, cert.roots.clone());
    let started = Instant::now();
    let outcome = download_with(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
        clients,
    )
    .await
    .unwrap();
    let took = started.elapsed();
    let Outcome::Completed { path, .. } = outcome else {
        panic!("expected Completed, got {outcome:?}");
    };
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    (bytes, took)
}

fn requests(counts: &Counts) -> usize {
    counts.requests.load(Ordering::SeqCst)
}

#[tokio::test]
async fn downloads_over_http3_when_the_server_advertises_it() {
    let data = test_data(8 * MIB + 777);
    let cert = Cert::new();
    let (h3_port, h3) = h3_server::start_h3(data.clone(), &cert).await;
    let alt_svc = format!(r#"h3=":{h3_port}"; ma=3600"#);
    let (tcp_port, tcp) = h3_server::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, true, "used").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&tcp), 1, "only the first request went over TCP");
    assert!(requests(&h3) >= 3, "the other connections used HTTP/3");
}

#[tokio::test]
async fn http3_is_not_used_unless_switched_on() {
    let data = test_data(4 * MIB);
    let cert = Cert::new();
    let (h3_port, h3) = h3_server::start_h3(data.clone(), &cert).await;
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = h3_server::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, false, "off").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&h3), 0);
    assert!(requests(&tcp) >= 2);
}

#[tokio::test]
async fn falls_back_to_tcp_when_quic_is_blocked() {
    let data = test_data(8 * MIB);
    let cert = Cert::new();
    let (_socket, blocked_port) = h3_server::blackhole();
    let alt_svc = format!(r#"h3=":{blocked_port}""#);
    let (tcp_port, tcp) = h3_server::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, took) = fetch(&cert, tcp_port, true, "blocked").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert!(requests(&tcp) >= 2, "the other connections went over TCP");
    // One handshake timeout (3 s), not one per connection or per range.
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[tokio::test]
async fn falls_back_to_tcp_when_an_http3_stream_breaks() {
    let data = test_data(8 * MIB);
    let cert = Cert::new();
    let (h3_port, h3) = h3_server::start_h3_breaking(data.clone(), &cert, Some(100_000)).await;
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = h3_server::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, true, "broken").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert!(requests(&h3) >= 1, "HTTP/3 was tried");
    assert!(requests(&tcp) >= 2, "and abandoned for TCP");
}
