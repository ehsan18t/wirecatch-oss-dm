//! The FTP control connection: sockets that may switch to TLS, sending
//! commands and reading replies, and parsing the replies the engine relies on.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::debug;

use crate::error::Error;
use crate::http::Failure;

/// Longest reply line accepted. Real replies are far shorter; the cap keeps a
/// misbehaving server from filling memory.
const MAX_LINE: u64 = 4096;
/// Most lines in one multi-line reply.
const MAX_LINES: usize = 256;
/// Most reply text kept for an error message.
const MAX_TEXT: usize = 512;

/// Entries in one control connection's TLS session cache: room for one
/// server's tickets. rustls keeps a server's entry only while the cache holds
/// more than one server, so 8 or fewer would throw every ticket away at once.
const SESSION_CACHE_ENTRIES: usize = 32;

/// A socket, plain or encrypted.
pub(crate) enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl Stream {
    pub fn is_tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }

    /// Opens a TCP connection to `addr` within `timeout`.
    pub async fn connect(addr: SocketAddr, timeout: Duration, what: &str) -> Result<Self, Failure> {
        let tcp = match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
            Ok(Ok(tcp)) => tcp,
            Ok(Err(e)) => {
                return Err(
                    Error::socket(format!("could not open the {what} to {addr}"), e, true).into(),
                );
            }
            Err(_) => {
                return Err(Error::socket(
                    format!("could not open the {what} to {addr}"),
                    io::ErrorKind::TimedOut.into(),
                    true,
                )
                .into());
            }
        };
        // Commands are short and each waits for its reply, so Nagle's delay
        // would only slow them; without the option they still work.
        if let Err(e) = tcp.set_nodelay(true) {
            debug!(error = %e, "could not turn off Nagle's algorithm on an FTP connection");
        }
        Ok(Stream::Plain(tcp))
    }

    /// Wraps a plain socket in TLS. The server name must be the same for the
    /// control and data connections, so data connections resume the control
    /// connection's TLS session (many servers insist on it).
    pub async fn into_tls(self, tls: &Tls, timeout: Duration, what: &str) -> Result<Self, Failure> {
        let Stream::Plain(tcp) = self else {
            return Ok(self);
        };
        let handshake = tls.connector.connect(tls.server_name.clone(), tcp);
        match tokio::time::timeout(timeout, handshake).await {
            Ok(Ok(stream)) => Ok(Stream::Tls(Box::new(stream))),
            Ok(Err(e)) => {
                Err(
                    Error::socket(format!("the encrypted {what} could not be set up"), e, true)
                        .into(),
                )
            }
            Err(_) => Err(Error::socket(
                format!("the encrypted {what} could not be set up"),
                io::ErrorKind::TimedOut.into(),
                true,
            )
            .into()),
        }
    }
}

/// What TLS needs for one server.
#[derive(Clone)]
pub(crate) struct Tls {
    config: std::sync::Arc<rustls::ClientConfig>,
    pub connector: TlsConnector,
    pub server_name: rustls::pki_types::ServerName<'static>,
}

impl Tls {
    pub fn new(
        config: rustls::ClientConfig,
        server_name: rustls::pki_types::ServerName<'static>,
    ) -> Self {
        let config = std::sync::Arc::new(config);
        Self {
            connector: TlsConnector::from(config.clone()),
            config,
            server_name,
        }
    }

    /// The same settings with a TLS session cache of its own, for one control
    /// connection and its data connections. Those then resume only that
    /// control connection's session, never another's: servers such as vsftpd
    /// (`require_ssl_reuse`) and `ProFTPD` refuse a data connection that resumes
    /// a different session than its control connection.
    pub fn for_new_session(&self) -> Self {
        let mut config = (*self.config).clone();
        config.resumption = rustls::client::Resumption::in_memory_sessions(SESSION_CACHE_ENTRIES);
        Self::new(config, self.server_name.clone())
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// A server reply: its three-digit code and its text (all lines of a
/// multi-line reply, joined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    pub code: u16,
    pub text: String,
}

impl Reply {
    /// 1xx: the command started and another reply follows.
    pub fn is_preliminary(&self) -> bool {
        (100..200).contains(&self.code)
    }

    /// 2xx: the command succeeded.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.code)
    }

    /// 5xx: the command will never succeed as sent.
    pub fn is_permanent_failure(&self) -> bool {
        (500..600).contains(&self.code)
    }

    /// The text cut to a length fit for an error message.
    pub fn message(&self) -> String {
        let mut text: String = self.text.chars().take(MAX_TEXT).collect();
        if text.len() < self.text.len() {
            text.push_str("...");
        }
        text
    }
}

/// The control connection: one command at a time, each answered by a reply.
pub(crate) struct Control {
    reader: BufReader<Stream>,
    line: Vec<u8>,
    /// How long to wait for a reply.
    pub stall: Duration,
}

impl Control {
    pub fn new(stream: Stream, stall: Duration) -> Self {
        Self {
            reader: BufReader::new(stream),
            line: Vec::with_capacity(256),
            stall,
        }
    }

    /// Switches the connection to TLS after the server accepted `AUTH TLS`.
    pub async fn start_tls(self, tls: &Tls, timeout: Duration) -> Result<Self, Failure> {
        // Anything the server sent after its reply would otherwise be read as
        // if it had come encrypted (a known command injection trick).
        if !self.reader.buffer().is_empty() {
            return Err(Error::socket(
                "the FTP server sent unexpected data before encryption started",
                io::ErrorKind::InvalidData.into(),
                true,
            )
            .into());
        }
        let stall = self.stall;
        let stream = self
            .reader
            .into_inner()
            .into_tls(tls, timeout, "control connection")
            .await?;
        Ok(Self::new(stream, stall))
    }

    /// Sends one command and reads its reply. `shown` is what the debug log
    /// says was sent, so a password is never logged.
    pub async fn command(&mut self, command: &str, shown: &str) -> Result<Reply, Failure> {
        self.send(command, shown).await?;
        self.reply().await
    }

    pub async fn send(&mut self, command: &str, shown: &str) -> Result<(), Failure> {
        debug!(command = shown, "ftp");
        let stream = self.reader.get_mut();
        let write = async {
            stream.write_all(command.as_bytes()).await?;
            stream.write_all(b"\r\n").await?;
            stream.flush().await
        };
        match tokio::time::timeout(self.stall, write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(Error::socket("the FTP control connection failed", e, false).into()),
            Err(_) => Err(Error::Stalled { after: self.stall }.into()),
        }
    }

    /// Reads the next reply, waiting at most the stall timeout.
    pub async fn reply(&mut self) -> Result<Reply, Failure> {
        match tokio::time::timeout(self.stall, self.read_reply()).await {
            Ok(Ok(reply)) => {
                debug!(code = reply.code, text = %reply.message(), "ftp reply");
                Ok(reply)
            }
            Ok(Err(e)) => Err(Error::socket("the FTP control connection failed", e, false).into()),
            Err(_) => Err(Error::Stalled { after: self.stall }.into()),
        }
    }

    async fn read_reply(&mut self) -> io::Result<Reply> {
        let first = self.read_line().await?;
        let (code, separator, rest) = split_reply_line(&first)?;
        let mut text = rest.to_string();
        if separator == '-' {
            // Multi-line: ends with a line starting with the same code and a
            // space, or (as some servers send it) the bare code.
            let end = format!("{code} ");
            let bare = code.to_string();
            for _ in 0..MAX_LINES {
                let line = self.read_line().await?;
                let (content, last) = match line.strip_prefix(&end) {
                    Some(content) => (content, true),
                    None if line == bare => ("", true),
                    None => (line.as_str(), false),
                };
                if text.len() < MAX_TEXT {
                    text.push(' ');
                    text.push_str(content.trim());
                }
                if last {
                    return Ok(Reply {
                        code,
                        text: text.trim().to_string(),
                    });
                }
            }
            return Err(invalid("a reply with too many lines"));
        }
        Ok(Reply {
            code,
            text: text.trim().to_string(),
        })
    }

    async fn read_line(&mut self) -> io::Result<String> {
        self.line.clear();
        let n = (&mut self.reader)
            .take(MAX_LINE)
            .read_until(b'\n', &mut self.line)
            .await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the server closed the connection",
            ));
        }
        if self.line.last() != Some(&b'\n') {
            return Err(invalid("a reply line that is too long"));
        }
        let line = String::from_utf8_lossy(&self.line);
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the FTP server sent {what}"),
    )
}

/// Splits `229-text` or `229 text` into code, separator and text.
fn split_reply_line(line: &str) -> io::Result<(u16, char, &str)> {
    let bytes = line.as_bytes();
    if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
        return Err(invalid("a reply that is not FTP"));
    }
    let code: u16 = line[..3]
        .parse()
        .map_err(|_| invalid("a reply that is not FTP"))?;
    if !(100..600).contains(&code) {
        return Err(invalid("a reply that is not FTP"));
    }
    match bytes.get(3) {
        None => Ok((code, ' ', "")),
        Some(b' ') => Ok((code, ' ', &line[4..])),
        Some(b'-') => Ok((code, '-', &line[4..])),
        Some(_) => Err(invalid("a reply that is not FTP")),
    }
}

/// The port in `229 Entering Extended Passive Mode (|||6446|)` (RFC 2428).
pub(crate) fn parse_epsv(text: &str) -> Option<u16> {
    let start = text.find('(')?;
    let end = start + text[start..].find(')')?;
    let inner = &text[start + 1..end];
    let delimiter = inner.chars().next()?;
    // `<d><d><d>port<d>`: network and address are left empty.
    let parts: Vec<&str> = inner.split(delimiter).collect();
    if parts.len() != 5 || !parts[1].is_empty() || !parts[2].is_empty() {
        return None;
    }
    parts[3].parse().ok().filter(|&port| port != 0)
}

/// The port in `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)` (RFC 959).
///
/// The address in the reply is ignored on purpose: data connections always
/// go to the address the control connection uses. That works behind servers
/// that report a private address, and stops a server from steering the client
/// at another machine.
pub(crate) fn parse_pasv(text: &str) -> Option<u16> {
    // Some servers leave out the parentheses; then take the first six numbers.
    let start = match text.find('(') {
        Some(open) => open + 1,
        None => text.find(|c: char| c.is_ascii_digit())?,
    };
    let numbers: Vec<u16> = text[start..]
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(6)
        .map(|s| s.parse().ok().filter(|&n| n <= 255))
        .collect::<Option<_>>()?;
    if numbers.len() != 6 {
        return None;
    }
    let port = numbers[4] << 8 | numbers[5];
    (port != 0).then_some(port)
}

/// The size in `213 12345`.
pub(crate) fn parse_size(text: &str) -> Option<u64> {
    text.split_whitespace().next()?.parse().ok()
}

/// The size many servers add to their `150` reply, as in
/// `150 Opening BINARY mode data connection for file.bin (12345 bytes)`.
pub(crate) fn parse_transfer_size(text: &str) -> Option<u64> {
    let end = text.rfind(" bytes)")?;
    let start = text[..end].rfind('(')? + 1;
    text[start..end].trim().parse().ok()
}

/// The time in `213 20240102030405` or `213 20240102030405.123`, kept as
/// text: it only needs to be compared with itself on a later run.
pub(crate) fn parse_mdtm(text: &str) -> Option<String> {
    let value = text.split_whitespace().next()?;
    let digits = value.split('.').next()?;
    (digits.len() == 14 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control connection to a local socket that sends `script`.
    async fn control_reading(script: &'static [u8]) -> Control {
        use tokio::io::AsyncWriteExt as _;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut server, _) = listener.accept().await.unwrap();
            server.write_all(script).await.unwrap();
            // Keep the connection open so a reader waiting for more stalls.
            std::future::pending::<()>().await;
        });
        let stream = Stream::connect(addr, Duration::from_secs(5), "test connection")
            .await
            .unwrap();
        Control::new(stream, Duration::from_secs(2))
    }

    #[tokio::test]
    async fn multi_line_replies_end_with_the_code_alone_too() {
        let mut control =
            control_reading(b"211-Features:\r\n EPSV\r\n UTF8\r\n211\r\n200 Next.\r\n").await;
        let features = control.reply().await.unwrap();
        assert_eq!(features.code, 211);
        assert_eq!(features.text, "Features: EPSV UTF8");
        let next = control.reply().await.unwrap();
        assert_eq!((next.code, next.text.as_str()), (200, "Next."));

        // The usual form, with a space and text after the code.
        let mut control = control_reading(b"220-Hello\r\n220 Ready\r\n").await;
        assert_eq!(control.reply().await.unwrap().text, "Hello Ready");
    }

    #[test]
    fn reply_lines() {
        assert_eq!(split_reply_line("220 Ready").unwrap(), (220, ' ', "Ready"));
        assert_eq!(
            split_reply_line("211-Features:").unwrap(),
            (211, '-', "Features:")
        );
        assert_eq!(split_reply_line("200").unwrap(), (200, ' ', ""));
        assert!(split_reply_line("HTTP/1.1 400 Bad Request").is_err());
        assert!(split_reply_line("22").is_err());
        assert!(split_reply_line("099 low").is_err());
        assert!(split_reply_line("220x").is_err());
    }

    #[test]
    fn passive_replies() {
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (|||6446|)"),
            Some(6446)
        );
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (!!!6446!)"),
            Some(6446)
        );
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (|1|10.0.0.1|6446|)"),
            None
        );
        assert_eq!(parse_epsv("Entering Extended Passive Mode (|||0|)"), None);
        assert_eq!(parse_epsv("nothing here"), None);

        assert_eq!(
            parse_pasv("Entering Passive Mode (192,168,1,2,19,137)."),
            Some(19 * 256 + 137)
        );
        assert_eq!(parse_pasv("Entering Passive Mode 10,0,0,1,4,1"), Some(1025));
        assert_eq!(
            parse_pasv("Entering Passive Mode v4 (10,0,0,1,4,2)"),
            Some(1026)
        );
        assert_eq!(parse_pasv("Entering Passive Mode (300,1,1,1,4,1)"), None);
        assert_eq!(parse_pasv("Entering Passive Mode (1,2,3)"), None);
    }

    #[test]
    fn sizes_and_times() {
        assert_eq!(parse_size("1048576"), Some(1_048_576));
        assert_eq!(parse_size("big"), None);
        assert_eq!(
            parse_transfer_size("Opening BINARY mode data connection for a (x).bin (12345 bytes)"),
            Some(12345)
        );
        assert_eq!(parse_transfer_size("Here comes the file"), None);
        assert_eq!(parse_mdtm("20240102030405"), Some("20240102030405".into()));
        assert_eq!(
            parse_mdtm("20240102030405.250"),
            Some("20240102030405.250".into())
        );
        assert_eq!(parse_mdtm("2024"), None);
    }
}
