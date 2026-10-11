//! FTP with a proxy set in the environment is refused rather than going
//! around the proxy. A test binary of its own, with a single test, because it
//! changes the process environment.

mod common;
#[path = "common/ftp_server.rs"]
mod ftp_server;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ftp_server::{FtpConfig, FtpServer};
use wirecatch_engine::{CancellationToken, DownloadRequest, Error, Progress, Url, download};

#[tokio::test(flavor = "current_thread")]
async fn ftp_is_refused_while_a_proxy_is_set() {
    let server = FtpServer::start(FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(b"data".to_vec()))],
        ..Default::default()
    })
    .await;
    let dir = std::env::temp_dir().join(format!("wirecatch-ftp-proxy-{}", std::process::id()));
    let url = Url::parse(&server.url("ftp", "", "file.bin")).unwrap();

    for variable in ["ALL_PROXY", "all_proxy", "FTP_PROXY", "ftp_proxy"] {
        // SAFETY: this test binary runs this one test, on one thread, so
        // nothing else reads the environment meanwhile.
        unsafe { std::env::set_var(variable, "http://proxy.example:8080") };
        let error = download(
            DownloadRequest::new(url.clone(), &dir),
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
        DownloadRequest::new(url, &dir),
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
