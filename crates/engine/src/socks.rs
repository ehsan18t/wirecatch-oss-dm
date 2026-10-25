//! A SOCKS5 client (RFC 1928, with the user name and password login of RFC
//! 1929) for the FTP connections that go through a SOCKS proxy. The HTTP client
//! speaks SOCKS itself.

use std::fmt;
use std::io;
use std::net::SocketAddr;

use percent_encoding::percent_decode_str;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

const VERSION: u8 = 5;
const NO_LOGIN: u8 = 0;
const LOGIN: u8 = 2;
const NO_METHOD: u8 = 0xFF;
const LOGIN_VERSION: u8 = 1;
const CONNECT: u8 = 1;
const IPV4: u8 = 1;
const DOMAIN: u8 = 3;
const IPV6: u8 = 4;
const SUCCEEDED: u8 = 0;

/// A SOCKS5 proxy, read from its URL.
///
/// Deliberately shows no login in `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Socks5 {
    host: String,
    port: u16,
    login: Option<(String, String)>,
    /// Names are sent to the proxy to resolve (`socks5h://`), rather than
    /// resolved here (`socks5://`).
    proxy_resolves: bool,
}

impl fmt::Debug for Socks5 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Socks5")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("login", &self.login.as_ref().map(|_| "***"))
            .field("proxy_resolves", &self.proxy_resolves)
            .finish()
    }
}

/// Where a connection through the proxy goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Destination {
    Address(SocketAddr),
    /// A name the proxy resolves, and the port.
    Name(String, u16),
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(addr) => write!(f, "{addr}"),
            Self::Name(name, port) => write!(f, "{name}:{port}"),
        }
    }
}

/// Why a connection through the proxy could not be opened.
#[derive(Debug)]
pub(crate) enum SocksFailure {
    /// The proxy itself could not be reached, or the connection to it broke.
    Proxy(io::Error),
    /// The proxy wants a login it was not given, or refused the one it was.
    LoginRefused,
    /// The proxy could not open the connection; the reason as it gave it.
    Refused(&'static str),
    /// The proxy answered something that is not SOCKS5.
    Garbled(&'static str),
}

impl Socks5 {
    /// The proxy a `socks5://` or `socks5h://` URL names, its port 1080 when
    /// the URL has none; `None` for any other URL.
    pub fn from_url(url: &Url) -> Option<Self> {
        let proxy_resolves = match url.scheme() {
            "socks5" => false,
            "socks5h" => true,
            _ => return None,
        };
        let decode = |text: &str| percent_decode_str(text).decode_utf8_lossy().into_owned();
        let login = (!url.username().is_empty()).then(|| {
            (
                decode(url.username()),
                url.password().map(decode).unwrap_or_default(),
            )
        });
        let host = match url.host()? {
            url::Host::Ipv6(ip) => ip.to_string(),
            other => other.to_string(),
        };
        Some(Self {
            host,
            port: url.port().unwrap_or(1080),
            login,
            proxy_resolves,
        })
    }

    /// Whether names go to the proxy unresolved.
    pub fn resolves_names(&self) -> bool {
        self.proxy_resolves
    }

    /// The proxy's address, for messages.
    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Opens a TCP connection to `to` through the proxy. Time limits are the
    /// caller's.
    pub async fn connect(&self, to: &Destination) -> Result<TcpStream, SocksFailure> {
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(SocksFailure::Proxy)?;
        self.handshake(&mut stream, to).await?;
        Ok(stream)
    }

    async fn handshake(
        &self,
        stream: &mut TcpStream,
        to: &Destination,
    ) -> Result<(), SocksFailure> {
        let methods: &[u8] = if self.login.is_some() {
            &[NO_LOGIN, LOGIN]
        } else {
            &[NO_LOGIN]
        };
        let mut greeting = vec![VERSION, u8::try_from(methods.len()).unwrap_or(1)];
        greeting.extend_from_slice(methods);
        send(stream, &greeting).await?;
        let [version, method] = receive::<2>(stream).await?;
        if version != VERSION {
            return Err(SocksFailure::Garbled("the proxy does not speak SOCKS5"));
        }
        match (method, &self.login) {
            (NO_LOGIN, _) => {}
            (LOGIN, Some((user, password))) => self.log_in(stream, user, password).await?,
            (NO_METHOD | LOGIN, _) => return Err(SocksFailure::LoginRefused),
            _ => {
                return Err(SocksFailure::Garbled(
                    "the proxy chose a login it was not offered",
                ));
            }
        }

        let mut request = vec![VERSION, CONNECT, 0];
        match to {
            Destination::Address(SocketAddr::V4(addr)) => {
                request.push(IPV4);
                request.extend_from_slice(&addr.ip().octets());
                request.extend_from_slice(&addr.port().to_be_bytes());
            }
            Destination::Address(SocketAddr::V6(addr)) => {
                request.push(IPV6);
                request.extend_from_slice(&addr.ip().octets());
                request.extend_from_slice(&addr.port().to_be_bytes());
            }
            Destination::Name(name, port) => {
                let length = u8::try_from(name.len())
                    .map_err(|_| SocksFailure::Refused("the server name is too long for SOCKS5"))?;
                request.extend_from_slice(&[DOMAIN, length]);
                request.extend_from_slice(name.as_bytes());
                request.extend_from_slice(&port.to_be_bytes());
            }
        }
        send(stream, &request).await?;
        let [version, reply, _, kind] = receive::<4>(stream).await?;
        if version != VERSION {
            return Err(SocksFailure::Garbled("the proxy's answer is not SOCKS5"));
        }
        if reply != SUCCEEDED {
            return Err(SocksFailure::Refused(reason(reply)));
        }
        // The address the proxy connected from, which the connection does
        // not need.
        let rest = match kind {
            IPV4 => 4 + 2,
            IPV6 => 16 + 2,
            DOMAIN => usize::from(receive::<1>(stream).await?[0]) + 2,
            _ => {
                return Err(SocksFailure::Garbled(
                    "the proxy's answer has an unknown address type",
                ));
            }
        };
        let mut skipped = vec![0; rest];
        stream
            .read_exact(&mut skipped)
            .await
            .map_err(SocksFailure::Proxy)?;
        Ok(())
    }

    async fn log_in(
        &self,
        stream: &mut TcpStream,
        user: &str,
        password: &str,
    ) -> Result<(), SocksFailure> {
        // RFC 1929 fits each in 255 bytes; a longer one cannot be what the
        // proxy expects.
        let (Ok(user_length), Ok(password_length)) =
            (u8::try_from(user.len()), u8::try_from(password.len()))
        else {
            return Err(SocksFailure::LoginRefused);
        };
        let mut request = vec![LOGIN_VERSION, user_length];
        request.extend_from_slice(user.as_bytes());
        request.push(password_length);
        request.extend_from_slice(password.as_bytes());
        send(stream, &request).await?;
        let [_, status] = receive::<2>(stream).await?;
        if status == SUCCEEDED {
            Ok(())
        } else {
            Err(SocksFailure::LoginRefused)
        }
    }
}

/// An address as a destination, for connections to a server already resolved.
impl From<SocketAddr> for Destination {
    fn from(addr: SocketAddr) -> Self {
        Self::Address(addr)
    }
}

impl Destination {
    /// The same host on another port, for an FTP data connection.
    pub fn with_port(&self, port: u16) -> Self {
        match self {
            Self::Address(addr) => Self::Address(SocketAddr::new(addr.ip(), port)),
            Self::Name(name, _) => Self::Name(name.clone(), port),
        }
    }
}

async fn send(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), SocksFailure> {
    stream.write_all(bytes).await.map_err(SocksFailure::Proxy)
}

async fn receive<const N: usize>(stream: &mut TcpStream) -> Result<[u8; N], SocksFailure> {
    let mut bytes = [0; N];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(SocksFailure::Proxy)?;
    Ok(bytes)
}

/// A SOCKS5 reply code as words (RFC 1928, section 6).
fn reason(reply: u8) -> &'static str {
    match reply {
        2 => "the proxy's rules do not allow this connection",
        3 => "the proxy cannot reach that network",
        4 => "the proxy cannot reach the server",
        5 => "the server refused the connection",
        6 => "the connection timed out at the proxy",
        7 => "the proxy does not support connecting",
        8 => "the proxy does not support this kind of address",
        _ => "the proxy could not open the connection",
    }
}

#[cfg(test)]
mod tests;
