//! A small scripted FTP server for tests: plain, explicit TLS (`AUTH TLS`) or
//! implicit TLS; `EPSV` and `PASV` (each can be switched off); `SIZE`, `MDTM`
//! and `REST`; a session limit refused with `421` or `530`; per-connection
//! throttling and injected data connection drops. Every command is logged so
//! tests can check what the client sent.
//!
//! It depends only on tokio and rustls, so both the integration tests and the
//! engine's own unit tests (which need to trust its test certificate) use it.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::HandshakeKind;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// How a session over the limit is turned away.
#[derive(Clone, Copy, Debug)]
pub enum Refuse {
    /// `421` instead of the greeting, then the connection is closed.
    Greeting421,
    /// The greeting, then `530` for the password.
    Login530,
}

#[derive(Clone)]
pub struct FtpConfig {
    /// Files by the path the client asks for (no leading slash).
    pub files: Vec<(String, Arc<Vec<u8>>)>,
    pub mdtm: Option<String>,
    /// The only user and password accepted; `None` accepts any login.
    pub login: Option<(String, String)>,
    /// Offers `AUTH TLS` (or, with `implicit`, speaks TLS from the start).
    pub tls: Option<Arc<rustls::ServerConfig>>,
    pub implicit: bool,
    pub max_sessions: Option<(usize, Refuse)>,
    pub epsv: bool,
    pub rest: bool,
    pub size: bool,
    /// The address `PASV` names (the data port always listens on 127.0.0.1).
    pub pasv_ip: [u8; 4],
    /// Bytes per second for each data connection.
    pub data_bps: Option<u64>,
    /// The first `n` transfers break off after `after` bytes.
    pub drop_first: Option<(usize, u64)>,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            mdtm: None,
            login: None,
            tls: None,
            implicit: false,
            max_sessions: None,
            epsv: true,
            rest: true,
            size: true,
            pasv_ip: [127, 0, 0, 1],
            data_bps: None,
            drop_first: None,
        }
    }
}

#[derive(Default)]
pub struct FtpStats {
    /// Every command line received, in order, from all sessions.
    pub commands: Mutex<Vec<String>>,
    pub sessions: AtomicUsize,
    pub max_sessions: AtomicUsize,
    logged_in: AtomicUsize,
    /// Most sessions logged in at the same time.
    pub max_logged_in: AtomicUsize,
    pub refused: AtomicUsize,
    pub transfers: AtomicUsize,
    current_transfers: AtomicUsize,
    pub max_transfers: AtomicUsize,
    pub bytes_sent: AtomicU64,
    /// `REST` offsets the transfers started from.
    pub rest_offsets: Mutex<Vec<u64>>,
    /// How each TLS data connection's handshake went.
    pub data_handshakes: Mutex<Vec<HandshakeKind>>,
}

impl FtpStats {
    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    /// How many commands started with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.commands()
            .iter()
            .filter(|c| c.starts_with(prefix))
            .count()
    }
}

pub struct FtpServer {
    pub port: u16,
    pub stats: Arc<FtpStats>,
    config: Arc<Mutex<FtpConfig>>,
}

impl FtpServer {
    pub async fn start(config: FtpConfig) -> Self {
        Self::start_on(0, config).await
    }

    /// Listens on `port` of 127.0.0.1 (0 picks a free one).
    pub async fn start_on(port: u16, config: FtpConfig) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let stats = Arc::new(FtpStats::default());
        let config = Arc::new(Mutex::new(config));
        {
            let (stats, config) = (stats.clone(), config.clone());
            tokio::spawn(async move {
                loop {
                    let Ok((tcp, _)) = listener.accept().await else {
                        return;
                    };
                    let (stats, config) = (stats.clone(), config.lock().unwrap().clone());
                    tokio::spawn(async move {
                        let _ = session(tcp, config, stats).await;
                    });
                }
            });
        }
        Self {
            port,
            stats,
            config,
        }
    }

    /// `scheme://userinfo127.0.0.1:port/path`.
    pub fn url(&self, scheme: &str, userinfo: &str, path: &str) -> String {
        format!("{scheme}://{userinfo}127.0.0.1:{}/{path}", self.port)
    }

    pub fn configure(&self, change: impl FnOnce(&mut FtpConfig)) {
        change(&mut self.config.lock().unwrap());
    }
}

/// A self-signed certificate for 127.0.0.1: the server's TLS configuration
/// and the root store a client needs to trust it.
pub fn test_tls() -> (Arc<rustls::ServerConfig>, rustls::RootCertStore) {
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der.clone()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key)
        .unwrap();
    (Arc::new(config), roots)
}

/// Decrements a gauge when dropped.
struct Gauge<'a>(&'a AtomicUsize);

impl<'a> Gauge<'a> {
    fn up(current: &'a AtomicUsize, max: &AtomicUsize) -> (Self, usize) {
        let now = current.fetch_add(1, Ordering::SeqCst) + 1;
        max.fetch_max(now, Ordering::SeqCst);
        (Self(current), now)
    }
}

impl Drop for Gauge<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn reply(control: &mut BufReader<Box<dyn Io>>, text: &str) -> std::io::Result<()> {
    let stream = control.get_mut();
    stream.write_all(text.as_bytes()).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

async fn session(tcp: TcpStream, config: FtpConfig, stats: Arc<FtpStats>) -> std::io::Result<()> {
    // Like vsftpd with `require_ssl_reuse` and ProFTPD: a data connection may
    // only resume this control connection's TLS session. Session tickets are
    // sealed with a key of this session's own, so a ticket from any other
    // session cannot be resumed here (the handshake is then a full one).
    let session_tls = config.tls.as_ref().map(|tls| {
        let mut own = (**tls).clone();
        own.ticketer = rustls::crypto::ring::Ticketer::new().unwrap();
        own.session_storage = rustls::server::ServerSessionMemoryCache::new(16);
        Arc::new(own)
    });
    let mut stream: Box<dyn Io> = Box::new(tcp);
    let mut protected = false;
    if config.implicit
        && let Some(tls) = &session_tls
    {
        stream = Box::new(TlsAcceptor::from(tls.clone()).accept(stream).await?);
        protected = true;
    }
    let (_session, active) = Gauge::up(&stats.sessions, &stats.max_sessions);
    let over_limit = config.max_sessions.filter(|&(limit, _)| active > limit);
    let mut control = BufReader::new(stream);
    if let Some((_, Refuse::Greeting421)) = over_limit {
        stats.refused.fetch_add(1, Ordering::SeqCst);
        reply(
            &mut control,
            "421 Too many connections from your address. Try again later.",
        )
        .await?;
        return Ok(());
    }
    reply(
        &mut control,
        "220-Welcome to the Wirecatch test server.\r\n220-It speaks just enough FTP.\r\n220 Ready.",
    )
    .await?;

    let mut user = None;
    let mut logged_in = false;
    let mut _login_gauge = None;
    let mut data_tls = false;
    let mut rest = 0u64;
    let mut passive: Option<TcpListener> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if control.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let command = line.trim_end_matches(['\r', '\n']).to_string();
        stats.commands.lock().unwrap().push(command.clone());
        let (verb, arg) = match command.split_once(' ') {
            Some((verb, arg)) => (verb.to_ascii_uppercase(), arg.to_string()),
            None => (command.to_ascii_uppercase(), String::new()),
        };
        match verb.as_str() {
            "AUTH" if config.tls.is_some() && !protected && arg.eq_ignore_ascii_case("TLS") => {
                reply(&mut control, "234 Proceed with negotiation.").await?;
                let Some(tls) = &session_tls else {
                    unreachable!()
                };
                let plain = control.into_inner();
                control = BufReader::new(Box::new(
                    TlsAcceptor::from(tls.clone()).accept(plain).await?,
                ));
                protected = true;
            }
            "AUTH" => reply(&mut control, "502 Encryption is not available here.").await?,
            "USER" => {
                user = Some(arg);
                reply(&mut control, "331 Please specify the password.").await?;
            }
            "PASS" => {
                let accepted = match &config.login {
                    Some((name, password)) => {
                        user.as_deref() == Some(name.as_str()) && arg == *password
                    }
                    None => user.is_some(),
                };
                if !accepted {
                    reply(&mut control, "530 Login incorrect.").await?;
                } else if let Some((_, Refuse::Login530)) = over_limit {
                    stats.refused.fetch_add(1, Ordering::SeqCst);
                    reply(&mut control, "530 Sorry, the maximum number of sessions for this user are already connected.").await?;
                } else {
                    if !logged_in {
                        _login_gauge = Some(Gauge::up(&stats.logged_in, &stats.max_logged_in).0);
                    }
                    logged_in = true;
                    reply(&mut control, "230 Login successful.").await?;
                }
            }
            "QUIT" => {
                reply(&mut control, "221 Goodbye.").await?;
                return Ok(());
            }
            "NOOP" => reply(&mut control, "200 NOOP ok.").await?,
            _ if !logged_in => reply(&mut control, "530 Please login with USER and PASS.").await?,
            "PBSZ" => reply(&mut control, "200 PBSZ=0").await?,
            "PROT" if arg.eq_ignore_ascii_case("P") && protected => {
                data_tls = true;
                reply(&mut control, "200 PROT now Private.").await?;
            }
            "PROT" if arg.eq_ignore_ascii_case("C") => {
                data_tls = false;
                reply(&mut control, "200 PROT now Clear.").await?;
            }
            "PROT" => {
                reply(
                    &mut control,
                    "503 PROT P needs a secure control connection.",
                )
                .await?
            }
            "TYPE" => reply(&mut control, "200 Switching to Binary mode.").await?,
            "SIZE" if config.size => match find(&config, &arg) {
                Some(file) => reply(&mut control, &format!("213 {}", file.len())).await?,
                None => reply(&mut control, "550 Could not get file size.").await?,
            },
            "MDTM" => match (&config.mdtm, find(&config, &arg)) {
                (Some(mdtm), Some(_)) => reply(&mut control, &format!("213 {mdtm}")).await?,
                _ => reply(&mut control, "550 Could not get file modification time.").await?,
            },
            "REST" if config.rest => match arg.parse::<u64>() {
                Ok(offset) => {
                    rest = offset;
                    reply(
                        &mut control,
                        &format!("350 Restart position accepted ({offset})."),
                    )
                    .await?;
                }
                Err(_) => reply(&mut control, "501 REST needs a number.").await?,
            },
            "EPSV" if config.epsv => {
                let listener = TcpListener::bind("127.0.0.1:0").await?;
                let port = listener.local_addr()?.port();
                passive = Some(listener);
                reply(
                    &mut control,
                    &format!("229 Entering Extended Passive Mode (|||{port}|)"),
                )
                .await?;
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await?;
                let port = listener.local_addr()?.port();
                passive = Some(listener);
                let [a, b, c, d] = config.pasv_ip;
                reply(
                    &mut control,
                    &format!(
                        "227 Entering Passive Mode ({a},{b},{c},{d},{},{}).",
                        port >> 8,
                        port & 0xff
                    ),
                )
                .await?;
            }
            "RETR" => {
                let offset = std::mem::take(&mut rest);
                let Some(listener) = passive.take() else {
                    reply(&mut control, "425 Use PASV or EPSV first.").await?;
                    continue;
                };
                let Some(file) = find(&config, &arg) else {
                    reply(
                        &mut control,
                        "550 Failed to open file: no such file or directory.",
                    )
                    .await?;
                    continue;
                };
                stats.rest_offsets.lock().unwrap().push(offset);
                let index = stats.transfers.fetch_add(1, Ordering::SeqCst);
                let (_transfer, _) = Gauge::up(&stats.current_transfers, &stats.max_transfers);
                reply(
                    &mut control,
                    &format!(
                        "150 Opening BINARY mode data connection for {arg} ({} bytes).",
                        file.len()
                    ),
                )
                .await?;
                let Ok(Ok((tcp, _))) =
                    tokio::time::timeout(Duration::from_secs(10), listener.accept()).await
                else {
                    reply(&mut control, "425 Failed to establish connection.").await?;
                    continue;
                };
                let mut data: Box<dyn Io> = Box::new(tcp);
                if data_tls && let Some(tls) = &session_tls {
                    let Ok(secured) = TlsAcceptor::from(tls.clone()).accept(data).await else {
                        reply(&mut control, "522 The data connection needs TLS.").await?;
                        continue;
                    };
                    if let Some(kind) = secured.get_ref().1.handshake_kind() {
                        stats.data_handshakes.lock().unwrap().push(kind);
                    }
                    data = Box::new(secured);
                }
                let cut = config
                    .drop_first
                    .filter(|&(n, _)| index < n)
                    .map(|(_, after)| after);
                let sent = send_file(&mut data, &file, offset, config.data_bps, cut, &stats).await;
                match sent {
                    Ok(true) => {
                        let _ = data.shutdown().await;
                        drop(data);
                        reply(&mut control, "226 Transfer complete.").await?;
                    }
                    // Broken off on purpose, or the client closed the connection.
                    Ok(false) | Err(_) => {
                        drop(data);
                        reply(&mut control, "426 Connection closed; transfer aborted.").await?;
                    }
                }
            }
            _ => reply(&mut control, "502 Command not implemented.").await?,
        }
    }
}

fn find(config: &FtpConfig, path: &str) -> Option<Arc<Vec<u8>>> {
    config
        .files
        .iter()
        .find(|(name, _)| name == path)
        .map(|(_, data)| data.clone())
}

/// Sends `file` from `offset`; false if it stopped at `cut` on purpose.
async fn send_file(
    data: &mut Box<dyn Io>,
    file: &[u8],
    offset: u64,
    bps: Option<u64>,
    cut: Option<u64>,
    stats: &FtpStats,
) -> std::io::Result<bool> {
    let start = Instant::now();
    let mut position = offset.min(file.len() as u64) as usize;
    let mut sent = 0u64;
    let chunk = bps.map_or(64 * 1024, |bps| (bps / 20).clamp(1024, 64 * 1024)) as usize;
    while position < file.len() {
        let mut end = (position + chunk).min(file.len());
        if let Some(cut) = cut {
            let limit = (offset + cut) as usize;
            if position >= limit {
                return Ok(false);
            }
            end = end.min(limit);
        }
        data.write_all(&file[position..end]).await?;
        let n = (end - position) as u64;
        position = end;
        sent += n;
        stats.bytes_sent.fetch_add(n, Ordering::Relaxed);
        if let Some(bps) = bps {
            let due = Duration::from_secs_f64(sent as f64 / bps as f64);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                tokio::time::sleep(wait).await;
            }
        }
    }
    data.flush().await?;
    Ok(true)
}
