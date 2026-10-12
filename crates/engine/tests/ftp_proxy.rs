//! FTP with a proxy set in the environment is refused rather than going
//! around the proxy, unless `NO_PROXY` names the host. A test binary of its own, with a single test, because it
//! changes the process environment.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use wirecatch_engine::{CancellationToken, DownloadRequest, Error, Progress, download};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer};
use wirecatch_test_support::private_dir;

#[tokio::test(flavor = "current_thread")]
async fn ftp_is_refused_while_a_proxy_is_set() {
    let server = FtpServer::start(FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(b"data".to_vec()))],
        ..Default::default()
    })
    .await;
    let dir = private_dir("ftp-proxy").unwrap();
    let url = server.url("ftp", "", "file.bin");

    // SAFETY: as below. The machine running the test may exclude local hosts
    // from its proxy; this test decides that itself.
    unsafe {
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }
    for variable in ["ALL_PROXY", "all_proxy", "FTP_PROXY", "ftp_proxy"] {
        // SAFETY: this test binary runs this one test, on one thread, so
        // nothing else reads the environment meanwhile.
        unsafe { std::env::set_var(variable, "http://proxy.example:8080") };
        let error = download(
            DownloadRequest::new(url.clone(), dir.path()),
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        // SAFETY: as above.
        unsafe { std::env::remove_var(variable) };

        let Error::FtpProxyUnsupported {
            variable: named, ..
        } = &error
        else {
            panic!("{variable}: expected FtpProxyUnsupported, got {error:?}");
        };
        // Windows names environment variables without regard to case.
        assert!(
            named.eq_ignore_ascii_case(variable),
            "{named} for {variable}"
        );
        let text = error.to_string();
        assert!(text.contains("cannot go through a proxy"), "{text}");
        assert!(
            text.contains(&format!("remove {named}")),
            "says how to go on: {text}"
        );
    }
    assert_eq!(
        server.stats.sessions.load(Ordering::SeqCst),
        0,
        "never connected"
    );

    // Without a proxy the same link downloads.
    download(
        DownloadRequest::new(url.clone(), dir.path()),
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    // So it does with a proxy that NO_PROXY says this host does not use.
    let host = url.host_str().unwrap().to_string();
    // SAFETY: as above.
    unsafe {
        std::env::set_var("ALL_PROXY", "http://proxy.example:8080");
        std::env::set_var("NO_PROXY", format!("other.example,{host}"));
    }
    let direct = download(
        DownloadRequest::new(url, dir.path()),
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await;
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("ALL_PROXY");
        std::env::remove_var("NO_PROXY");
    }
    direct.unwrap();
    dir.remove().unwrap();
}
