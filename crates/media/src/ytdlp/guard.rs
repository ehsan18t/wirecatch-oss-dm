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

/// How much of a response is held on its way.
const RELAY_BUFFER: usize = 16 * KIB_USIZE;
/// How long to wait after a connection could not be accepted, which can repeat while the system is out of handles.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
/// Headers about this connection, not the request.
const HOP_BY_HOP: [&str; 4] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
];

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
                Err(e) => {
                    tracing::debug!(error = %e, "the local network guard missed a connection");
                    tokio::time::sleep(ACCEPT_RETRY).await;
                }
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
    /// What a plain request sends on; `None` for a tunnel, which sends nothing of its own.
    request: Option<PlainRequest>,
}

#[derive(Debug, PartialEq, Eq)]
struct PlainRequest {
    head: Vec<u8>,
    body_len: u64,
}

/// The request line's target as a server reads it, exactly as the client wrote it: only its scheme and host go.
fn origin_form(target: &str) -> Option<String> {
    let (_, rest) = target.split_once("://")?;
    Some(match rest.find(['/', '?']) {
        Some(at) => match rest.get(at..)? {
            query if query.starts_with('?') => format!("/{query}"),
            path => path.to_owned(),
        },
        None => "/".to_owned(),
    })
}

/// Reads a request head. A body whose length the head does not give is refused, as is one that waits for an answer first: only the bytes of one request ever go on, and they are sent before any answer is read.
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
            request: None,
        });
    }
    let url = Url::parse(target).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let mut sent = format!("{method} {} {version}\r\n", origin_form(target)?);
    let mut body_len = 0;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap_or((line, ""));
        if name.eq_ignore_ascii_case("transfer-encoding") || name.eq_ignore_ascii_case("expect") {
            return None;
        }
        if name.eq_ignore_ascii_case("content-length") {
            body_len = value.trim().parse().ok()?;
        }
        if !HOP_BY_HOP.iter().any(|n| name.eq_ignore_ascii_case(n)) {
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
        request: Some(PlainRequest {
            head: sent.into_bytes(),
            body_len,
        }),
    })
}

/// A response head that tells the client to close, so its next request comes through a new connection and is checked: this one carries no more.
fn closing_response(head: &[u8]) -> Vec<u8> {
    const END: &[u8] = b"\r\n";
    let mut sent = Vec::with_capacity(head.len());
    let mut lines = head
        .split(|b| *b == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l));
    if let Some(status) = lines.next() {
        sent.extend_from_slice(status);
        sent.extend_from_slice(END);
    }
    // First: a client stops reading a head at a line it cannot parse.
    sent.extend_from_slice(b"Connection: close\r\n");
    for line in lines.take_while(|line| !line.is_empty()) {
        let name = line.split(|b| *b == b':').next().unwrap_or_default();
        if !HOP_BY_HOP
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n.as_bytes()))
        {
            sent.extend_from_slice(line);
            sent.extend_from_slice(END);
        }
    }
    sent.extend_from_slice(END);
    sent
}

/// Reads on from `bytes` up to the end of a head: the head and what came after it. `None` when the input ends first or the head runs past [`MAX_HEAD`].
async fn read_head(input: &mut TcpStream, mut bytes: Vec<u8>) -> Option<(Vec<u8>, Vec<u8>)> {
    const END: &[u8] = b"\r\n\r\n";
    let mut chunk = [0_u8; KIB_USIZE];
    loop {
        if let Some(at) = bytes.windows(END.len()).position(|w| w == END) {
            let rest = bytes.split_off(at + END.len());
            return Some((bytes, rest));
        }
        if bytes.len() > MAX_HEAD {
            return None;
        }
        let read = input.read(&mut chunk).await.ok()?;
        let new = chunk.get(..read).filter(|new| !new.is_empty())?;
        bytes.extend_from_slice(new);
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
    let Some((head, rest)) = read_head(&mut client, Vec::new()).await else {
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
    let carried = match &destination.request {
        Some(request) => carry_request(&mut client, &mut server, request, &rest).await,
        None => carry_tunnel(&mut client, &mut server, &rest).await,
    };
    if let Err(e) = carried {
        tracing::debug!(error = %e, "a connection through the local network guard ended early");
    }
}

async fn carry_tunnel(
    client: &mut TcpStream,
    server: &mut TcpStream,
    rest: &[u8],
) -> io::Result<()> {
    client.write_all(TUNNEL_OPEN).await?;
    server.write_all(rest).await?;
    copy_bidirectional(client, server).await.map(drop)
}

/// Carries one plain request and its response. Nothing the client sends past the request's body reaches the server: it can name another host, and its cookies would go with it.
async fn carry_request(
    client: &mut TcpStream,
    server: &mut TcpStream,
    request: &PlainRequest,
    rest: &[u8],
) -> io::Result<()> {
    server.write_all(&request.head).await?;
    let held = usize::try_from(request.body_len).map_or(rest.len(), |len| len.min(rest.len()));
    server
        .write_all(rest.get(..held).unwrap_or_default())
        .await?;
    let left = request.body_len.saturating_sub(held as u64);
    tokio::io::copy(&mut (&mut *client).take(left), server).await?;
    // More than the request already came: no answer may follow it.
    if rest.len() > held {
        return Ok(());
    }
    carry_response(client, server).await
}

/// The response's final head and what came after it; `None` when the server ends first. Answers before it are not passed on: `http.client` reads some of them as final and keeps the connection, and no client here waits on one, since `Expect` is refused.
async fn final_head(server: &mut TcpStream) -> Option<(Vec<u8>, Vec<u8>)> {
    const INFORMATIONAL: &[u8] = b"1";
    const STATUS_AT: usize = "HTTP/1.1 ".len();
    let mut bytes = Vec::new();
    loop {
        let (head, after) = read_head(server, bytes).await?;
        if head.get(STATUS_AT..=STATUS_AT) != Some(INFORMATIONAL) {
            return Some((head, after));
        }
        bytes = after;
    }
}

/// Passes on the response, told to close. It ends when either side closes, and at anything more from the client: the client read this connection as still open, so whatever the server sends next would answer that request, which may be for another host.
async fn carry_response(client: &mut TcpStream, server: &mut TcpStream) -> io::Result<()> {
    let mut from_client = [0_u8; 1];
    let (head, body) = tokio::select! {
        response = final_head(server) => match response {
            Some(response) => response,
            None => return client.write_all(UNREACHABLE).await,
        },
        _ = client.read(&mut from_client) => return Ok(()),
    };
    client.write_all(&closing_response(&head)).await?;
    client.write_all(&body).await?;
    let mut from_server = vec![0_u8; RELAY_BUFFER];
    loop {
        tokio::select! {
            read = server.read(&mut from_server) => match read? {
                0 => return Ok(()),
                n => client.write_all(from_server.get(..n).unwrap_or_default()).await?,
            },
            _ = client.read(&mut from_client) => return Ok(()),
        }
    }
}

/// Takes the outcome of answering: a client that has gone is not told, which changes nothing.
fn client_may_have_gone(told: &io::Result<()>) {
    let (Ok(()) | Err(_)) = told;
}

#[cfg(test)]
mod tests {
    use wirecatch_test_support::PATIENCE;

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
            let (head, _) = read_head(&mut peer, Vec::new()).await.unwrap();
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

    /// A server that keeps its connection open never gets a later request sent down this one, which could name another host and carry its cookies.
    #[tokio::test]
    async fn a_keep_alive_server_sees_one_request_only() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let later = tokio::spawn(async move {
            let (mut peer, _) = server.accept().await.unwrap();
            let (_, _) = read_head(&mut peer, Vec::new()).await.unwrap();
            peer.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\nKeep-Alive: timeout=60\r\n\r\nok",
            )
            .await
            .unwrap();
            let mut rest = Vec::new();
            peer.read_to_end(&mut rest).await.unwrap();
            rest
        });
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        client
            .write_all(
                format!("GET http://{target}/a HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let (response, mut body) = read_head(&mut client, Vec::new()).await.unwrap();
        let response = String::from_utf8(response).unwrap();
        assert_eq!(
            response,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\n"
        );
        while body.len() < 2 {
            let mut more = [0_u8; 2];
            let n = client.read(&mut more).await.unwrap();
            body.extend_from_slice(more.get(..n).unwrap());
        }
        assert_eq!(body, b"ok");
        client
            .write_all(b"GET http://other.example/ HTTP/1.1\r\nCookie: session=SECRET\r\n\r\n")
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        assert_eq!(later.await.unwrap(), Vec::<u8>::new());
    }

    /// A request's body goes on to its stated length, and what follows it does not.
    #[tokio::test]
    async fn only_a_requests_own_body_goes_on() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let heard = tokio::spawn(async move {
            let (mut peer, _) = server.accept().await.unwrap();
            let (_, mut body) = read_head(&mut peer, Vec::new()).await.unwrap();
            peer.read_to_end(&mut body).await.unwrap();
            body
        });
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        client
            .write_all(
                format!(
                    "POST http://{target}/ HTTP/1.1\r\nContent-Length: 4\r\n\r\nbodyGET http://other.example/ HTTP/1.1\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let heard = tokio::time::timeout(PATIENCE, heard).await.unwrap();
        assert_eq!(heard.unwrap(), b"body");
        drop(client);
    }

    #[tokio::test]
    async fn a_body_of_unstated_length_goes_nowhere() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let said = answer(
            &guard,
            "POST http://127.0.0.1:9/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await;
        assert!(said.starts_with("HTTP/1.1 400"), "{said}");
        let said = answer(
            &guard,
            "POST http://127.0.0.1:9/ HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 1\r\n\r\n",
        )
        .await;
        assert!(said.starts_with("HTTP/1.1 400"), "{said}");
    }

    #[test]
    fn the_target_goes_on_as_the_client_wrote_it() {
        for (target, sent) in [
            ("http://a.example/p%20q?x=a%2fb&y=|", "/p%20q?x=a%2fb&y=|"),
            ("http://a.example?q=1", "/?q=1"),
            ("http://a.example", "/"),
        ] {
            assert_eq!(origin_form(target).as_deref(), Some(sent), "{target}");
        }
    }

    /// A server cannot keep the client reading: an early answer does not reach it, and anything more from the client ends the connection before a later answer could pass for its next request.
    #[tokio::test]
    async fn an_early_answer_cannot_hold_the_connection() {
        let guard = Guard::keeping_off(nothing_kept_off).await.unwrap();
        let server = local_server().await;
        let target = server.local_addr().unwrap();
        let (hinted, was_hinted) = tokio::sync::oneshot::channel();
        let ended = tokio::spawn(async move {
            let (mut peer, _) = server.accept().await.unwrap();
            read_head(&mut peer, Vec::new()).await.unwrap();
            peer.write_all(b"HTTP/1.1 103 Early Hints\r\n\r\n")
                .await
                .unwrap();
            hinted.send(()).unwrap();
            let mut later = Vec::new();
            client_may_have_gone(&peer.read_to_end(&mut later).await.map(drop));
            let answered = peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            client_may_have_gone(&answered.await);
            later
        });
        let mut client = TcpStream::connect(guard.address).await.unwrap();
        client
            .write_all(format!("GET http://{target}/a HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        was_hinted.await.unwrap();
        client
            .write_all(b"GET http://other.example/ HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let later = tokio::time::timeout(PATIENCE, ended)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(later, Vec::<u8>::new());
        let mut said = Vec::new();
        // Closed with bytes unread, the connection may read as reset.
        client_may_have_gone(&client.read_to_end(&mut said).await.map(drop));
        assert_eq!(said, Vec::<u8>::new());
    }

    /// `Connection: close` comes first, ahead of a line that would stop a client reading.
    #[test]
    fn a_response_is_told_to_close_before_any_other_line() {
        let sent = closing_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nno colon here\r\nKeep-Alive: x\r\n\r\n",
        );
        assert_eq!(
            sent,
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\nno colon here\r\n\r\n"
        );
    }
}
