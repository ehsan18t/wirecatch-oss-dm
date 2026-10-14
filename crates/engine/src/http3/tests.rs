//! HTTP/3 against local servers with a test certificate: used when
//! advertised, ignored when switched off, and abandoned for TCP when QUIC is
//! blocked or breaks mid-download.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::download::{DownloadRequest, Outcome, download_with};
use crate::http::Clients;
use crate::progress::Progress;
use crate::units::{MIB, MIB_USIZE};
use reqwest::header::HeaderValue;
use std::ffi::OsString;
use wirecatch_test_support::http3::{self as servers, Counts};
use wirecatch_test_support::{TestCertificate, private_dir};

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

#[test]
fn http3_is_not_used_through_a_proxy() {
    use std::ffi::OsString;

    let opts = crate::options::DownloadOptions::default();
    let clients = Clients::with_roots(&opts, rustls::RootCertStore::empty());
    let url = Url::parse("https://files.example/a.iso").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("alt-svc", HeaderValue::from_static(r#"h3=":443""#));
    let advertised = |env: crate::proxy::Env<'_>| {
        Origin::advertised(&url, &headers, None, clients.tls(), &opts, env).is_some()
    };

    assert!(advertised(&|_| None), "no proxy: HTTP/3 is used");
    for variable in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        let env = move |name: &str| (name == variable).then(|| OsString::from("http://proxy:8080"));
        assert!(
            !advertised(&env),
            "{variable} is set: QUIC would go around it"
        );
    }
    // An empty variable sets no proxy, and an HTTP-only proxy does not apply.
    assert!(advertised(
        &|name: &str| (name == "HTTPS_PROXY").then(OsString::new)
    ));
    assert!(advertised(&|name: &str| {
        (name == "HTTP_PROXY").then(|| OsString::from("http://proxy:8080"))
    }));
    // A host the proxy settings exclude is reached directly, so QUIC goes nowhere the client would not.
    assert!(advertised(&|name: &str| match name {
        "HTTPS_PROXY" => Some(OsString::from("http://proxy:8080")),
        "NO_PROXY" => Some(OsString::from("other.example,.files.example")),
        _ => None,
    }));
}

/// An answer over HTTP/3 about another version of the file is a changed file,
/// as over TCP, not a reason to give up HTTP/3.
#[test]
fn an_answer_about_another_version_of_the_file_is_a_changed_file() {
    let context = RequestContext {
        url: Url::parse("https://files.example/a.iso").unwrap(),
        ranged: true,
        total: Some(100),
        validators: http::Validators {
            etag: Some("\"v1\"".into()),
            ..http::Validators::default()
        },
        cancel: CancellationToken::new(),
        stall_timeout: Duration::from_secs(5),
    };
    let answer = |etag: &'static str| {
        ::http::Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("content-range", "bytes 10-19/100")
            .header("etag", etag)
            .body(())
            .unwrap()
    };
    assert!(check_response(&answer("\"v1\""), &context, 10).is_ok());
    assert!(matches!(
        check_response(&answer("\"v2\""), &context, 10),
        Err(H3Failure::Answer(Failure {
            error: Error::RemoteChanged,
            ..
        }))
    ));
}

fn test_data(len: usize) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|i| (i * 31 + i / 509).to_le_bytes()[0])
            .collect::<Vec<u8>>(),
    )
}

/// Downloads from the TCP server on `tcp_port` with HTTP/3 switched on or off.
/// No retries are allowed: falling back from HTTP/3 must never need one.
async fn fetch(
    cert: &TestCertificate,
    tcp_port: u16,
    http3: bool,
    test: &str,
) -> (Vec<u8>, Duration) {
    fetch_from(cert, "127.0.0.1", tcp_port, http3, test, |_| {}).await
}

async fn fetch_from(
    cert: &TestCertificate,
    host: &str,
    tcp_port: u16,
    http3: bool,
    test: &str,
    tune: impl FnOnce(&mut crate::options::DownloadOptions),
) -> (Vec<u8>, Duration) {
    let dir = private_dir(&format!("h3-{test}")).unwrap();
    let url = Url::parse(&format!("https://{host}:{tcp_port}/file.bin")).unwrap();
    let mut request = DownloadRequest::new(url, dir.path());
    request.options.connections = 4;
    request.options.http3 = http3;
    request.options.max_retries = 0;
    tune(&mut request.options);
    let clients = Clients::with_roots(&request.options, cert.roots());
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
    dir.remove().unwrap();
    (bytes, took)
}

/// A download over HTTP/3 from 127.0.0.1 while the environment `env` reads is
/// in effect; how many requests went over HTTP/3.
async fn h3_requests_with(env: fn(&str) -> Option<OsString>, test: &str) -> usize {
    let data = test_data(8 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, _tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;
    let dir = private_dir(&format!("h3-{test}")).unwrap();
    let url = Url::parse(&format!("https://127.0.0.1:{tcp_port}/file.bin")).unwrap();
    let mut request = DownloadRequest::new(url, dir.path());
    request.options.connections = 4;
    request.options.http3 = true;
    let clients = Clients::with_roots(&request.options, cert.roots()).with_environment(env);
    let outcome = download_with(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
        clients,
    )
    .await
    .unwrap();
    let Outcome::Completed { path, .. } = outcome else {
        panic!("expected Completed, got {outcome:?}");
    };
    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    dir.remove().unwrap();
    requests(&h3)
}

/// The HTTP client reaches a host `NO_PROXY` names directly, so HTTP/3 may be
/// used for it; a `NO_PROXY` naming other hosts leaves the proxy, and QUIC
/// stays off.
#[tokio::test]
async fn no_proxy_decides_whether_http3_is_used_while_a_proxy_is_set() {
    fn exempt(name: &str) -> Option<OsString> {
        match name {
            "HTTPS_PROXY" => Some(OsString::from("http://proxy.example:8080")),
            "NO_PROXY" => Some(OsString::from("other.example, 127.0.0.0/8")),
            _ => None,
        }
    }
    fn proxied(name: &str) -> Option<OsString> {
        match name {
            "HTTPS_PROXY" => Some(OsString::from("http://proxy.example:8080")),
            "NO_PROXY" => Some(OsString::from("other.example")),
            _ => None,
        }
    }
    assert!(h3_requests_with(exempt, "no-proxy-exempt").await > 0);
    assert_eq!(h3_requests_with(proxied, "no-proxy-other").await, 0);
}

fn requests(counts: &Counts) -> usize {
    counts.requests.load(Ordering::SeqCst)
}

#[tokio::test]
async fn downloads_over_http3_when_the_server_advertises_it() {
    let data = test_data(8 * MIB_USIZE + 777);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let alt_svc = format!(r#"h3=":{h3_port}"; ma=3600"#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, true, "used").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&tcp), 1, "only the first request went over TCP");
    assert!(requests(&h3) >= 3, "the other connections used HTTP/3");
}

/// `localhost` may resolve to `::1` first while the servers only listen on
/// 127.0.0.1: QUIC must go to the address the first request reached.
#[tokio::test]
async fn quic_goes_to_the_address_the_first_request_reached() {
    let data = test_data(8 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, took) = fetch_from(&cert, "localhost", tcp_port, true, "localhost", |_| {}).await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&tcp), 1, "only the first request went over TCP");
    assert!(requests(&h3) >= 3, "the other connections used HTTP/3");
    assert!(
        took < Duration::from_secs(3),
        "no handshake timeout: {took:?}"
    );
}

#[tokio::test]
async fn http3_is_not_used_unless_switched_on() {
    let data = test_data(4 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, false, "off").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&h3), 0);
    assert!(requests(&tcp) >= 2);
    assert_eq!(
        tcp.http2.load(Ordering::SeqCst),
        requests(&tcp),
        "TCP requests used HTTP/2"
    );
}

#[tokio::test]
async fn falls_back_to_tcp_when_quic_is_blocked() {
    let data = test_data(8 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (_socket, blocked_port) = servers::blackhole();
    let alt_svc = format!(r#"h3=":{blocked_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, took) = fetch(&cert, tcp_port, true, "blocked").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert!(requests(&tcp) >= 2, "the other connections went over TCP");
    // One handshake timeout (3 s), not one per connection or per range.
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[tokio::test]
async fn falls_back_to_tcp_when_an_http3_stream_breaks() {
    let data = test_data(8 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (h3_port, h3) =
        servers::start_h3_breaking(data.clone(), &cert, Some(100_000), Duration::ZERO);
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let (bytes, _) = fetch(&cert, tcp_port, true, "broken").await;

    assert!(bytes == data, "downloaded bytes differ");
    assert!(requests(&h3) >= 1, "HTTP/3 was tried");
    assert!(
        requests(&h3) <= 4,
        "and given up after it broke, not tried again"
    );
    assert!(requests(&tcp) >= 2, "and abandoned for TCP");
}

/// The HTTP/3 connection breaks late in its range, after the only other
/// connection has finished and stopped. With no retries allowed, the range
/// must still finish over TCP: a switch of protocol is not a failed attempt.
#[tokio::test]
async fn falling_back_mid_stream_does_not_use_up_a_retry() {
    let data = test_data(16 * MIB_USIZE);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3_breaking(
        data.clone(),
        &cert,
        // Half of the 8 MiB the HTTP/3 connection asks for at most, so its
        // range breaks even when the first connection got a head start.
        Some(4 * MIB_USIZE),
        Duration::from_secs(1),
    );
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    // Two connections, up to 8 MiB each, and parts too big to split again: the
    // TCP connection finishes its part and stops while HTTP/3 waits to break.
    let (bytes, _) = fetch_from(&cert, "127.0.0.1", tcp_port, true, "late-break", |o| {
        o.connections = 2;
        o.min_split = 4 * MIB;
    })
    .await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&h3), 1);
    assert_eq!(requests(&tcp), 2, "the broken range went on over TCP");
}

/// With a limit on the size of one request, requests over HTTP/3 keep to it
/// too, and so does the first one over TCP.
#[tokio::test]
async fn http3_requests_keep_to_the_request_limit() {
    let data = test_data(8 * MIB_USIZE + 777);
    let cert = TestCertificate::new();
    let (h3_port, h3) = servers::start_h3(data.clone(), &cert);
    let alt_svc = format!(r#"h3=":{h3_port}""#);
    let (tcp_port, tcp) = servers::start_tcp(data.clone(), &cert, Some(alt_svc)).await;

    let limit = MIB / 2;
    let (bytes, _) = fetch_from(&cert, "127.0.0.1", tcp_port, true, "limited", |o| {
        o.max_request_bytes = Some(limit);
    })
    .await;

    assert!(bytes == data, "downloaded bytes differ");
    assert_eq!(requests(&tcp), 1, "only the first request went over TCP");
    assert!(requests(&h3) >= 8, "the rest came in pieces over HTTP/3");
    for (name, counts) in [("TCP", &tcp), ("HTTP/3", &h3)] {
        let largest = counts.largest_ask.load(Ordering::SeqCst);
        assert!(
            largest <= limit,
            "a request over {name} asked for {largest} bytes"
        );
    }
}
