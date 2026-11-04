//! A proxy on the loopback address that keeps a run of yt-dlp off the local network (D136). The engine's guard cannot reach another process, and yt-dlp has no such rule of its own, so every request and redirect hop of the run comes through here, and goes on only to an address on the internet.

#![forbid(unsafe_code)]

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use wirecatch_engine::Url;
use wirecatch_engine::units::KIB_USIZE;
use wirecatch_sites::local_network::is_local;

/// The longest request head read; yt-dlp's are a few kilobytes, signed addresses and cookies included.
const MAX_HEAD: usize = 64 * KIB_USIZE;
/// How long a name may take to resolve and a connection to open.
const REACH_TIMEOUT: Duration = Duration::from_secs(30);

const TUNNEL_OPEN: &[u8] = b"HTTP/1.1 200 Connection established\r\n\r\n";
const REFUSED: &[u8] = b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
const UNREACHABLE: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// A running guard. Dropping it stops it and every connection through it.
pub(super) struct Guard {
    address: SocketAddr,
    refused: Arc<AtomicBool>,
    serving: JoinHandle<()>,
}

impl Guard {
    /// Starts a guard for one run.
    pub(super) async fn start() -> io::Result<Self> {
        Self::keeping_off(is_local).await
    }

    async fn keeping_off(kept_off: fn(IpAddr) -> bool) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let refused = Arc::new(AtomicBool::new(false));
        let serving = tokio::spawn(serve(listener, kept_off, refused.clone()));
        Ok(Self {
            address,
            refused,
            serving,
        })
    }

    /// The address to give yt-dlp as its proxy.
    pub(super) fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Whether a request of the run was stopped for leading into the local network.
    pub(super) fn refused_any(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.serving.abort();
    }
}

async fn serve(listener: TcpListener, kept_off: fn(IpAddr) -> bool, refused: Arc<AtomicBool>) {
    // Dropped with this task, which aborts every connection.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((client, _)) => {
                    connections.spawn(carry(client, kept_off, refused.clone()));
                }
                Err(e) => tracing::debug!(error = %e, "the local network guard missed a connection"),
            },
            Some(_) = connections.join_next() => {}
        }
    }
}

/// Where a request head asks to go.
#[derive(Debug, PartialEq, Eq)]
struct Destination {
    host: String,
    port: u16,
    /// The head to send on; `None` for a tunnel, which sends nothing of its own.
    head: Option<Vec<u8>>,
}

/// The head of a request that is about to go on: first line, then its lines without what is for a proxy, and asking the server to close, so the client's next request comes back through here.
fn read_request(head: &[u8]) -> Option<Destination> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if first.next().is_some() {
        return None;
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = target.rsplit_once(':')?;
        return Some(Destination {
            host: host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned(),
            port: port.parse().ok()?,
            head: None,
        });
    }
    let url = Url::parse(target).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let mut sent = format!("{method} {path} {version}\r\n");
    for line in lines.take_while(|line| !line.is_empty()) {
        let name = line.split(':').next().unwrap_or_default();
        let dropped = ["connection", "proxy-connection", "proxy-authorization"]
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n));
        if !dropped {
            sent.push_str(line);
            sent.push_str("\r\n");
        }
    }
    sent.push_str("Connection: close\r\n\r\n");
    Some(Destination {
        host: url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
        port: url.port_or_known_default()?,
        head: Some(sent.into_bytes()),
    })
}

/// Reads up to the end of a request head: the head and what came after it. `None` when it ends first or runs past [`MAX_HEAD`].
async fn read_head(client: &mut TcpStream) -> Option<(Vec<u8>, Vec<u8>)> {
    const END: &[u8] = b"\r\n\r\n";
    let mut bytes = Vec::with_capacity(KIB_USIZE);
    let mut chunk = [0_u8; KIB_USIZE];
    loop {
        let read = client.read(&mut chunk).await.ok()?;
        let new = chunk.get(..read).filter(|new| !new.is_empty())?;
        bytes.extend_from_slice(new);
        if let Some(at) = bytes.windows(END.len()).position(|w| w == END) {
            let rest = bytes.split_off(at + END.len());
            return Some((bytes, rest));
        }
        if bytes.len() > MAX_HEAD {
            return None;
        }
    }
}

/// How a host's addresses stand under the guard.
#[derive(Debug, PartialEq, Eq)]
enum Reach {
    Open(Vec<SocketAddr>),
    Refused,
    NotFound,
}

/// Every address of a name must be on the internet, and only those looked up here are connected to, so a name that answers otherwise later is never asked again.
async fn reach(host: &str, port: u16, kept_off: fn(IpAddr) -> bool) -> Reach {
    let found: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        match tokio::time::timeout(REACH_TIMEOUT, tokio::net::lookup_host((host, port))).await {
            Ok(Ok(addresses)) => addresses.collect(),
            _ => Vec::new(),
        }
    };
    if found.is_empty() {
        Reach::NotFound
    } else if found.iter().any(|address| kept_off(address.ip())) {
        Reach::Refused
    } else {
        Reach::Open(found)
    }
}

async fn connect(addresses: &[SocketAddr]) -> Option<TcpStream> {
    tokio::time::timeout(REACH_TIMEOUT, TcpStream::connect(addresses))
        .await
        .ok()?
        .ok()
}

/// Carries one connection of yt-dlp's to where its request asks, if that is allowed.
async fn carry(mut client: TcpStream, kept_off: fn(IpAddr) -> bool, refused: Arc<AtomicBool>) {
    let Some((head, rest)) = read_head(&mut client).await else {
        client_may_have_gone(&client.write_all(BAD_REQUEST).await);
        return;
    };
    let Some(destination) = read_request(&head) else {
        client_may_have_gone(&client.write_all(BAD_REQUEST).await);
        return;
    };
    let addresses = match reach(&destination.host, destination.port, kept_off).await {
        Reach::Open(addresses) => addresses,
        Reach::Refused => {
            refused.store(true, Ordering::Relaxed);
            tracing::warn!("stopped a request of yt-dlp's that led into the local network");
            client_may_have_gone(&client.write_all(REFUSED).await);
            return;
        }
        Reach::NotFound => {
            client_may_have_gone(&client.write_all(UNREACHABLE).await);
            return;
        }
    };
    let Some(mut server) = connect(&addresses).await else {
        client_may_have_gone(&client.write_all(UNREACHABLE).await);
        return;
    };
    let opened = match &destination.head {
        Some(sent) => server.write_all(sent).await,
        None => client.write_all(TUNNEL_OPEN).await,
    };
    let carried = match opened {
        Ok(()) => match server.write_all(&rest).await {
            Ok(()) => copy_bidirectional(&mut client, &mut server).await.map(drop),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    if let Err(e) = carried {
        tracing::debug!(error = %e, "a connection through the local network guard ended early");
    }
}

/// Takes the outcome of answering: a client that has gone is not told, which changes nothing.
fn client_may_have_gone(told: &io::Result<()>) {
    let (Ok(()) | Err(_)) = told;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_kept_off(_: IpAddr) -> bool {
        false
    }

    /// What the guard answers to `request`, to the end of its answer.
    async fn answer(guard: &Guard, request: &str) -> String {
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let mut text = Vec::new();
        client.read_to_end(&mut text).await.unwrap();
        String::from_utf8_lossy(&text).into_owned()
    }

    async fn local_server() -> TcpListener {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap()
    }

    #[tokio::test]
    async fn a_tunnel_to_the_local_network_is_refused() {
        let guard = Guard::start().await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let said = answer(
            &guard,
            &format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n"),
        )
        .await;
        assert!(said.starts_with("HTTP/1.1 403"), "{said}");
        assert!(guard.refused_any());
    }

    #[tokio::test]
    async fn a_plain_request_to_the_local_network_is_refused() {
        let guard = Guard::start().await.unwrap();
        for target in [
            "http://127.0.0.1:9/",
            "http://192.168.0.1/admin",
            "http://[::1]:9/",
            "http://localhost:9/",
        ] {
            let said = answer(&guard, &format!("GET {target} HTTP/1.1\r\n\r\n")).await;
            assert!(said.starts_with("HTTP/1.1 403"), "{target}: {said}");
        }
    }

    /// The form of a link-local address `Url::parse` rejects, which a redirect can still name: it never goes on.
    #[tokio::test]
    async fn a_link_local_address_with_a_zone_goes_nowhere() {
        let guard = Guard::start().await.unwrap();
        for request in [
            "CONNECT [fe80::1%25eth0]:443 HTTP/1.1\r\n\r\n",
            "GET http://[fe80::1%25eth0]/ HTTP/1.1\r\n\r\n",
        ] {
            let said = answer(&guard, request).await;
            assert!(!said.starts_with("HTTP/1.1 200"), "{request}: {said}");
        }
    }

    /// The guard stops reading, so the client may see its connection reset rather than the answer.
    #[tokio::test]
    async fn a_head_past_the_limit_goes_nowhere() {
        let guard = Guard::start().await.unwrap();
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        let long = "x".repeat(MAX_HEAD * 2);
        let wrote = client
            .write_all(format!("GET http://a/{long}").as_bytes())
            .await;
        let mut said = Vec::new();
        let read = client.read_to_end(&mut said).await;
        assert!(wrote.is_err() || read.is_err() || said.starts_with(b"HTTP/1.1 400"));
    }

    #[tokio::test]
    async fn only_web_requests_go_on() {
        let guard = Guard::start().await.unwrap();
        for request in [
            "GET ftp://example.com/ HTTP/1.1\r\n\r\n",
            "GET /origin-form HTTP/1.1\r\n\r\n",
            "nonsense\r\n\r\n",
        ] {
            let said = answer(&guard, request).await;
            assert!(said.starts_with("HTTP/1.1 400"), "{request}: {said}");
        }
    }

    /// Where it is allowed, a tunnel carries bytes both ways.
    #[tokio::test]
    async fn an_allowed_tunnel_carries_both_ways() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let echo = tokio::spawn(async move {
            let (mut peer, _) = server.accept().await.unwrap();
            let mut ping = [0_u8; 4];
            peer.read_exact(&mut ping).await.unwrap();
            peer.write_all(b"pong").await.unwrap();
            ping
        });
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        client
            .write_all(format!("CONNECT {target} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut opened = vec![0_u8; TUNNEL_OPEN.len()];
        client.read_exact(&mut opened).await.unwrap();
        assert_eq!(opened, TUNNEL_OPEN);
        client.write_all(b"ping").await.unwrap();
        let mut pong = [0_u8; 4];
        client.read_exact(&mut pong).await.unwrap();
        assert_eq!(&pong, b"pong");
        assert_eq!(&echo.await.unwrap(), b"ping");
    }

    /// A plain request reaches the server in the form a server reads, without what was for a proxy, and closes, so the next request comes back through the guard.
    #[tokio::test]
    async fn an_allowed_plain_request_is_rewritten_for_the_server() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let heard = tokio::spawn(async move {
            let (mut peer, _) = server.accept().await.unwrap();
            let (head, _) = read_head(&mut peer).await.unwrap();
            peer.write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(head).unwrap()
        });
        let said = answer(
            &guard,
            &format!(
                "GET http://{target}/a?b=c HTTP/1.1\r\nHost: {target}\r\nProxy-Authorization: x\r\nProxy-Connection: keep-alive\r\nConnection: keep-alive\r\n\r\n"
            ),
        )
        .await;
        assert!(said.starts_with("HTTP/1.1 204"), "{said}");
        assert_eq!(
            heard.await.unwrap(),
            format!("GET /a?b=c HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n")
        );
    }
}
