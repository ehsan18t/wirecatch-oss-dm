//! FTP downloads through the public `download()` API, against the scripted
//! server in `common/ftp_server.rs`. FTPS needs the test certificate trusted,
//! so those tests live inside the crate (`src/ftp/tests.rs`).

mod common;
#[path = "common/ftp_server.rs"]
mod ftp_server;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::test_data;
use ftp_server::{FtpConfig, FtpServer, Refuse};
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error, Outcome, Progress, Url, download,
};

const MIB: usize = 1024 * 1024;

fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-ftp-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn serving(data: &[u8]) -> FtpConfig {
    FtpConfig {
        files: vec![("pub/file.bin".to_string(), Arc::new(data.to_vec()))],
        mdtm: Some("20260102030405".to_string()),
        ..Default::default()
    }
}

fn request(url: &str, dir: &Path, connections: usize) -> DownloadRequest {
    let mut req = DownloadRequest::new(Url::parse(url).unwrap(), dir);
    req.options.connections = connections;
    req.options.max_retries = 3;
    req
}

async fn run(req: DownloadRequest) -> Result<Outcome, Error> {
    download(req, Arc::new(Progress::default()), CancellationToken::new()).await
}

fn completed(outcome: Outcome) -> (PathBuf, u64, u64) {
    match outcome {
        Outcome::Completed {
            path,
            bytes,
            resumed_from,
            ..
        } => (path, bytes, resumed_from),
        other => panic!("expected Completed, got {other:?}"),
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn plain_download_is_byte_identical_over_parallel_connections() {
    let data = test_data(8 * MIB + 12_345);
    let mut config = serving(&data);
    // Slow enough per connection that the other connections join in.
    config.data_bps = Some(8 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("parallel");

    let (path, bytes, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 4))
            .await
            .unwrap(),
    );

    assert_eq!(path, dir.join("file.bin"));
    assert_eq!(bytes, data.len() as u64);
    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    assert_eq!(
        files_in(&dir),
        ["file.bin"],
        "no part or resume file left behind"
    );
    let stats = &server.stats;
    assert!(
        stats.max_transfers.load(Ordering::SeqCst) >= 2,
        "transfers ran in parallel"
    );
    let offsets = stats.rest_offsets.lock().unwrap().clone();
    assert!(
        offsets.iter().filter(|&&o| o > 0).count() >= 1,
        "later ranges start with REST: {offsets:?}"
    );
    for command in [
        "TYPE I",
        "EPSV",
        "SIZE pub/file.bin",
        "MDTM pub/file.bin",
        "USER anonymous",
        "PASS anonymous@",
    ] {
        assert!(stats.count(command) >= 1, "{command} was sent");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn resumes_after_cancel_without_downloading_again() {
    let data = test_data(8 * MIB);
    let mut config = serving(&data);
    config.login = Some(("alice".into(), "s3cret word".into()));
    config.data_bps = Some(2 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("resume");
    let url = server.url("ftp", "alice:s3cret%20word@", "pub/file.bin");

    // Pause once a quarter has arrived.
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    {
        let (progress, cancel) = (progress.clone(), cancel.clone());
        tokio::spawn(async move {
            while progress.done() < 2 * MIB as u64 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            cancel.cancel();
        });
    }
    let outcome = download(request(&url, &dir, 2), progress, cancel)
        .await
        .unwrap();
    let Outcome::Paused { remaining, .. } = outcome else {
        panic!("expected Paused, got {outcome:?}")
    };
    assert!(remaining > 0 && remaining < data.len() as u64);
    assert_eq!(files_in(&dir), ["file.bin.wcpart", "file.bin.wcstate"]);
    let state = std::fs::read_to_string(dir.join("file.bin.wcstate")).unwrap();
    assert!(
        !state.contains("s3cret"),
        "the resume file holds no password: {state}"
    );
    assert!(
        state.contains("20260102030405"),
        "MDTM is the validator: {state}"
    );
    let sent_before = server.stats.bytes_sent.load(Ordering::SeqCst);

    server.configure(|c| c.data_bps = None);
    let (path, bytes, resumed_from) = completed(run(request(&url, &dir, 2)).await.unwrap());
    assert_eq!(bytes, data.len() as u64);
    assert!(resumed_from >= 2 * MIB as u64, "kept {resumed_from} bytes");
    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    let sent_after = server.stats.bytes_sent.load(Ordering::SeqCst) - sent_before;
    assert!(
        sent_after < data.len() as u64,
        "the second run fetched only what was missing ({sent_after} bytes)"
    );
    assert!(
        server
            .stats
            .rest_offsets
            .lock()
            .unwrap()
            .iter()
            .any(|&o| o >= 2 * MIB as u64)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_changed_file_starts_over_instead_of_resuming() {
    let data = test_data(4 * MIB);
    let mut config = serving(&data);
    config.data_bps = Some(MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("changed");
    let url = server.url("ftp", "", "pub/file.bin");

    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    {
        let (progress, cancel) = (progress.clone(), cancel.clone());
        tokio::spawn(async move {
            while progress.done() < MIB as u64 / 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            cancel.cancel();
        });
    }
    let outcome = download(request(&url, &dir, 2), progress, cancel)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Paused { .. }), "{outcome:?}");

    // Same size, new modification time: the old bytes are useless.
    let new_data: Vec<u8> = data.iter().map(|b| b.wrapping_add(1)).collect();
    server.configure(|c| {
        c.files = vec![("pub/file.bin".to_string(), Arc::new(new_data.clone()))];
        c.mdtm = Some("20260202030405".to_string());
        c.data_bps = None;
    });
    let (path, _, resumed_from) = completed(run(request(&url, &dir, 2)).await.unwrap());
    assert_eq!(resumed_from, 0);
    assert!(
        std::fs::read(&path).unwrap() == new_data,
        "the new version, not a mix"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

async fn completes_within_session_limit(refuse: Refuse, test: &str) {
    let data = test_data(8 * MIB);
    let mut config = serving(&data);
    config.max_sessions = Some((2, refuse));
    config.data_bps = Some(8 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir(test);

    let (path, _, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 6))
            .await
            .unwrap(),
    );

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    let stats = &server.stats;
    assert!(
        stats.refused.load(Ordering::SeqCst) >= 1,
        "the limit was hit"
    );
    assert!(stats.max_logged_in.load(Ordering::SeqCst) <= 2);
    assert_eq!(
        stats.max_transfers.load(Ordering::SeqCst),
        2,
        "both allowed sessions were used"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn completes_when_extra_connections_get_421() {
    completes_within_session_limit(Refuse::Greeting421, "limit-421").await;
}

#[tokio::test]
async fn completes_when_extra_logins_get_530() {
    completes_within_session_limit(Refuse::Login530, "limit-530").await;
}

#[tokio::test]
async fn a_wrong_password_is_a_plain_language_error() {
    let data = test_data(1000);
    let mut config = serving(&data);
    config.login = Some(("alice".into(), "right".into()));
    let server = FtpServer::start(config).await;
    let dir = temp_dir("password");

    let error = run(request(
        &server.url("ftp", "alice:wrong-pass@", "pub/file.bin"),
        &dir,
        4,
    ))
    .await
    .unwrap_err();

    let Error::FtpLoginRefused { host, code, .. } = &error else {
        panic!("expected FtpLoginRefused, got {error:?}");
    };
    assert_eq!((host.as_str(), *code), ("127.0.0.1", 530));
    assert!(!error.is_retryable());
    let text = error.to_string();
    assert!(
        text.contains("did not accept the login") && text.contains("Login incorrect"),
        "{text}"
    );
    assert!(
        !text.contains("wrong-pass"),
        "the password is not in the message: {text}"
    );
    assert_eq!(
        server.stats.count("PASS"),
        1,
        "a refused login is not retried"
    );
    assert!(files_in(&dir).is_empty(), "nothing written");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_missing_file_is_reported_and_nothing_is_written() {
    let server = FtpServer::start(serving(b"x")).await;
    let dir = temp_dir("missing");

    let error = run(request(
        &server.url("ftp", "bob:pw@", "pub/nothing.bin"),
        &dir,
        4,
    ))
    .await
    .unwrap_err();

    let Error::FtpReply { code, url, .. } = &error else {
        panic!("expected FtpReply, got {error:?}");
    };
    assert_eq!(*code, 550);
    assert!(!url.contains("pw"), "no password in {url}");
    assert!(!error.is_retryable());
    assert!(error.to_string().contains("no such file"), "{error}");
    assert!(files_in(&dir).is_empty(), "nothing written");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn pasv_is_used_without_epsv_and_its_address_is_ignored() {
    let data = test_data(4 * MIB);
    let mut config = serving(&data);
    config.epsv = false;
    // A server naming another machine must not send the client there.
    config.pasv_ip = [192, 0, 2, 1];
    let server = FtpServer::start(config).await;
    let dir = temp_dir("pasv");

    let (path, _, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 3))
            .await
            .unwrap(),
    );

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    let stats = &server.stats;
    assert!(stats.count("PASV") >= 2);
    assert_eq!(stats.count("EPSV"), 1, "a refused EPSV is not asked again");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn an_unreadable_epsv_answer_falls_back_to_pasv_once() {
    let data = test_data(4 * MIB);
    let mut config = serving(&data);
    config.epsv_garbage = true;
    let server = FtpServer::start(config).await;
    let dir = temp_dir("epsv-garbage");

    let (path, _, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 3))
            .await
            .unwrap(),
    );

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    assert_eq!(server.stats.count("EPSV"), 1, "not asked again");
    assert!(server.stats.count("PASV") >= 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_server_without_rest_is_downloaded_over_one_connection() {
    let data = test_data(4 * MIB);
    let mut config = serving(&data);
    config.rest = false;
    let server = FtpServer::start(config).await;
    let dir = temp_dir("no-rest");

    let (path, _, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 8))
            .await
            .unwrap(),
    );

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    assert_eq!(server.stats.transfers.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn the_size_from_the_transfer_reply_is_used_without_size_support() {
    let data = test_data(4 * MIB);
    let mut config = serving(&data);
    config.size = false;
    config.data_bps = Some(8 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("no-size");

    let progress = Arc::new(Progress::default());
    let outcome = download(
        request(&server.url("ftp", "", "pub/file.bin"), &dir, 4),
        progress.clone(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let (path, _, _) = completed(outcome);

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    assert_eq!(
        progress.total(),
        Some(data.len() as u64),
        "size from the 150 reply"
    );
    assert!(
        server.stats.max_transfers.load(Ordering::SeqCst) >= 2,
        "still split"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn dropped_data_connections_are_retried() {
    let data = test_data(3 * MIB);
    let mut config = serving(&data);
    config.drop_first = Some((2, 100_000));
    let server = FtpServer::start(config).await;
    let dir = temp_dir("dropped");

    let (path, _, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 2))
            .await
            .unwrap(),
    );

    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    assert!(server.stats.transfers.load(Ordering::SeqCst) >= 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn an_empty_file_completes() {
    let server = FtpServer::start(serving(b"")).await;
    let dir = temp_dir("empty");

    let (path, bytes, _) = completed(
        run(request(&server.url("ftp", "", "pub/file.bin"), &dir, 4))
            .await
            .unwrap(),
    );

    assert_eq!(bytes, 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn ftpes_never_falls_back_to_an_unencrypted_login() {
    // This server offers no TLS at all.
    let server = FtpServer::start(serving(b"secret file")).await;
    let dir = temp_dir("no-tls");

    let error = run(request(
        &server.url("ftpes", "alice:pw@", "pub/file.bin"),
        &dir,
        2,
    ))
    .await
    .unwrap_err();

    assert!(
        matches!(error, Error::FtpTlsUnavailable { .. }),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("ftp://"),
        "says how to go on: {error}"
    );
    assert_eq!(
        server.stats.count("USER"),
        0,
        "no credentials sent in the clear"
    );
    assert_eq!(server.stats.count("PASS"), 0);
    assert!(files_in(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_folder_link_is_refused_before_connecting() {
    let server = FtpServer::start(serving(b"x")).await;
    let dir = temp_dir("folder");
    let error = run(request(&server.url("ftp", "", "pub/"), &dir, 1))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidUrl(_)), "{error:?}");
    assert!(error.to_string().contains("folder"), "{error}");
    assert_eq!(server.stats.sessions.load(Ordering::SeqCst), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Cancels `cancel` once `ready` holds, and records when.
fn cancel_when(
    cancel: &CancellationToken,
    ready: impl Fn() -> bool + Send + 'static,
) -> Arc<std::sync::Mutex<Option<std::time::Instant>>> {
    let at = Arc::new(std::sync::Mutex::new(None));
    let (cancel, recorded) = (cancel.clone(), at.clone());
    tokio::spawn(async move {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        *recorded.lock().unwrap() = Some(std::time::Instant::now());
        cancel.cancel();
    });
    at
}

fn since(at: &std::sync::Mutex<Option<std::time::Instant>>) -> Duration {
    at.lock().unwrap().expect("cancelled").elapsed()
}

#[tokio::test]
async fn pausing_while_the_server_holds_back_its_426_returns_at_once() {
    let data = test_data(8 * MIB);
    let mut config = serving(&data);
    config.data_bps = Some(2 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("held-426");
    let url = server.url("ftp", "", "pub/file.bin");

    // A first run pauses with most of the file still to come.
    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let seen = progress.clone();
    cancel_when(&cancel, move || seen.done() >= MIB as u64);
    let outcome = download(request(&url, &dir, 2), progress, cancel)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Paused { .. }), "{outcome:?}");

    // On resuming, the first transfer (from byte 0) is not needed and is cut
    // short. The server now takes 30 s to answer that; pause meanwhile.
    server.configure(|c| c.hold_abort_reply = Some(Duration::from_secs(30)));
    let held = server.stats.held_replies.load(Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let stats = server.stats.clone();
    let paused_at = cancel_when(&cancel, move || {
        stats.held_replies.load(Ordering::SeqCst) > held
    });
    let outcome = download(
        request(&url, &dir, 2),
        Arc::new(Progress::default()),
        cancel,
    )
    .await
    .unwrap();

    let took = since(&paused_at);
    assert!(matches!(outcome, Outcome::Paused { .. }), "{outcome:?}");
    assert!(took < Duration::from_secs(2), "paused after {took:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_control_connection_is_reused_after_a_range_is_cut_short() {
    let data = test_data(8 * MIB);
    let mut config = serving(&data);
    config.data_bps = Some(4 * MIB as u64);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("reuse");
    let url = server.url("ftp", "", "pub/file.bin");

    // Pause early, then resume: the resumed run's first transfer (from byte
    // 0) is cut short at once, and its control connection carries on.
    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let seen = progress.clone();
    cancel_when(&cancel, move || seen.done() >= MIB as u64);
    let outcome = download(request(&url, &dir, 2), progress, cancel)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Paused { .. }), "{outcome:?}");
    let logins_before = server.stats.count("USER");
    let transfers_before = server.stats.transfers.load(Ordering::SeqCst);

    server.configure(|c| c.data_bps = None);
    let (path, _, resumed_from) = completed(run(request(&url, &dir, 2)).await.unwrap());

    assert!(resumed_from > 0);
    assert!(
        std::fs::read(&path).unwrap() == data,
        "downloaded bytes differ"
    );
    let stats = &server.stats;
    assert!(
        stats.reused_after_abort.load(Ordering::SeqCst) >= 1,
        "a transfer followed a cut-short one on the same control connection: {:?}",
        stats.commands()
    );
    let logins = stats.count("USER") - logins_before;
    let transfers = stats.transfers.load(Ordering::SeqCst) - transfers_before;
    assert!(
        logins < transfers,
        "{logins} logins for {transfers} transfers"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn pausing_while_a_connection_waits_for_a_greeting_returns_at_once() {
    let data = test_data(8 * MIB);
    let mut config = serving(&data);
    config.data_bps = Some(2 * MIB as u64);
    // Only the first session is greeted; the others wait forever.
    config.silent_after = Some(1);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("silent");

    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let seen = progress.clone();
    let paused_at = cancel_when(&cancel, move || seen.done() >= MIB as u64);
    let outcome = download(
        request(&server.url("ftp", "", "pub/file.bin"), &dir, 4),
        progress,
        cancel,
    )
    .await
    .unwrap();

    let took = since(&paused_at);
    assert!(matches!(outcome, Outcome::Paused { .. }), "{outcome:?}");
    assert!(took < Duration::from_secs(2), "paused after {took:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn cancelling_before_the_server_greets_returns_at_once() {
    let mut config = serving(b"x");
    config.silent_after = Some(0);
    let server = FtpServer::start(config).await;
    let dir = temp_dir("silent-first");

    let cancel = CancellationToken::new();
    let stats = server.stats.clone();
    let cancelled_at = cancel_when(&cancel, move || stats.sessions.load(Ordering::SeqCst) >= 1);
    let error = download(
        request(&server.url("ftp", "", "pub/file.bin"), &dir, 4),
        Arc::new(Progress::default()),
        cancel,
    )
    .await
    .unwrap_err();

    let took = since(&cancelled_at);
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(took < Duration::from_secs(2), "returned after {took:?}");
    assert!(files_in(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn an_unsupported_scheme_is_reported_without_the_password() {
    let dir = temp_dir("scheme");
    let error = run(request("gopher://bob:hunter2@files.example/a.bin", &dir, 1))
        .await
        .unwrap_err();

    let Error::InvalidUrl(text) = &error else {
        panic!("expected InvalidUrl, got {error:?}");
    };
    assert!(text.contains("gopher://bob@files.example/a.bin"), "{text}");
    assert!(
        !text.contains("hunter2"),
        "the password is not in the message: {text}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
