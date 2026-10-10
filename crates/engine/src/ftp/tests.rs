//! URL rules, and FTPS against the scripted server with its test certificate
//! trusted (plain FTP is tested through the public API in `tests/ftp.rs`).

#[path = "../../tests/common/ftp_server.rs"]
mod ftp_server;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rustls::HandshakeKind;

use super::*;
use crate::download::{DownloadRequest, Outcome, download_with};
use crate::http::Clients;
use crate::progress::Progress;
use ftp_server::{FtpConfig, FtpServer, test_tls};

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn target(s: &str) -> Result<Target, Error> {
    let roots = rustls::RootCertStore::empty();
    let clients = Clients::with_roots(&DownloadOptions::default(), roots);
    Target::new(&url(s), &DownloadOptions::default(), clients.tls())
}

#[test]
fn the_scheme_and_port_pick_the_encryption() {
    let pick = |s: &str| security_and_port(&url(s)).unwrap();
    assert_eq!(pick("ftp://h/f"), (Security::Plain, 21));
    assert_eq!(pick("ftp://h:2121/f"), (Security::Plain, 2121));
    assert_eq!(pick("ftps://h/f"), (Security::Implicit, 990));
    assert_eq!(pick("ftps://h:990/f"), (Security::Implicit, 990));
    assert_eq!(pick("ftps://h:21/f"), (Security::Explicit, 21));
    assert_eq!(pick("ftps://h:2990/f"), (Security::Implicit, 2990));
    assert_eq!(pick("ftpes://h/f"), (Security::Explicit, 21));
    assert_eq!(pick("ftpes://h:2121/f"), (Security::Explicit, 2121));
    assert!(security_and_port(&url("http://h/f")).is_none());
}

#[test]
fn paths_and_credentials_come_from_the_url() {
    let t = target("ftp://files.example/pub/a%20b.iso").unwrap();
    assert_eq!(t.path, "pub/a b.iso", "relative to the login folder");
    assert_eq!((t.user.as_str(), t.password.as_str()), ANONYMOUS);
    assert!(t.tls.is_none());

    let t = target("ftp://files.example/%2Fsrv/a.iso;type=i").unwrap();
    assert_eq!(
        t.path, "/srv/a.iso",
        "%2F makes it absolute; ;type= is dropped"
    );

    let t = target("ftps://us%40er:p%3Ass@files.example/a.iso").unwrap();
    assert_eq!((t.user.as_str(), t.password.as_str()), ("us@er", "p:ss"));
    assert_eq!(t.display_url.as_str(), "ftps://us%40er@files.example/a.iso");
    assert!(t.tls.is_some());

    let t = target("ftp://bob@files.example/a.iso").unwrap();
    assert_eq!((t.user.as_str(), t.password.as_str()), ("bob", ""));
}

#[test]
fn urls_that_cannot_be_sent_safely_are_refused() {
    for bad in [
        "ftp://files.example/",
        "ftp://files.example/pub/",
        "ftp://files.example/a%0D%0ADELE%20b",
        "ftp://us%0Aer:pw@files.example/a",
        "ftp://u:p%0D@files.example/a",
        "ftp://files.example/%FF.bin",
    ] {
        match target(bad) {
            Err(Error::InvalidUrl(text)) => assert!(!text.contains("pw"), "{text}"),
            Err(other) => panic!("{bad}: expected InvalidUrl, got {other:?}"),
            Ok(_) => panic!("{bad} was accepted"),
        }
    }
}

const MIB: usize = 1024 * 1024;

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
}

async fn secure_download(
    server: &FtpServer,
    scheme: &str,
    roots: rustls::RootCertStore,
) -> Result<Outcome, Error> {
    let dir = std::env::temp_dir().join(format!(
        "wirecatch-ftps-{scheme}-{}-{}",
        std::process::id(),
        server.port
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut request = DownloadRequest::new(url(&server.url(scheme, "alice:pw@", "file.bin")), &dir);
    request.options.connections = 4;
    request.options.max_retries = 0;
    let clients = Clients::with_roots(&request.options, roots);
    download_with(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
        clients,
    )
    .await
}

fn served(file: &[u8]) -> FtpConfig {
    FtpConfig {
        files: vec![("file.bin".to_string(), Arc::new(file.to_vec()))],
        login: Some(("alice".into(), "pw".into())),
        data_bps: Some(8 * MIB as u64),
        ..Default::default()
    }
}

fn check_completed(outcome: Outcome, file: &[u8]) {
    let Outcome::Completed { path, .. } = outcome else {
        panic!("expected Completed, got {outcome:?}");
    };
    assert!(
        std::fs::read(&path).unwrap() == file,
        "downloaded bytes differ"
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn explicit_tls_download_is_byte_identical() {
    let file = data(6 * MIB + 99);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    let server = FtpServer::start(config).await;

    check_completed(
        secure_download(&server, "ftpes", roots).await.unwrap(),
        &file,
    );

    let stats = &server.stats;
    let commands = stats.commands();
    assert_eq!(
        commands[0], "AUTH TLS",
        "encryption before anything else: {commands:?}"
    );
    assert!(stats.count("PBSZ 0") >= 1 && stats.count("PROT P") >= 1);
    assert!(
        stats.max_transfers.load(Ordering::SeqCst) >= 2,
        "parallel over TLS"
    );
    let handshakes = stats.data_handshakes.lock().unwrap().clone();
    assert!(!handshakes.is_empty(), "data connections were encrypted");
    assert!(
        handshakes
            .iter()
            .all(|&kind| kind == HandshakeKind::Resumed),
        "data connections resume the control session, as many servers require: {handshakes:?}"
    );
}

#[tokio::test]
async fn implicit_tls_download_is_byte_identical() {
    let file = data(6 * MIB);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    config.implicit = true;
    let server = FtpServer::start(config).await;

    // `ftps://` on a port other than 21 is implicit TLS.
    check_completed(
        secure_download(&server, "ftps", roots).await.unwrap(),
        &file,
    );

    let stats = &server.stats;
    assert_eq!(
        stats.count("AUTH"),
        0,
        "TLS from the first byte, no AUTH TLS"
    );
    assert!(stats.count("PROT P") >= 1);
    assert!(
        stats.max_transfers.load(Ordering::SeqCst) >= 2,
        "parallel over TLS"
    );
    assert!(!stats.data_handshakes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_untrusted_certificate_is_refused() {
    let file = data(1000);
    let (tls, _) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    let server = FtpServer::start(config).await;

    // Nothing trusted: the server's self-signed certificate must not pass.
    let error = secure_download(&server, "ftpes", rustls::RootCertStore::empty())
        .await
        .unwrap_err();

    let Error::Network(network) = &error else {
        panic!("expected a network error, got {error:?}");
    };
    assert!(network.is_connect());
    assert!(error.to_string().contains("certificate"), "{error}");
    assert_eq!(server.stats.count("USER"), 0, "no credentials sent");
}
