//! FTP links: accepted from the app and from captures, and the engine's FTP
//! errors shown as plain messages with a fix. The test-support crate's scripted
//! FTP server plays each failure.

use wirecatch_core::CoreError;
use wirecatch_protocol::app::{DownloadStatus, FixAction, UserError};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer};

use crate::common::{Harness, TestResult};

/// An FTP server with no files, which accepts only `bob` with password `pw`
/// and offers no encryption.
async fn ftp_server() -> FtpServer {
    FtpServer::start(FtpConfig {
        login: Some(("bob".into(), "pw".into())),
        ..FtpConfig::default()
    })
    .await
}

async fn failure(h: &Harness, url: impl AsRef<str>) -> TestResult<UserError> {
    let row = h.add(url).await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    Ok(failed.error.ok_or("the failed download has no error")?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_login_asks_for_the_right_name_and_password() -> TestResult {
    let server = ftp_server().await;
    let h = Harness::new().await?;
    let error = failure(&h, server.url("ftp", "alice:secret-pw@", "report.pdf")).await?;
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(
        error
            .message
            .contains("did not accept the user name or password"),
        "{}",
        error.message
    );
    assert!(error.message.contains("530 Login incorrect"));
    assert!(!error.message.contains("secret-pw"));
    assert!(server.stats.commands().iter().any(|c| c == "USER alice"));

    let error = failure(&h, server.url("ftp", "", "public.zip")).await?;
    assert!(
        error.message.contains("does not allow anonymous downloads"),
        "{}",
        error.message
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_file_says_so() -> TestResult {
    let server = ftp_server().await;
    let h = Harness::new().await?;
    let error = failure(&h, server.url("ftp", "bob:pw@", "gone.iso")).await?;
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(
        error.message.contains("does not exist"),
        "{}",
        error.message
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secure_link_to_a_server_without_encryption_stops() -> TestResult {
    let server = ftp_server().await;
    let h = Harness::new().await?;
    let error = failure(&h, server.url("ftpes", "bob:pw@", "file.bin")).await?;
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(
        error.message.contains("does not offer encryption"),
        "{}",
        error.message
    );
    assert!(error.message.contains("ftp://"));
    // Nothing, not even the user name, was sent unencrypted.
    assert_eq!(server.stats.count("USER"), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ftp_capture_that_fails_leaves_the_download_to_the_browser() -> TestResult {
    let server = ftp_server().await;
    let h = Harness::new().await?;
    let capture = NewDownload {
        handover_id: "ftp-1".into(),
        url: server.url("ftp", "bob:pw@", "gone.iso").into(),
        referrer: None,
        page_url: None,
        suggested_name: Some("gone.iso".into()),
        mime: None,
        size_hint: None,
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::ContextMenu,
        queue: None,
        media: None,
    };
    let error = h.manager.add_capture(capture).await.unwrap_err();
    let CoreError::NotStarted(reason) = &error else {
        panic!("unexpected {error:?}");
    };
    assert!(reason.contains("does not exist"), "{reason}");
    Ok(())
}
