//! Tests of the HTTP layer, some against a local TLS server.

use super::*;
use reqwest::header::{AUTHORIZATION, COOKIE};

#[test]
fn retry_after_forms() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
    assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
    let later = httpdate::fmt_http_date(now + Duration::from_secs(42));
    assert_eq!(
        parse_retry_after(&later, now),
        Some(Duration::from_secs(42))
    );
    // The two obsolete date formats HTTP still allows.
    let base = httpdate::fmt_http_date(now);
    assert_eq!(base, "Tue, 14 Nov 2023 22:13:20 GMT");
    assert_eq!(
        parse_retry_after("Tuesday, 14-Nov-23 22:13:30 GMT", now),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        parse_retry_after("Tue Nov 14 22:13:25 2023", now),
        Some(Duration::from_secs(5))
    );
    let earlier = httpdate::fmt_http_date(now - Duration::from_secs(60));
    assert_eq!(parse_retry_after(&earlier, now), Some(Duration::ZERO));
    assert_eq!(parse_retry_after("86400", now), Some(MAX_RETRY_AFTER));
    assert_eq!(
        parse_retry_after("99999999999999999999999", now),
        Some(MAX_RETRY_AFTER)
    );
    assert_eq!(parse_retry_after("-5", now), None);
    assert_eq!(parse_retry_after("soon", now), None);
    assert_eq!(parse_retry_after("", now), None);
}

#[test]
fn credentials_only_go_to_their_own_origin() {
    let mut given = HeaderMap::new();
    given.insert(COOKIE, HeaderValue::from_static("session=secret"));
    given.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
    given.insert("x-api-key", HeaderValue::from_static("secret"));
    given.insert(
        REFERER,
        HeaderValue::from_static("https://site.example/page"),
    );
    given.insert(USER_AGENT, HeaderValue::from_static("Browser/1.0"));
    let origin = Url::parse("https://files.example/a.bin").unwrap();
    let headers = RequestHeaders::new(&origin, &given);

    let url = |s: &str| Url::parse(s).unwrap();
    // Same origin, including the default port written out.
    assert_eq!(headers.for_url(&url("https://files.example/b.bin")), &given);
    assert_eq!(
        headers.for_url(&url("https://files.example:443/b.bin")),
        &given
    );

    for other in [
        "https://cdn.example/a.bin",        // other host
        "https://files.example:8443/a.bin", // other port
        "http://files.example/a.bin",       // other scheme (downgrade)
        "https://sub.files.example/a.bin",  // subdomain
    ] {
        let sent = headers.for_url(&url(other));
        assert!(sent.get(COOKIE).is_none(), "cookie leaked to {other}");
        assert!(sent.get(AUTHORIZATION).is_none(), "auth leaked to {other}");
        assert!(sent.get("x-api-key").is_none(), "key leaked to {other}");
        // Other origins only learn which site the link came from.
        assert_eq!(sent.get(REFERER).unwrap(), "https://site.example/");
        assert_eq!(sent.get(USER_AGENT), given.get(USER_AGENT));
    }
}

#[test]
fn referer_is_cut_to_its_origin_for_other_origins() {
    let cut = |v: &'static str| referer_origin(&HeaderValue::from_static(v));
    assert_eq!(
        cut("http://site.example:8080/a/b?token=1#x").unwrap(),
        "http://site.example:8080/"
    );
    assert_eq!(
        cut("https://site.example:443/a").unwrap(),
        "https://site.example/"
    );
    assert_eq!(cut("not a url"), None);
    assert_eq!(cut("file:///etc/passwd"), None);
}

#[test]
fn url_credentials_become_an_origin_bound_header() {
    let mut headers = HeaderMap::new();
    let url = Url::parse("https://us%40er:p%3Ass@files.example/a.bin").unwrap();
    let clean = split_userinfo(&url, &mut headers);
    assert_eq!(clean.as_str(), "https://files.example/a.bin");
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("us@er:p:ss")
    );
    assert_eq!(headers.get(AUTHORIZATION).unwrap(), expected.as_str());

    // A header the caller set wins over the URL.
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer mine"));
    split_userinfo(&url, &mut headers);
    assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer mine");

    // No credentials, nothing added.
    let mut headers = HeaderMap::new();
    let plain = Url::parse("https://files.example/a.bin").unwrap();
    assert_eq!(split_userinfo(&plain, &mut headers), plain);
    assert!(headers.is_empty());
}

/// Against a local TLS server: session resumption, checked from the server
/// side (it records whether each handshake was full or resumed), and
/// redirects from https to http.
mod tls {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use rustls::HandshakeKind;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct TlsServer {
        url: Url,
        roots: rustls::RootCertStore,
        handshakes: Arc<Mutex<Vec<HandshakeKind>>>,
    }

    const OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

    /// Answers every request with a tiny body and closes the connection, so
    /// each request needs a new TCP connection and a new TLS handshake.
    async fn start_tls_server() -> TlsServer {
        start_tls_server_answering(OK.to_vec()).await
    }

    /// Reads one request's headers from `stream`; false if it closed first.
    async fn read_request(stream: &mut (impl AsyncReadExt + Unpin)) -> bool {
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return false,
                Ok(n) => request.extend_from_slice(&buf[..n]),
            }
        }
        true
    }

    /// Writes `response` to `stream` and closes it.
    async fn answer(stream: &mut (impl AsyncWriteExt + Unpin), response: &[u8]) {
        let written = stream.write_all(response).await;
        let closed = stream.shutdown().await;
        client_may_hang_up(&written.and(closed));
    }

    /// Takes the outcome of answering a client. One that hung up first
    /// fails its own request, so the server has nothing to report.
    fn client_may_hang_up(answered: &std::io::Result<()>) {
        let (Ok(()) | Err(_)) = answered;
    }

    /// Answers every request with `response` and closes the connection.
    async fn start_tls_server_answering(response: Vec<u8>) -> TlsServer {
        let cert = wirecatch_test_support::TestCertificate::new();
        let roots = cert.roots();
        let config = cert.server_config(&[]);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handshakes = Arc::new(Mutex::new(Vec::new()));
        {
            let handshakes = handshakes.clone();
            tokio::spawn(async move {
                loop {
                    let (tcp, _) = listener.accept().await.unwrap();
                    let acceptor = acceptor.clone();
                    let handshakes = handshakes.clone();
                    let response = response.clone();
                    tokio::spawn(async move {
                        let Ok(mut tls) = acceptor.accept(tcp).await else {
                            return;
                        };
                        if let Some(kind) = tls.get_ref().1.handshake_kind() {
                            handshakes.lock().unwrap().push(kind);
                        }
                        if !read_request(&mut tls).await {
                            return;
                        }
                        answer(&mut tls, &response).await;
                    });
                }
            });
        }
        TlsServer {
            url: Url::parse(&format!("https://{addr}/file.bin")).unwrap(),
            roots,
            handshakes,
        }
    }

    async fn get(client: &Client, url: &Url) {
        let body = client.get(url.clone()).send().await.unwrap();
        assert_eq!(body.bytes().await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn connections_of_one_download_resume_the_first_session() {
        let server = start_tls_server().await;
        let opts = DownloadOptions::default();
        let clients = Clients::with_roots(&opts, server.roots.clone());

        // One client per worker, exactly as `download` builds them.
        for _ in 0..4 {
            get(&clients.build().unwrap(), &server.url).await;
        }

        let kinds = server.handshakes.lock().unwrap().clone();
        assert_eq!(
            kinds,
            vec![
                HandshakeKind::Full,
                HandshakeKind::Resumed,
                HandshakeKind::Resumed,
                HandshakeKind::Resumed
            ],
            "4 connections, only the first with a full handshake"
        );
    }

    /// A plain HTTP server answering `OK`; returns its URL and request count.
    async fn start_plain_server() -> (Url, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        tokio::spawn(async move {
            loop {
                let (mut tcp, _) = listener.accept().await.unwrap();
                let counter = counter.clone();
                tokio::spawn(async move {
                    if read_request(&mut tcp).await {
                        counter.fetch_add(1, Ordering::SeqCst);
                        answer(&mut tcp, OK).await;
                    }
                });
            }
        });
        let url = Url::parse(&format!("http://{addr}/file.bin")).unwrap();
        (url, requests)
    }

    #[tokio::test]
    async fn https_to_http_redirects_are_refused_unless_allowed() {
        let (plain, plain_requests) = start_plain_server().await;
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: {plain}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let server = start_tls_server_answering(redirect.into_bytes()).await;
        let mut opts = DownloadOptions::default();
        let client = Clients::with_roots(&opts, server.roots.clone())
            .build()
            .unwrap();

        // Refused by default, before anything is sent over plain HTTP.
        let (session, url) = Session::new(&server.url, &opts, CancellationToken::new());
        let failure = send(&client, &url, &session, None).await.unwrap_err();
        match failure.error {
            Error::InsecureRedirect { from, to } => {
                assert_eq!(from, server.url.as_str());
                assert_eq!(to, plain.as_str());
            }
            other => panic!("expected InsecureRedirect, got {other:?}"),
        }
        assert_eq!(plain_requests.load(Ordering::SeqCst), 0);

        // Followed when the caller allows it.
        opts.allow_insecure_redirects = true;
        let (session, url) = Session::new(&server.url, &opts, CancellationToken::new());
        let response = send(&client, &url, &session, None).await.unwrap();
        assert_eq!(response.url(), &plain);
        assert_eq!(response.bytes().await.unwrap(), "ok");
        assert_eq!(plain_requests.load(Ordering::SeqCst), 1);
    }

    /// `bytes */10` names no bytes at all: a `206` carrying it is not the
    /// start of the file, whatever offset was asked for.
    #[tokio::test]
    async fn a_partial_answer_without_a_span_is_not_the_range_asked_for() {
        let answer = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes */10\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let server = start_tls_server_answering(answer.to_vec()).await;
        let opts = DownloadOptions::default();
        let client = Clients::with_roots(&opts, server.roots.clone())
            .build()
            .unwrap();
        let (session, url) = Session::new(&server.url, &opts, CancellationToken::new());
        let probed = probe(&client, &url, &session, None).await;
        assert!(
            matches!(
                probed.as_ref().map_err(|f| &f.error),
                Err(Error::RangeNotHonored {
                    expected: 0,
                    got: None
                })
            ),
            "probe: {:?}",
            probed.map(|p| (p.ranged, p.total)).map_err(|f| f.error)
        );
        let request = RequestContext {
            url,
            ranged: true,
            total: Some(10),
            validators: Validators::default(),
            cancel: CancellationToken::new(),
            stall_timeout: opts.stall_timeout,
        };
        let fetched = fetch(&client, &request, &session, 0, 10).await;
        assert!(
            matches!(
                fetched.as_ref().map_err(|f| &f.error),
                Err(Error::RangeNotHonored {
                    expected: 0,
                    got: None
                })
            ),
            "fetch: {:?}",
            fetched.map(|(_, skip)| skip).map_err(|f| f.error)
        );
    }

    #[tokio::test]
    async fn separate_tls_configs_cannot_resume() {
        // Control: proves the test above detects the difference.
        let server = start_tls_server().await;
        let opts = DownloadOptions::default();
        for _ in 0..3 {
            let clients = Clients::with_roots(&opts, server.roots.clone());
            get(&clients.build().unwrap(), &server.url).await;
        }
        let kinds = server.handshakes.lock().unwrap().clone();
        assert_eq!(kinds, vec![HandshakeKind::Full; 3]);
    }
}
