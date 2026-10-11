//! FTP and FTPS downloads.
//!
//! Every worker keeps one logged-in control connection and opens a passive
//! data connection for each range: `REST <offset>` and `RETR`, read until the
//! range is complete, then close the data connection and carry on with the
//! next range on the same control connection. The first connection finds out
//! the size (`SIZE`), the modification time (`MDTM`, the resume validator) and
//! whether the server can start a transfer at an offset (`REST`), and is
//! already receiving the file from byte 0 when it hands over to the workers.
//!
//! Which connection is encrypted follows the URL and never falls back to less:
//!
//! - `ftp://`: plain FTP, default port 21. No encryption is attempted.
//! - `ftps://`: TLS from the first byte (implicit TLS), default port 990; on
//!   port 21 it means `AUTH TLS` on a plain connection (explicit TLS).
//! - `ftpes://`: explicit TLS (`AUTH TLS`) on any port, default 21.
//!
//! With TLS, data connections are encrypted too (`PROT P`), and resume the
//! control connection's TLS session, which many servers require.

mod control;
#[cfg(test)]
mod tests;

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use percent_encoding::percent_decode_str;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use url::{Host, Url};

use crate::download::{DownloadOptions, Shared};
use crate::error::Error;
use crate::http::{Failure, Validators};
use control::{Control, Reply, Stream, Tls};

/// Bytes read from a data connection at once. One buffer per connection,
/// reused for every range it downloads.
const READ_BUFFER: usize = 128 * 1024;

/// How long to wait for the server's answer after closing a data connection
/// early, before giving up on reusing the control connection.
const FINISH_WAIT: Duration = Duration::from_secs(5);

/// The user name and password used when the URL has none (RFC 1635).
const ANONYMOUS: (&str, &str) = ("anonymous", "anonymous@");

/// How the connections to the server are encrypted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Security {
    /// No encryption.
    Plain,
    /// A plain connection switched to TLS with `AUTH TLS`.
    Explicit,
    /// TLS from the first byte.
    Implicit,
}

/// The encryption and port a URL asks for; see the module documentation.
pub(crate) fn security_and_port(url: &Url) -> Option<(Security, u16)> {
    // `url` drops the default port of `ftp` (21), not of `ftps` or `ftpes`.
    match url.scheme() {
        "ftp" => Some((Security::Plain, url.port().unwrap_or(21))),
        "ftpes" => Some((Security::Explicit, url.port().unwrap_or(21))),
        "ftps" => Some(match url.port() {
            Some(21) => (Security::Explicit, 21),
            Some(port) => (Security::Implicit, port),
            None => (Security::Implicit, 990),
        }),
        _ => None,
    }
}

/// Everything about a server that is known before talking to it.
///
/// Deliberately not `Debug`: it holds the password.
#[derive(Clone)]
pub(crate) struct Target {
    host: Host<String>,
    port: u16,
    security: Security,
    user: String,
    password: String,
    /// The file's path, as sent in commands.
    path: String,
    /// The URL without its password, for messages and the resume file.
    display_url: Url,
    tls: Option<Tls>,
    connect_timeout: Duration,
    stall: Duration,
}

impl Target {
    /// Reads the URL. Fails for anything that cannot be sent safely, such as a
    /// line break in the path, which would let the URL inject FTP commands.
    pub fn new(
        url: &Url,
        opts: &DownloadOptions,
        tls: &rustls::ClientConfig,
    ) -> Result<Self, Error> {
        let invalid = |why: &str| Error::InvalidUrl(format!("{} {why}", without_password(url)));
        let (security, port) =
            security_and_port(url).ok_or_else(|| invalid("is not an FTP link"))?;
        let host = match url.host() {
            None => return Err(invalid("has no server name")),
            // `url` turns international names into their ASCII form (IDNA)
            // only for the schemes it knows, which include `ftp` but not
            // `ftps` or `ftpes`; there the name arrives percent-encoded.
            Some(Host::Domain(name)) => {
                let name = percent_decode_str(name)
                    .decode_utf8()
                    .map_err(|_| invalid("has a server name that is not valid UTF-8"))?;
                Host::parse(&name).map_err(|_| invalid("has a server name that cannot be used"))?
            }
            Some(other) => other.to_owned(),
        };

        let decode = |text: &str| {
            percent_decode_str(text)
                .decode_utf8()
                .map(|s| s.into_owned())
                .map_err(|_| invalid("contains characters that are not valid UTF-8"))
        };
        let mut path = url
            .path()
            .strip_prefix('/')
            .unwrap_or(url.path())
            .to_string();
        // `;type=i` and friends (RFC 1738): transfers are always binary.
        if let Some(at) = path.rfind(";type=") {
            path.truncate(at);
        }
        // Relative to the login folder, as RFC 1738 says; `%2F` makes it absolute.
        let path = decode(&path)?;
        if path.is_empty() || path.ends_with('/') {
            return Err(invalid("points to a folder, not a file"));
        }
        let (user, password) = if url.username().is_empty() {
            (
                ANONYMOUS.0.to_string(),
                url.password()
                    .map(decode)
                    .transpose()?
                    .unwrap_or(ANONYMOUS.1.into()),
            )
        } else {
            (
                decode(url.username())?,
                url.password().map(decode).transpose()?.unwrap_or_default(),
            )
        };
        if [&path, &user, &password]
            .iter()
            .any(|s| s.contains(['\r', '\n', '\0']))
        {
            return Err(invalid("contains a line break, which FTP cannot send"));
        }

        let tls = (security != Security::Plain)
            .then(|| -> Result<Tls, Error> {
                let mut config = tls.clone();
                config.alpn_protocols.clear();
                // Each control connection gets a session cache of its own
                // (see `Tls::for_new_session`); this one is never used.
                config.resumption = rustls::client::Resumption::disabled();
                let server_name = match &host {
                    Host::Domain(name) => rustls::pki_types::ServerName::try_from(name.clone())
                        .map_err(|_| invalid("has a server name that cannot be used with TLS"))?,
                    Host::Ipv4(ip) => rustls::pki_types::ServerName::IpAddress((*ip).into()),
                    Host::Ipv6(ip) => rustls::pki_types::ServerName::IpAddress((*ip).into()),
                };
                Ok(Tls::new(config, server_name))
            })
            .transpose()?;

        Ok(Self {
            host,
            port,
            security,
            user,
            password,
            path,
            display_url: without_password(url),
            tls,
            connect_timeout: opts.connect_timeout,
            stall: opts.stall_timeout,
        })
    }

    /// The URL without its password.
    pub fn display_url(&self) -> &Url {
        &self.display_url
    }

    fn host_name(&self) -> String {
        self.host.to_string()
    }

    /// The server's addresses, as the system resolves its name.
    async fn resolve(&self) -> Result<Vec<SocketAddr>, Failure> {
        let name = match &self.host {
            Host::Ipv4(ip) => return Ok(vec![SocketAddr::new((*ip).into(), self.port)]),
            Host::Ipv6(ip) => return Ok(vec![SocketAddr::new((*ip).into(), self.port)]),
            Host::Domain(name) => name,
        };
        let what = format!("could not find the FTP server {name}");
        match tokio::time::timeout(
            self.connect_timeout,
            tokio::net::lookup_host((name.as_str(), self.port)),
        )
        .await
        {
            Ok(Ok(found)) => {
                let found: Vec<_> = found.collect();
                if found.is_empty() {
                    Err(Error::socket(what, io::ErrorKind::NotFound.into(), true).into())
                } else {
                    Ok(found)
                }
            }
            Ok(Err(e)) => Err(Error::socket(what, e, true).into()),
            Err(_) => Err(Error::socket(what, io::ErrorKind::TimedOut.into(), true).into()),
        }
    }

    /// Connects to `addr`, reads the greeting and sets up TLS as the URL asks,
    /// with `tls`: the session cache of this control connection.
    async fn connect(&self, addr: SocketAddr, tls: Option<&Tls>) -> Result<Control, Failure> {
        let mut stream =
            Stream::connect(addr, self.connect_timeout, "FTP control connection").await?;
        if let (Security::Implicit, Some(tls)) = (self.security, tls) {
            stream = stream
                .into_tls(tls, self.connect_timeout, "control connection")
                .await?;
        }
        let mut control = Control::new(stream, self.stall);
        let mut greeting = control.reply().await?;
        // 120: "ready in a few minutes"; the real greeting follows.
        while greeting.code == 120 {
            greeting = control.reply().await?;
        }
        if greeting.code != 220 {
            // 421 is how most servers say they have too many connections.
            return Err(self.reply_failure(&greeting));
        }
        if let (Security::Explicit, Some(tls)) = (self.security, tls) {
            let reply = control.command("AUTH TLS", "AUTH TLS").await?;
            if reply.code != 234 {
                return Err(if (400..500).contains(&reply.code) {
                    self.reply_failure(&reply)
                } else {
                    Error::FtpTlsUnavailable {
                        host: self.host_name(),
                    }
                    .into()
                });
            }
            control = control.start_tls(tls, self.connect_timeout).await?;
        }
        Ok(control)
    }

    /// Logs in and switches to binary transfers. `first` is the first
    /// connection of the download: a refused login there means wrong
    /// credentials. On a later connection the same login already worked, so a
    /// refusal means the server allows no more connections.
    async fn login(&self, control: &mut Control, first: bool) -> Result<(), Failure> {
        let mut reply = control
            .command(&format!("USER {}", self.user), "USER <hidden>")
            .await?;
        if reply.code == 331 {
            reply = control
                .command(&format!("PASS {}", self.password), "PASS <hidden>")
                .await?;
        }
        match reply.code {
            230 | 202 => {}
            400..500 => return Err(self.reply_failure(&reply)),
            530 if first => {
                return Err(Error::FtpLoginRefused {
                    host: self.host_name(),
                    code: reply.code,
                    message: reply.message(),
                }
                .into());
            }
            500..600 => return Err(self.reply_failure(&reply)),
            // 332 asks for an account (ACCT), which almost no server still uses.
            _ => return Err(self.reply_failure(&reply)),
        }
        if self.tls.is_some() {
            // Data connections must be encrypted too; a server that refuses
            // to is not used rather than sending the file in the clear.
            for command in ["PBSZ 0", "PROT P"] {
                let reply = control.command(command, command).await?;
                if !reply.is_success() {
                    return Err(if (400..500).contains(&reply.code) {
                        self.reply_failure(&reply)
                    } else {
                        Error::FtpTlsUnavailable {
                            host: self.host_name(),
                        }
                        .into()
                    });
                }
            }
        }
        let reply = control.command("TYPE I", "TYPE I").await?;
        if !reply.is_success() {
            return Err(self.reply_failure(&reply));
        }
        Ok(())
    }

    /// `SIZE` of the file, if the server supports it.
    async fn size(&self, control: &mut Control) -> Result<Option<u64>, Failure> {
        let reply = control
            .command(&format!("SIZE {}", self.path), "SIZE")
            .await?;
        Ok((reply.code == 213)
            .then(|| control::parse_size(&reply.text))
            .flatten())
    }

    /// An error reply as a failure: 4xx temporary, 5xx permanent. Any other
    /// code where an error was expected means the conversation went wrong,
    /// which a new connection may fix.
    fn reply_failure(&self, reply: &Reply) -> Failure {
        if (400..600).contains(&reply.code) {
            Error::FtpReply {
                code: reply.code,
                message: reply.message(),
                url: self.display_url.to_string(),
            }
            .into()
        } else {
            Error::socket(
                format!(
                    "the FTP server answered \"{} {}\" unexpectedly",
                    reply.code,
                    reply.message()
                ),
                io::ErrorKind::InvalidData.into(),
                false,
            )
            .into()
        }
    }
}

/// `url` with its password removed.
pub(crate) fn without_password(url: &Url) -> Url {
    let mut clean = url.clone();
    // Fails only for URLs that cannot carry credentials, which then have none.
    let _ = clean.set_password(None);
    clean
}

/// The server as the first connection found it; shared by every connection of
/// one download.
pub(crate) struct Server {
    target: Target,
    /// The address the first connection reached. Every later connection,
    /// control or data, goes to the same address: the same machine serves
    /// every range, and credentials only ever go there.
    addr: SocketAddr,
    /// The size the first connection saw, to notice a file that changed.
    size: Option<u64>,
    /// Set once the server refused `EPSV`; `PASV` is used from then on.
    epsv_refused: AtomicBool,
}

impl Server {
    /// A new, logged-in control connection for one more worker.
    async fn connect(&self, tls: Option<&Tls>) -> Result<Control, Failure> {
        let mut control = self.target.connect(self.addr, tls).await?;
        self.target.login(&mut control, false).await?;
        if let Some(expected) = self.size
            && let Some(now) = self.target.size(&mut control).await?
            && now != expected
        {
            return Err(Error::RemoteChanged.into());
        }
        Ok(control)
    }

    /// Asks for a passive data port: `EPSV` (RFC 2428), else `PASV`.
    async fn passive_port(&self, control: &mut Control) -> Result<u16, Failure> {
        if !self.epsv_refused.load(Ordering::Relaxed) {
            let reply = control.command("EPSV", "EPSV").await?;
            if reply.code == 229 {
                if let Some(port) = control::parse_epsv(&reply.text) {
                    return Ok(port);
                }
                // An answer that cannot be read would be the same next time.
                debug!(text = %reply.message(), "the FTP server's EPSV answer cannot be read; using PASV");
            } else if !reply.is_permanent_failure() {
                return Err(self.target.reply_failure(&reply));
            } else {
                debug!("the FTP server does not support EPSV; using PASV");
            }
            self.epsv_refused.store(true, Ordering::Relaxed);
        }
        let reply = control.command("PASV", "PASV").await?;
        if reply.code == 227 {
            control::parse_pasv(&reply.text).ok_or_else(|| unexpected(&reply))
        } else {
            Err(self.target.reply_failure(&reply))
        }
    }

    /// Starts receiving the file from `from` on `control`: a passive data
    /// connection, `REST` when starting past 0, then `RETR`.
    async fn start(
        &self,
        mut control: Control,
        from: u64,
        buffer: Vec<u8>,
        tls: Option<&Tls>,
    ) -> Result<Transfer, Failure> {
        let port = self.passive_port(&mut control).await?;
        let data_addr = SocketAddr::new(self.addr.ip(), port);
        let mut data = Stream::connect(
            data_addr,
            self.target.connect_timeout,
            "FTP data connection",
        )
        .await?;
        if from > 0 {
            let reply = control
                .command(&format!("REST {from}"), &format!("REST {from}"))
                .await?;
            if reply.code != 350 {
                return Err(self.target.reply_failure(&reply));
            }
        }
        let reply = control
            .command(&format!("RETR {}", self.target.path), "RETR")
            .await?;
        if !reply.is_preliminary() {
            return Err(self.target.reply_failure(&reply));
        }
        if let Some(tls) = tls {
            data = data
                .into_tls(tls, self.target.connect_timeout, "data connection")
                .await?;
        }
        Ok(Transfer {
            control,
            data: Some(data),
            buffer,
            phase: Phase::Receiving,
            announced_size: control::parse_transfer_size(&reply.text),
            display_url: self.target.display_url.to_string(),
        })
    }
}

fn unexpected(reply: &Reply) -> Failure {
    Error::socket(
        format!(
            "the FTP server sent a reply that could not be read: \"{} {}\"",
            reply.code,
            reply.message()
        ),
        io::ErrorKind::InvalidData.into(),
        false,
    )
    .into()
}

/// What the first connection found out, and the transfer it already started.
pub(crate) struct Probe {
    pub server: Arc<Server>,
    /// The first worker's connection; its control connection is inside
    /// `transfer` until that ends.
    pub connection: Connection,
    pub transfer: Transfer,
    pub total: Option<u64>,
    pub ranged: bool,
    pub validators: Validators,
}

/// Connects, logs in, asks for the size, modification time and offset
/// support, and starts receiving the file from byte 0, so the first worker
/// starts with data already flowing (and a missing file or a wrong password is
/// known before anything is written).
pub(crate) async fn probe(target: &Target, cancel: &CancellationToken) -> Result<Probe, Failure> {
    let work = async {
        let tls = target.tls.as_ref().map(Tls::for_new_session);
        let mut last = None;
        let mut connected = None;
        for addr in target.resolve().await? {
            match target.connect(addr, tls.as_ref()).await {
                Ok(control) => {
                    connected = Some((control, addr));
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let Some((mut control, addr)) = connected else {
            return Err(
                last.unwrap_or_else(|| Error::Internal("no address to connect to".into()).into())
            );
        };
        target.login(&mut control, true).await?;
        let size = target.size(&mut control).await?;
        let mdtm = control
            .command(&format!("MDTM {}", target.path), "MDTM")
            .await?;
        let modified = (mdtm.code == 213)
            .then(|| control::parse_mdtm(&mdtm.text))
            .flatten();
        // `REST 0` is harmless (the transfer starts at 0 anyway) and tells
        // whether ranges are possible.
        let rest = control.command("REST 0", "REST 0").await?.code == 350;

        let server = Arc::new(Server {
            target: target.clone(),
            addr,
            size,
            epsv_refused: AtomicBool::new(false),
        });
        let transfer = server
            .start(control, 0, vec![0; READ_BUFFER], tls.as_ref())
            .await?;
        let total = size.or(transfer.announced_size);
        debug!(?total, rest, modified = ?modified, "ftp probe");
        Ok(Probe {
            connection: Connection {
                server: server.clone(),
                control: None,
                buffer: None,
                tls,
            },
            server,
            transfer,
            total,
            ranged: rest && total.is_some(),
            validators: Validators {
                etag: None,
                last_modified: modified,
            },
        })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled.into()),
        probed = work => probed,
    }
}

/// One worker's connection: a logged-in control connection kept between
/// ranges, and the read buffer its transfers use.
pub(crate) struct Connection {
    server: Arc<Server>,
    control: Option<Control>,
    buffer: Option<Vec<u8>>,
    /// TLS with the session cache of the current control connection; a new
    /// control connection starts a new one.
    tls: Option<Tls>,
}

impl Connection {
    pub fn new(server: Arc<Server>) -> Self {
        Self {
            server,
            control: None,
            buffer: None,
            tls: None,
        }
    }

    /// Starts receiving the file from `from`, logging in first if this worker
    /// has no control connection yet.
    pub async fn open(&mut self, shared: &Shared, from: u64) -> Result<Transfer, Failure> {
        let work = async {
            let control = match self.control.take() {
                Some(control) => control,
                None => {
                    self.tls = self.server.target.tls.as_ref().map(Tls::for_new_session);
                    self.server.connect(self.tls.as_ref()).await?
                }
            };
            let buffer = self.buffer.take().unwrap_or_else(|| vec![0; READ_BUFFER]);
            // Without range support, every attempt starts at 0 and the worker
            // skips what it already has.
            let from = if shared.ranged { from } else { 0 };
            self.server
                .start(control, from, buffer, self.tls.as_ref())
                .await
        };
        tokio::select! {
            biased;
            _ = shared.cancel.cancelled() => Err(Error::Cancelled.into()),
            started = work => started,
        }
    }

    /// Ends a transfer. If the server confirmed it, or answers quickly after
    /// the data connection is closed early, the control connection is kept for
    /// the next range; otherwise it is closed and the next range logs in again.
    pub async fn finish(&mut self, transfer: Transfer, cancel: &CancellationToken) {
        let Transfer {
            mut control,
            data,
            buffer,
            phase,
            ..
        } = transfer;
        self.buffer = Some(buffer);
        match phase {
            Phase::Confirmed => self.control = Some(control),
            // Cut off while reading the final reply: the next reply could be
            // that one, so the conversation cannot continue.
            Phase::Confirming | Phase::Broken => {}
            Phase::Receiving => {
                // Closing the data connection ends the transfer; the server
                // answers with 426 or 226.
                drop(data);
                let wait = control.stall.min(FINISH_WAIT);
                // A pause never waits for the server's answer.
                let reply = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return,
                    reply = tokio::time::timeout(wait, control.reply()) => reply,
                };
                if let Ok(Ok(reply)) = reply
                    && !reply.is_preliminary()
                {
                    self.control = Some(control);
                }
            }
        }
    }
}

/// Where a transfer stands, which decides whether its control connection can
/// be reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Data is flowing.
    Receiving,
    /// The data connection ended; reading the server's final reply.
    Confirming,
    /// The final reply was read.
    Confirmed,
    /// The control connection failed.
    Broken,
}

/// A file transfer in progress: the data connection, and the control
/// connection that waits for its final reply.
pub(crate) struct Transfer {
    control: Control,
    data: Option<Stream>,
    buffer: Vec<u8>,
    phase: Phase,
    /// The size the `150` reply mentioned, if any.
    announced_size: Option<u64>,
    display_url: String,
}

impl Transfer {
    /// The next piece of the file, or `None` once the data connection ended
    /// and the server confirmed the transfer was complete.
    pub async fn next(&mut self) -> Result<Option<&[u8]>, Error> {
        let Some(data) = &mut self.data else {
            return Ok(None);
        };
        let tls = data.is_tls();
        let n = match data.read(&mut self.buffer).await {
            Ok(n) => n,
            // Many servers close a TLS data connection without the TLS goodbye
            // (close_notify). The final reply below still says whether the
            // transfer was complete, and sizes are checked by the caller.
            Err(e) if tls && e.kind() == io::ErrorKind::UnexpectedEof => 0,
            Err(e) => {
                return Err(Error::socket(
                    "receiving the file from the FTP server failed",
                    e,
                    false,
                ));
            }
        };
        if n > 0 {
            return Ok(Some(&self.buffer[..n]));
        }
        self.data = None;
        self.phase = Phase::Confirming;
        let reply = match self.control.reply().await {
            Ok(reply) => reply,
            Err(f) => {
                self.phase = Phase::Broken;
                return Err(f.error);
            }
        };
        self.phase = Phase::Confirmed;
        if reply.is_success() {
            Ok(None)
        } else if (400..600).contains(&reply.code) {
            Err(Error::FtpReply {
                code: reply.code,
                message: reply.message(),
                url: self.display_url.clone(),
            })
        } else {
            self.phase = Phase::Broken;
            Err(unexpected(&reply).error)
        }
    }
}
