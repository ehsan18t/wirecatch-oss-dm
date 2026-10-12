//! FTP credentials never reach the logs. A test binary of its own: it installs
//! a process-wide log subscriber, which would also catch other tests' logs.

use std::sync::{Arc, Mutex};

use wirecatch_engine::units::MIB_USIZE;
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, download};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer};
use wirecatch_test_support::{private_dir, test_data};

/// Collects everything the engine logs, at every level.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("the log buffer's lock is poisoned"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn credentials_are_never_logged() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer({
            let captured = captured.clone();
            move || captured.clone()
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let data = test_data(2 * MIB_USIZE);
    let server = FtpServer::start(FtpConfig {
        files: vec![("pub/file.bin".to_string(), Arc::new(data.clone()))],
        login: Some(("alice".into(), "hunter2-secret".into())),
        ..Default::default()
    })
    .await;
    let dir = private_dir("ftp-logging").unwrap();
    let url = server.url("ftp", "alice:hunter2-secret@", "pub/file.bin");
    let mut request = DownloadRequest::new(url, dir.path());
    request.options.connections = 3;

    let outcome = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("expected Completed, got {outcome:?}");
    };
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(
        server.stats.count("PASS hunter2-secret") >= 1,
        "the server got the password"
    );
    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("PASS <hidden>"),
        "commands are logged: {logs}"
    );
    assert!(!logs.contains("hunter2"), "the password is never logged");
}
