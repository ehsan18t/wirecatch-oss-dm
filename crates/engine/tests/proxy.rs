//! Downloads through the proxies an application gives
//! ([`DownloadOptions::proxy`]): SOCKS5 for HTTP and FTP, a proxy's refused
//! login, FTP with a proxy that cannot carry it, and exceptions.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error, ManualProxies, Outcome, Progress, ProxyChoice,
    ProxySetting, Url, download,
};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::proxy::{LoginWall, SocksConfig, SocksServer};
use wirecatch_test_support::{TestResult, private_dir, test_data};

/// Manual proxies with `all` for every protocol.
fn every_protocol(proxy: &str) -> ManualProxies {
    let mut manual = ManualProxies::default();
    manual.all = Some(proxy.to_string());
    manual
}

/// Downloads `url` into a new folder through `proxy`.
async fn fetch_through(
    url: Url,
    proxy: ManualProxies,
) -> TestResult<wirecatch_engine::Result<Outcome>> {
    let dir = private_dir("proxy")?;
    let mut request = DownloadRequest::new(url, dir.path());
    request.options.proxy = ProxyChoice::Manual(proxy);
    Ok(download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await)
}

fn completed_bytes(outcome: &Outcome) -> u64 {
    match outcome {
        Outcome::Completed { bytes, .. } => *bytes,
        other => panic!("expected a finished download, got {other:?}"),
    }
}

#[tokio::test]
async fn an_http_download_goes_through_a_socks5_proxy() {
    let data = test_data(300 * 1024);
    let server = TestServer::start(data.clone(), Config::default()).await;
    for scheme in ["socks5", "socks5h"] {
        let socks = SocksServer::start(SocksConfig::default()).await;
        let outcome = fetch_through(
            server.url("/file.bin"),
            every_protocol(&socks.url(scheme, "")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(completed_bytes(&outcome), data.len() as u64, "{scheme}");
        assert!(
            !socks.destinations().is_empty(),
            "{scheme}: went through the proxy"
        );
    }
}

#[tokio::test]
async fn a_socks5_login_is_sent_and_a_wrong_one_is_a_refused_login() {
    let data = test_data(1024);
    let server = TestServer::start(data.clone(), Config::default()).await;
    let socks = SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await;
    let outcome = fetch_through(
        server.url("/f.bin"),
        every_protocol(&socks.url("socks5", "alice:secret@")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(completed_bytes(&outcome), data.len() as u64);

    for userinfo in ["", "alice:wrong@"] {
        let error = fetch_through(
            server.url("/f.bin"),
            every_protocol(&socks.url("socks5", userinfo)),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            matches!(error, Error::ProxyLoginRefused),
            "{userinfo}: {error:?}"
        );
    }
}

#[tokio::test]
async fn an_http_proxy_that_wants_a_login_is_a_refused_login_for_http_and_https() {
    let wall = LoginWall::start().await;
    // Neither request gets past the proxy, so the servers need not exist.
    for url in ["http://files.example/a.bin", "https://files.example/a.bin"] {
        let error = fetch_through(Url::parse(url).unwrap(), every_protocol(&wall.url()))
            .await
            .unwrap()
            .unwrap_err();
        assert!(
            matches!(error, Error::ProxyLoginRefused),
            "{url}: {error:?}"
        );
        assert!(!error.is_retryable());
    }
}

#[tokio::test]
async fn hosts_the_exceptions_name_are_reached_directly() {
    let data = test_data(1024);
    let server = TestServer::start(data.clone(), Config::default()).await;
    let socks = SocksServer::start(SocksConfig::default()).await;
    for exceptions in [
        "127.0.0.1",
        "127.*",
        "127.0.0.0/8",
        "files.example, 127.0.0.1",
    ] {
        let mut proxy = every_protocol(&socks.url("socks5", ""));
        proxy.exceptions = exceptions.to_string();
        fetch_through(server.url("/f.bin"), proxy)
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(socks.destinations(), Vec::<String>::new());
}

#[tokio::test]
async fn ftp_goes_through_a_socks5_proxy_control_and_data_alike() {
    let data = test_data(200 * 1024);
    let server = FtpServer::start(FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(data.clone()))],
        ..Default::default()
    })
    .await;
    let socks = SocksServer::start(SocksConfig::default()).await;
    let mut proxy = ManualProxies::default();
    proxy.ftp = Some(socks.url("socks5", ""));
    let outcome = fetch_through(server.url("ftp", "", "file.bin"), proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed_bytes(&outcome), data.len() as u64);

    let destinations = socks.destinations();
    let control = format!("127.0.0.1:{}", server.port);
    assert!(destinations.contains(&control), "{destinations:?}");
    assert!(
        destinations.iter().any(|d| *d != control),
        "the data connections went through the proxy too: {destinations:?}"
    );
}

#[tokio::test]
async fn ftp_behind_a_proxy_that_cannot_carry_it_is_refused_unless_it_may_go_directly() {
    let data = test_data(1024);
    let server = FtpServer::start(FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(data.clone()))],
        ..Default::default()
    })
    .await;
    let url = server.url("ftp", "", "file.bin");
    let proxy = every_protocol("http://proxy.example:8080");

    let error = fetch_through(url.clone(), proxy.clone())
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(
            &error,
            Error::FtpProxyUnsupported {
                setting: ProxySetting::Given,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        server.stats.sessions.load(Ordering::SeqCst),
        0,
        "never connected"
    );

    let mut direct = proxy;
    direct.ftp_direct = true;
    let outcome = fetch_through(url, direct).await.unwrap().unwrap();
    assert_eq!(completed_bytes(&outcome), data.len() as u64);
}

#[tokio::test]
async fn direct_uses_no_proxy() {
    let data = test_data(1024);
    let server = TestServer::start(data.clone(), Config::default()).await;
    let dir = private_dir("proxy-direct").unwrap();
    let mut request = DownloadRequest::new(server.url("/f.bin"), dir.path());
    request.options.proxy = ProxyChoice::Direct;
    let outcome = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(completed_bytes(&outcome), data.len() as u64);
}

#[tokio::test]
async fn a_given_proxy_that_cannot_be_read_is_refused_rather_than_gone_around() {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    // Only digits and dots, but no address: a URL cannot hold it.
    for proxy in ["http://999.1.1.1:8080", "http://proxy.10:8080"] {
        let error = fetch_through(server.url("/f.bin"), every_protocol(proxy))
            .await
            .unwrap()
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidOption("proxy")),
            "{proxy}: {error:?}"
        );
    }
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ftp_goes_by_name_through_a_socks5h_proxy_that_wants_a_login() {
    let data = test_data(100 * 1024);
    let server = FtpServer::start(FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(data.clone()))],
        ..Default::default()
    })
    .await;
    let socks = SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await;
    let mut url = server.url("ftp", "", "file.bin");
    url.set_host(Some("localhost")).unwrap();
    let mut proxy = ManualProxies::default();
    proxy.ftp = Some(socks.url("socks5h", "alice:secret@"));
    let outcome = fetch_through(url, proxy).await.unwrap().unwrap();
    assert_eq!(completed_bytes(&outcome), data.len() as u64);

    // The proxy was given the name, for the control connection and the data connections alike.
    let destinations = socks.destinations();
    assert!(
        destinations.iter().all(|d| d.starts_with("localhost:")),
        "{destinations:?}"
    );
    assert!(destinations.len() >= 2, "{destinations:?}");
}
