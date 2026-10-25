//! URL rules, and FTPS against the scripted server with its test certificate
//! trusted (plain FTP is tested through the public API in `tests/ftp.rs`).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rustls::HandshakeKind;

use super::*;
use crate::download::{DownloadRequest, Outcome, download_with};
use crate::http::Clients;
use crate::progress::Progress;
use crate::units::{MIB, MIB_USIZE};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer, Quirk, test_tls};
use wirecatch_test_support::private_dir;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn target(s: &str) -> Result<Target, Error> {
    let roots = rustls::RootCertStore::empty();
    let clients = Clients::with_roots(&DownloadOptions::default(), roots);
    Target::new(&url(s), &DownloadOptions::default(), clients.tls(), None)
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
fn international_server_names_are_sent_in_their_ascii_form() {
    for scheme in ["ftp", "ftps", "ftpes"] {
        let t = target(&format!("{scheme}://b\u{fc}cher.example/a.iso")).unwrap();
        assert_eq!(
            t.host,
            Host::Domain("xn--bcher-kva.example".to_string()),
            "{scheme}"
        );
        if let Some(tls) = &t.tls {
            assert_eq!(
                tls.server_name,
                rustls::pki_types::ServerName::try_from("xn--bcher-kva.example").unwrap()
            );
        }
    }
    // An address written in a secure link is an address, not a name.
    let t = target("ftps://127.0.0.1/a.iso").unwrap();
    assert_eq!(t.host, Host::<String>::Ipv4([127, 0, 0, 1].into()));
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

fn data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i * 7 + i / 251).to_le_bytes()[0])
        .collect()
}

/// Downloads `file.bin` from `server` into `dir` over `scheme`, trusting `roots`.
async fn secure_download(
    server: &FtpServer,
    scheme: &str,
    roots: rustls::RootCertStore,
    dir: &std::path::Path,
) -> Result<Outcome, Error> {
    let mut request = DownloadRequest::new(server.url(scheme, "alice:pw@", "file.bin"), dir);
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
        data_bps: Some(8 * MIB),
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
}

#[tokio::test]
async fn explicit_tls_download_is_byte_identical() {
    let file = data(6 * MIB_USIZE + 99);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    let server = FtpServer::start(config).await;

    let dir = private_dir("ftps").unwrap();
    check_completed(
        secure_download(&server, "ftpes", roots, dir.path())
            .await
            .unwrap(),
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
        "every data connection resumes its own control connection's session (the server seals each session's tickets with its own key), as vsftpd and ProFTPD require: {handshakes:?}"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn implicit_tls_download_is_byte_identical() {
    let file = data(6 * MIB_USIZE);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    config.implicit = true;
    let server = FtpServer::start(config).await;

    // `ftps://` on a port other than 21 is implicit TLS.
    let dir = private_dir("ftps").unwrap();
    check_completed(
        secure_download(&server, "ftps", roots, dir.path())
            .await
            .unwrap(),
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
    let handshakes = stats.data_handshakes.lock().unwrap().clone();
    assert_ne!(handshakes, [] as [rustls::HandshakeKind; 0]);
    assert!(
        handshakes
            .iter()
            .all(|&kind| kind == HandshakeKind::Resumed),
        "every data connection resumes its own control session: {handshakes:?}"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn an_untrusted_certificate_is_refused() {
    let file = data(1000);
    let (tls, _) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    let server = FtpServer::start(config).await;

    // Nothing trusted: the server's self-signed certificate must not pass.
    let dir = private_dir("ftps").unwrap();
    let error = secure_download(&server, "ftpes", rustls::RootCertStore::empty(), dir.path())
        .await
        .unwrap_err();

    let Error::Network(network) = &error else {
        panic!("expected a network error, got {error:?}");
    };
    assert!(network.is_connect());
    assert!(error.to_string().contains("certificate"), "{error}");
    assert_eq!(server.stats.count("USER"), 0, "no credentials sent");
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_reply_sent_ahead_of_encryption_is_refused() {
    let file = data(1000);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    // A forged "230" right behind "234", in the clear: it must not be read as
    // if it had come over TLS.
    config.quirks.push(Quirk::InjectAfterAuth);
    let server = FtpServer::start(config).await;

    let dir = private_dir("ftps").unwrap();
    let error = secure_download(&server, "ftpes", roots, dir.path())
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Network(_)), "{error:?}");
    assert!(
        error
            .to_string()
            .contains("unexpected data before encryption"),
        "{error}"
    );
    assert_eq!(server.stats.count("USER"), 0, "no credentials sent");
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_server_refusing_to_encrypt_data_is_not_used() {
    let file = data(1000);
    let (tls, roots) = test_tls();
    let mut config = served(&file);
    config.tls = Some(tls);
    config.quirks.push(Quirk::RefuseProtP);
    let server = FtpServer::start(config).await;

    let dir = private_dir("ftps").unwrap();
    let error = secure_download(&server, "ftpes", roots, dir.path())
        .await
        .unwrap_err();

    assert!(
        matches!(error, Error::FtpTlsUnavailable { .. }),
        "{error:?}"
    );
    assert_eq!(server.stats.count("PROT P"), 1);
    assert_eq!(server.stats.count("RETR"), 0, "nothing sent in the clear");
    assert_eq!(server.stats.count("EPSV"), 0);
    dir.remove().unwrap();
}

/// A new connection finds the file with another modification time than the
/// first connection saw, as when it was replaced by a file of the same size:
/// that connection must not add its bytes to the old ones.
#[tokio::test]
async fn a_file_modified_since_the_first_connection_is_reported_as_changed() {
    let file = data(1000);
    let server = FtpServer::start(FtpConfig {
        mdtm: Some("20260102030405".into()),
        ..served(&file)
    })
    .await;
    let target = target(server.url("ftp", "alice:pw@", "file.bin").as_str()).unwrap();
    let addr = Destination::from(SocketAddr::from(([127, 0, 0, 1], server.port)));
    let first_saw = |modified: &str| Server {
        target: target.clone(),
        addr: addr.clone(),
        size: Some(1000),
        modified: Some(modified.into()),
        epsv_refused: AtomicBool::new(false),
    };
    assert!(first_saw("20260102030405").connect(None).await.is_ok());
    let changed = first_saw("20250101000000").connect(None).await;
    assert!(matches!(
        changed,
        Err(Failure {
            error: Error::RemoteChanged,
            ..
        })
    ));
}

/// A worker keeps its control connection from one range to the next. The file
/// changed after it logged in: the next range must not add its bytes either.
#[tokio::test]
async fn a_file_modified_before_a_kept_connection_starts_its_next_range_is_reported_as_changed() {
    let file = data(1000);
    let server = FtpServer::start(FtpConfig {
        mdtm: Some("20260102030405".into()),
        ..served(&file)
    })
    .await;
    let url = server.url("ftp", "alice:pw@", "file.bin");
    let target = target(url.as_str()).unwrap();
    let addr = Destination::from(SocketAddr::from(([127, 0, 0, 1], server.port)));
    let mut control = target.connect(&addr, None).await.unwrap();
    target.login(&mut control, false).await.unwrap();
    let mut connection = Connection::new(Arc::new(Server {
        target: target.clone(),
        addr,
        size: Some(1000),
        modified: Some("20250101000000".into()),
        epsv_refused: AtomicBool::new(false),
    }));
    connection.control = Some(control);
    let request = RequestContext {
        url,
        ranged: true,
        total: Some(1000),
        validators: Validators::default(),
        cancel: CancellationToken::new(),
        stall_timeout: std::time::Duration::from_secs(5),
    };
    let opened = connection.open(&request, 500).await;
    assert!(matches!(
        opened,
        Err(Failure {
            error: Error::RemoteChanged,
            ..
        })
    ));
}
