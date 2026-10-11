//! FTP links: accepted from the app and from captures, and the engine's FTP
//! errors shown as plain messages with a fix. A scripted local FTP server plays
//! each failure.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use wirecatch_core::CoreError;
use wirecatch_protocol::app::{DownloadStatus, FixAction, UserError};
use wirecatch_protocol::extension::{CaptureSource, NewDownload};

use crate::common::Harness;

/// The replies that differ between scenarios.
#[derive(Clone, Copy)]
struct Script {
    auth: &'static str,
    pass: &'static str,
    retr: &'static str,
}

const GOOD_LOGIN: Script = Script {
    auth: "502 Not implemented.",
    pass: "230 Logged in.",
    retr: "550 No such file or directory.",
};

/// Starts a scripted FTP server; returns its `host:port` and the commands it got.
async fn ftp_server(script: Script) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let commands = Arc::new(Mutex::new(Vec::new()));
    let log = commands.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                let _ = write.write_all(b"220 Scripted server ready.\r\n").await;
                let mut data = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    let verb = line.split(' ').next().unwrap_or("").to_ascii_uppercase();
                    log.lock().unwrap().push(line.clone());
                    let reply = match verb.as_str() {
                        "AUTH" => script.auth.to_string(),
                        "USER" => "331 Password please.".into(),
                        "PASS" => script.pass.into(),
                        "PBSZ" | "PROT" | "TYPE" | "OPTS" => "200 OK.".into(),
                        "SIZE" | "MDTM" => "550 Not available.".into(),
                        "EPSV" => {
                            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                            let port = listener.local_addr().unwrap().port();
                            data = Some(listener);
                            format!("229 Entering Extended Passive Mode (|||{port}|)")
                        }
                        "REST" => "350 Restarting.".into(),
                        "RETR" => {
                            if let Some(listener) = data.take() {
                                // Accept the data connection, then refuse the file.
                                tokio::spawn(async move {
                                    let _ = listener.accept().await;
                                });
                            }
                            script.retr.into()
                        }
                        "QUIT" => {
                            let _ = write.write_all(b"221 Bye.\r\n").await;
                            return;
                        }
                        _ => "502 Not implemented.".into(),
                    };
                    if write
                        .write_all(format!("{reply}\r\n").as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    (addr.to_string(), commands)
}

async fn failure(h: &Harness, url: &str) -> UserError {
    let row = h.add(url).await;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    failed.error.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_login_asks_for_the_right_name_and_password() {
    let (host, commands) = ftp_server(Script {
        pass: "530 Login incorrect.",
        ..GOOD_LOGIN
    })
    .await;
    let h = Harness::new().await;
    let error = failure(&h, &format!("ftp://alice:secret-pw@{host}/report.pdf")).await;
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
    assert!(commands.lock().unwrap().iter().any(|c| c == "USER alice"));

    let error = failure(&h, &format!("ftp://{host}/public.zip")).await;
    assert!(
        error.message.contains("does not allow anonymous downloads"),
        "{}",
        error.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_file_says_so() {
    let (host, _) = ftp_server(GOOD_LOGIN).await;
    let h = Harness::new().await;
    let error = failure(&h, &format!("ftp://bob:pw@{host}/gone.iso")).await;
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(
        error.message.contains("does not exist"),
        "{}",
        error.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secure_link_to_a_server_without_encryption_stops() {
    let (host, commands) = ftp_server(GOOD_LOGIN).await;
    let h = Harness::new().await;
    let error = failure(&h, &format!("ftpes://bob:pw@{host}/file.bin")).await;
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(
        error.message.contains("does not offer encryption"),
        "{}",
        error.message
    );
    assert!(error.message.contains("ftp://"));
    // Nothing, not even the user name, was sent unencrypted.
    assert!(
        !commands
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("USER"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ftp_capture_that_fails_leaves_the_download_to_the_browser() {
    let (host, _) = ftp_server(GOOD_LOGIN).await;
    let h = Harness::new().await;
    let capture = NewDownload {
        handover_id: "ftp-1".into(),
        url: format!("ftp://bob:pw@{host}/gone.iso"),
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
}
