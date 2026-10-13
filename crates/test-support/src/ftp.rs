//! A small scripted FTP server for tests: plain, explicit TLS (`AUTH TLS`) or
//! implicit TLS; `EPSV` and `PASV`; `SIZE`, `MDTM` and `REST`; a session limit
//! refused with `421` or `530`; per-connection throttling, injected data
//! connection drops and a set of [`Quirk`]s real servers have. Every command is
//! logged so tests can check what the client sent.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::HandshakeKind;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use url::Url;

use crate::TestCertificate;
use crate::http::{index, time_to_send};

/// How long a `RETR` waits for the client to open the data connection.
const DATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Most bytes written to a data connection at once.
const MAX_CHUNK: u64 = 64 * 1024;
/// The fewest bytes written to a throttled data connection at once.
const MIN_THROTTLED_CHUNK: u64 = 1024;
/// A throttled data connection sends its speed in this many pieces a second.
const THROTTLE_STEPS_PER_SECOND: u64 = 20;
/// TLS sessions each control connection's own cache keeps.
const SESSION_CACHE_SIZE: usize = 16;

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

/// A way the server departs from a complete, well-behaved one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quirk {
    /// Does not know `EPSV`, so the client must use `PASV`.
    NoEpsv,
    /// Does not know `REST`, so a transfer cannot start part way.
    NoRest,
    /// Does not know `SIZE`.
    NoSize,
    /// Sends a forged reply right behind `234`, before TLS starts.
    InjectAfterAuth,
    /// Answers `PROT P` with `534`.
    RefuseProtP,
    /// Answers `EPSV` with a `229` that names no port.
    EpsvGarbage,
    /// Names that are not ASCII are only found after `OPTS UTF8 ON`, as on
    /// servers that otherwise read paths in a legacy encoding.
    Utf8AfterOpts,
    /// Answers `OPTS` with `501`.
    RefuseOpts,
}

/// How the server behaves.
#[derive(Clone)]
pub struct FtpConfig {
    /// Files by the path the client asks for (no leading slash).
    pub files: Vec<(String, Arc<Vec<u8>>)>,
    /// What `MDTM` answers for every file; without it `MDTM` fails.
    pub mdtm: Option<String>,
    /// The only user and password accepted; `None` accepts any login.
    pub login: Option<(String, String)>,
    /// Offers `AUTH TLS` (or, with `implicit`, speaks TLS from the start).
    pub tls: Option<Arc<rustls::ServerConfig>>,
    /// Speaks TLS from the first byte instead of after `AUTH TLS`.
    pub implicit: bool,
    /// At most this many sessions at once, and how the ones beyond are refused.
    pub max_sessions: Option<(usize, Refuse)>,
    /// The address `PASV` names (the data port always listens on 127.0.0.1).
    pub pasv_ip: [u8; 4],
    /// Bytes per second for each data connection.
    pub data_bps: Option<u64>,
    /// The first `n` transfers break off after `after` bytes.
    pub drop_first: Option<(usize, u64)>,
    /// How long to hold back the `426` after the client closed a data
    /// connection early.
    pub hold_abort_reply: Option<Duration>,
    /// Sessions after the first `n` are accepted but never greeted.
    pub silent_after: Option<usize>,
    /// Transfer `n` (0 is the first the server started, as
    /// [`FtpStats::transfers`] counts them), if it is from byte 0, opens its
    /// data connection and then sends nothing until the client closes it: the
    /// probe a resuming client breaks off sends no byte of the file, however
    /// long the client takes, and a client that reads it on waits forever.
    /// Only that transfer is held, so a resumed range that starts at byte 0 is
    /// sent as usual.
    pub hold_probe: Option<usize>,
    /// Ways the server misbehaves.
    pub quirks: Vec<Quirk>,
}

impl FtpConfig {
    fn has(&self, quirk: Quirk) -> bool {
        self.quirks.contains(&quirk)
    }

    fn find(&self, path: &str) -> Option<Arc<Vec<u8>>> {
        self.files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, data)| data.clone())
    }
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
            pasv_ip: [127, 0, 0, 1],
            data_bps: None,
            drop_first: None,
            hold_abort_reply: None,
            silent_after: None,
            hold_probe: None,
            quirks: Vec::new(),
        }
    }
}

/// What the server has done so far.
#[derive(Default)]
pub struct FtpStats {
    /// Every command line received, in order, from all sessions.
    pub commands: Mutex<Vec<String>>,
    /// Sessions open now.
    pub sessions: AtomicUsize,
    /// Most sessions open at the same time.
    pub max_sessions: AtomicUsize,
    logged_in: AtomicUsize,
    /// Most sessions logged in at the same time.
    pub max_logged_in: AtomicUsize,
    /// Sessions turned away by `max_sessions`.
    pub refused: AtomicUsize,
    accepted: AtomicUsize,
    /// Transfers started.
    pub transfers: AtomicUsize,
    current_transfers: AtomicUsize,
    /// Most transfers running at the same time.
    pub max_transfers: AtomicUsize,
    /// File bytes sent over data connections.
    pub bytes_sent: AtomicU64,
    /// `REST` offsets the transfers started from.
    pub rest_offsets: Mutex<Vec<u64>>,
    /// Each transfer's offset and the file bytes it has sent so far.
    sent_by_transfer: Mutex<Vec<(u64, Arc<AtomicU64>)>>,
    /// `426` replies currently held back (see `hold_abort_reply`).
    pub held_replies: AtomicUsize,
    /// Transfers started on a control connection whose previous transfer
    /// the client had cut short.
    pub reused_after_abort: AtomicUsize,
    /// How each TLS data connection's handshake went.
    pub data_handshakes: Mutex<Vec<HandshakeKind>>,
}

impl FtpStats {
    /// A copy of every command received so far.
    #[must_use]
    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    /// Every transfer started so far, in order: the offset it started from,
    /// and the file bytes it has sent. A transfer the client broke off still
    /// counts what went out before it did.
    #[must_use]
    pub fn transfers_sent(&self) -> Vec<(u64, u64)> {
        self.sent_by_transfer
            .lock()
            .unwrap()
            .iter()
            .map(|(from, sent)| (*from, sent.load(Ordering::SeqCst)))
            .collect()
    }

    /// How many commands started with `prefix`.
    #[must_use]
    pub fn count(&self, prefix: &str) -> usize {
        self.commands()
            .iter()
            .filter(|c| c.starts_with(prefix))
            .count()
    }
}

/// A running server; it stops with the test's runtime.
pub struct FtpServer {
    /// The port it listens on, on 127.0.0.1.
    pub port: u16,
    /// What the server has done so far.
    pub stats: Arc<FtpStats>,
    config: Arc<Mutex<FtpConfig>>,
}

impl FtpServer {
    /// Listens on a free port of 127.0.0.1.
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
                while let Ok((tcp, _)) = listener.accept().await {
                    let (stats, config) = (stats.clone(), config.lock().unwrap().clone());
                    tokio::spawn(async move {
                        // A session ends when the client goes away; that is not a failure.
                        if let Err(error) = session(tcp, config, stats).await {
                            drop(error);
                        }
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
    #[must_use]
    pub fn url(&self, scheme: &str, userinfo: &str, path: &str) -> Url {
        Url::parse(&format!(
            "{scheme}://{userinfo}127.0.0.1:{}/{path}",
            self.port
        ))
        .unwrap()
    }

    /// Changes the configuration for the sessions that follow.
    pub fn configure(&self, change: impl FnOnce(&mut FtpConfig)) {
        change(&mut self.config.lock().unwrap());
    }
}

/// A self-signed certificate for 127.0.0.1: the server's TLS configuration
/// and the root store a client needs to trust it.
#[must_use]
pub fn test_tls() -> (Arc<rustls::ServerConfig>, rustls::RootCertStore) {
    let certificate = TestCertificate::new();
    (
        Arc::new(certificate.server_config(&[])),
        certificate.roots(),
    )
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

/// Which connections of a session are encrypted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Protection {
    Plain,
    /// The control connection only (`PROT C`, the default after TLS starts).
    Control,
    /// The control connection and every data connection (`PROT P`).
    ControlAndData,
}

/// What to do after a command.
enum Then {
    Next,
    Quit,
}

/// One control connection.
struct Session<'a> {
    config: FtpConfig,
    stats: &'a FtpStats,
    control: BufReader<Box<dyn Io>>,
    /// This session's own TLS configuration.
    tls: Option<Arc<rustls::ServerConfig>>,
    protection: Protection,
    /// Sessions over the limit, and how they are refused.
    over_limit: Option<Refuse>,
    user: Option<String>,
    login_gauge: Option<Gauge<'a>>,
    rest: u64,
    passive: Option<TcpListener>,
    last_aborted: bool,
    utf8: bool,
}

async fn session(tcp: TcpStream, config: FtpConfig, stats: Arc<FtpStats>) -> std::io::Result<()> {
    // Like vsftpd with `require_ssl_reuse` and ProFTPD: a data connection may
    // only resume this control connection's TLS session. Session tickets are
    // sealed with a key of this session's own, so a ticket from any other
    // session cannot be resumed here (the handshake is then a full one).
    let tls = config.tls.as_ref().map(|tls| {
        let mut own = (**tls).clone();
        own.ticketer = rustls::crypto::ring::Ticketer::new().unwrap();
        own.session_storage = rustls::server::ServerSessionMemoryCache::new(SESSION_CACHE_SIZE);
        Arc::new(own)
    });
    let mut stream: Box<dyn Io> = Box::new(tcp);
    let mut protection = Protection::Plain;
    if config.implicit
        && let Some(tls) = &tls
    {
        stream = Box::new(TlsAcceptor::from(tls.clone()).accept(stream).await?);
        protection = Protection::Control;
    }
    let (_session, active) = Gauge::up(&stats.sessions, &stats.max_sessions);
    let over_limit = config
        .max_sessions
        .filter(|&(limit, _)| active > limit)
        .map(|(_, refuse)| refuse);
    if config
        .silent_after
        .is_some_and(|n| stats.accepted.fetch_add(1, Ordering::SeqCst) >= n)
    {
        std::future::pending::<()>().await;
    }
    let mut session = Session {
        config,
        stats: &stats,
        control: BufReader::new(stream),
        tls,
        protection,
        over_limit,
        user: None,
        login_gauge: None,
        rest: 0,
        passive: None,
        last_aborted: false,
        utf8: false,
    };
    session.run().await
}

impl Session<'_> {
    async fn run(&mut self) -> std::io::Result<()> {
        if let Some(Refuse::Greeting421) = self.over_limit {
            self.stats.refused.fetch_add(1, Ordering::SeqCst);
            return self
                .reply("421 Too many connections from your address. Try again later.")
                .await;
        }
        self.reply(
            "220-Welcome to the Wirecatch test server.\r\n220-It speaks just enough FTP.\r\n220 Ready.",
        )
        .await?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.control.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            let command = line.trim_end_matches(['\r', '\n']).to_string();
            self.stats.commands.lock().unwrap().push(command.clone());
            let (verb, arg) = match command.split_once(' ') {
                Some((verb, arg)) => (verb.to_ascii_uppercase(), arg.to_string()),
                None => (command.to_ascii_uppercase(), String::new()),
            };
            if let Then::Quit = self.command(&verb, arg).await? {
                return Ok(());
            }
        }
    }

    async fn reply(&mut self, text: &str) -> std::io::Result<()> {
        let stream = self.control.get_mut();
        stream.write_all(text.as_bytes()).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await
    }

    async fn command(&mut self, verb: &str, arg: String) -> std::io::Result<Then> {
        match verb {
            "AUTH" => self.auth(&arg).await?,
            "USER" => {
                self.user = Some(arg);
                self.reply("331 Please specify the password.").await?;
            }
            "PASS" => self.pass(&arg).await?,
            "QUIT" => {
                self.reply("221 Goodbye.").await?;
                return Ok(Then::Quit);
            }
            "NOOP" => self.reply("200 NOOP ok.").await?,
            "OPTS" => self.opts(&arg).await?,
            _ if self.login_gauge.is_none() => {
                self.reply("530 Please login with USER and PASS.").await?;
            }
            _ => self.logged_in_command(verb, &arg).await?,
        }
        Ok(Then::Next)
    }

    /// Commands that need a login first.
    async fn logged_in_command(&mut self, verb: &str, arg: &str) -> std::io::Result<()> {
        match verb {
            "PBSZ" => self.reply("200 PBSZ=0").await,
            "PROT" => self.prot(arg).await,
            "TYPE" => self.reply("200 Switching to Binary mode.").await,
            "SIZE" if !self.config.has(Quirk::NoSize) => match self.config.find(arg) {
                Some(file) => self.reply(&format!("213 {}", file.len())).await,
                None => self.reply("550 Could not get file size.").await,
            },
            "MDTM" => match (self.config.mdtm.clone(), self.config.find(arg)) {
                (Some(mdtm), Some(_)) => self.reply(&format!("213 {mdtm}")).await,
                _ => {
                    self.reply("550 Could not get file modification time.")
                        .await
                }
            },
            "REST" if !self.config.has(Quirk::NoRest) => {
                if let Ok(offset) = arg.parse::<u64>() {
                    self.rest = offset;
                    self.reply(&format!("350 Restart position accepted ({offset})."))
                        .await
                } else {
                    self.reply("501 REST needs a number.").await
                }
            }
            "EPSV" if self.config.has(Quirk::EpsvGarbage) => {
                self.reply("229 Entering Extended Passive Mode (somewhere)")
                    .await
            }
            "EPSV" if !self.config.has(Quirk::NoEpsv) => {
                let port = self.listen().await?;
                self.reply(&format!("229 Entering Extended Passive Mode (|||{port}|)"))
                    .await
            }
            "PASV" => {
                let port = self.listen().await?;
                let [a, b, c, d] = self.config.pasv_ip;
                let [high, low] = port.to_be_bytes();
                self.reply(&format!(
                    "227 Entering Passive Mode ({a},{b},{c},{d},{high},{low})."
                ))
                .await
            }
            "RETR" => self.retr(arg).await,
            _ => self.reply("502 Command not implemented.").await,
        }
    }

    async fn auth(&mut self, arg: &str) -> std::io::Result<()> {
        let plain = self.protection == Protection::Plain;
        let Some(tls) = self.tls.clone().filter(|_| plain) else {
            return self.reply("502 Encryption is not available here.").await;
        };
        if !arg.eq_ignore_ascii_case("TLS") {
            return self.reply("502 Encryption is not available here.").await;
        }
        let answer = if self.config.has(Quirk::InjectAfterAuth) {
            "234 Proceed with negotiation.\n230 Forged: logged in."
        } else {
            "234 Proceed with negotiation."
        };
        self.reply(answer).await?;
        let plain = std::mem::replace(
            &mut self.control,
            BufReader::new(Box::new(tokio::io::empty())),
        )
        .into_inner();
        self.control = BufReader::new(Box::new(TlsAcceptor::from(tls).accept(plain).await?));
        self.protection = Protection::Control;
        Ok(())
    }

    async fn pass(&mut self, password: &str) -> std::io::Result<()> {
        let accepted = match &self.config.login {
            Some((name, expected)) => {
                self.user.as_deref() == Some(name.as_str()) && password == expected
            }
            None => self.user.is_some(),
        };
        if !accepted {
            return self.reply("530 Login incorrect.").await;
        }
        if let Some(Refuse::Login530) = self.over_limit {
            self.stats.refused.fetch_add(1, Ordering::SeqCst);
            return self
                .reply("530 Sorry, the maximum number of sessions for this user are already connected.")
                .await;
        }
        if self.login_gauge.is_none() {
            let stats = self.stats;
            self.login_gauge = Some(Gauge::up(&stats.logged_in, &stats.max_logged_in).0);
        }
        self.reply("230 Login successful.").await
    }

    async fn opts(&mut self, arg: &str) -> std::io::Result<()> {
        if self.config.has(Quirk::RefuseOpts) {
            self.reply("501 Option not understood.").await
        } else if arg.eq_ignore_ascii_case("UTF8 ON") {
            self.utf8 = true;
            self.reply("200 Always in UTF8 mode.").await
        } else if self.login_gauge.is_none() {
            self.reply("530 Please login with USER and PASS.").await
        } else {
            self.reply("502 Command not implemented.").await
        }
    }

    async fn prot(&mut self, arg: &str) -> std::io::Result<()> {
        if self.config.has(Quirk::RefuseProtP) {
            self.reply("534 Data protection is not available.").await
        } else if arg.eq_ignore_ascii_case("P") && self.protection != Protection::Plain {
            self.protection = Protection::ControlAndData;
            self.reply("200 PROT now Private.").await
        } else if arg.eq_ignore_ascii_case("C") {
            if self.protection == Protection::ControlAndData {
                self.protection = Protection::Control;
            }
            self.reply("200 PROT now Clear.").await
        } else {
            self.reply("503 PROT P needs a secure control connection.")
                .await
        }
    }

    /// Opens a passive data port; returns its number.
    async fn listen(&mut self) -> std::io::Result<u16> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        self.passive = Some(listener);
        Ok(port)
    }

    async fn retr(&mut self, path: &str) -> std::io::Result<()> {
        let offset = std::mem::take(&mut self.rest);
        if std::mem::take(&mut self.last_aborted) {
            self.stats.reused_after_abort.fetch_add(1, Ordering::SeqCst);
        }
        let Some(listener) = self.passive.take() else {
            return self.reply("425 Use PASV or EPSV first.").await;
        };
        let readable = self.utf8 || !self.config.has(Quirk::Utf8AfterOpts) || path.is_ascii();
        let Some(file) = self.config.find(path).filter(|_| readable) else {
            return self
                .reply("550 Failed to open file: no such file or directory.")
                .await;
        };
        self.stats.rest_offsets.lock().unwrap().push(offset);
        let sent_here = Arc::new(AtomicU64::new(0));
        self.stats
            .sent_by_transfer
            .lock()
            .unwrap()
            .push((offset, sent_here.clone()));
        let transfer = self.stats.transfers.fetch_add(1, Ordering::SeqCst);
        let (_transfer, _) = Gauge::up(&self.stats.current_transfers, &self.stats.max_transfers);
        self.reply(&format!(
            "150 Opening BINARY mode data connection for {path} ({} bytes).",
            file.len()
        ))
        .await?;
        let Some(mut data) = self.accept_data(&listener).await? else {
            return Ok(());
        };
        let cut = self
            .config
            .drop_first
            .filter(|&(n, _)| transfer < n)
            .map(|(_, after)| after);
        let throttle = self.config.data_bps;
        let counts = (self.stats, &*sent_here);
        let sent = if offset == 0 && self.config.hold_probe == Some(transfer) {
            held(&mut data).await
        } else {
            send_file(&mut data, &file, offset, throttle, cut, counts).await
        };
        match sent {
            Ok(Sent::All) => {
                // The client has all of it; a failed close changes nothing for the test.
                if let Err(error) = data.shutdown().await {
                    drop(error);
                }
                drop(data);
                self.reply("226 Transfer complete.").await
            }
            // Broken off on purpose, or the client closed the connection.
            Ok(Sent::Cut) | Err(_) => {
                drop(data);
                self.last_aborted = true;
                if let Some(hold) = self.config.hold_abort_reply {
                    self.stats.held_replies.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(hold).await;
                }
                self.reply("426 Connection closed; transfer aborted.").await
            }
        }
    }

    /// Waits for the client's data connection, with TLS if `PROT P` asked for it.
    /// Replies and returns `None` when none arrives.
    async fn accept_data(
        &mut self,
        listener: &TcpListener,
    ) -> std::io::Result<Option<Box<dyn Io>>> {
        let Ok(Ok((tcp, _))) = tokio::time::timeout(DATA_CONNECT_TIMEOUT, listener.accept()).await
        else {
            self.reply("425 Failed to establish connection.").await?;
            return Ok(None);
        };
        let data: Box<dyn Io> = Box::new(tcp);
        let encrypted = self.protection == Protection::ControlAndData;
        let Some(tls) = self.tls.clone().filter(|_| encrypted) else {
            return Ok(Some(data));
        };
        let Ok(secured) = TlsAcceptor::from(tls).accept(data).await else {
            self.reply("522 The data connection needs TLS.").await?;
            return Ok(None);
        };
        if let Some(kind) = secured.get_ref().1.handshake_kind() {
            self.stats.data_handshakes.lock().unwrap().push(kind);
        }
        Ok(Some(Box::new(secured)))
    }
}

/// How a transfer ended.
enum Sent {
    All,
    /// Stopped at the configured cut, on purpose.
    Cut,
}

/// Sends nothing on `data` until the client closes it.
async fn held(data: &mut Box<dyn Io>) -> std::io::Result<Sent> {
    let mut byte = [0];
    while data.read(&mut byte).await? > 0 {}
    Ok(Sent::Cut)
}

/// Sends `file` from `offset`, at `bps` bytes per second if given, stopping
/// after `cut` bytes if given, counting what it sends in the server's stats
/// and in the transfer's own count.
async fn send_file(
    data: &mut Box<dyn Io>,
    file: &[u8],
    offset: u64,
    bps: Option<u64>,
    cut: Option<u64>,
    (stats, sent_here): (&FtpStats, &AtomicU64),
) -> std::io::Result<Sent> {
    let start = Instant::now();
    let len = file.len() as u64;
    let mut position = offset.min(len);
    let mut sent = 0u64;
    let chunk = bps.map_or(MAX_CHUNK, |bps| {
        (bps / THROTTLE_STEPS_PER_SECOND).clamp(MIN_THROTTLED_CHUNK, MAX_CHUNK)
    });
    while position < len {
        let mut end = (position + chunk).min(len);
        if let Some(cut) = cut {
            let limit = offset + cut;
            if position >= limit {
                return Ok(Sent::Cut);
            }
            end = end.min(limit);
        }
        data.write_all(&file[index(position)..index(end)]).await?;
        let n = end - position;
        position = end;
        sent += n;
        stats.bytes_sent.fetch_add(n, Ordering::Relaxed);
        sent_here.fetch_add(n, Ordering::SeqCst);
        if let Some(bps) = bps
            && let Some(wait) = time_to_send(sent, bps).checked_sub(start.elapsed())
        {
            tokio::time::sleep(wait).await;
        }
    }
    data.flush().await?;
    Ok(Sent::All)
}
