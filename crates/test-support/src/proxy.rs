//! Proxies for tests: a SOCKS5 proxy (RFC 1928) that forwards connections,
//! optionally behind a user name and password (RFC 1929), and an HTTP proxy
//! that turns every request away with `407`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// How the SOCKS5 proxy behaves.
#[derive(Clone, Debug, Default)]
pub struct SocksConfig {
    /// The user name and password it asks for; `None` lets anyone in.
    pub login: Option<(String, String)>,
}

/// A SOCKS5 proxy on a local port that forwards `CONNECT` requests to the
/// address or name asked for, and remembers each destination.
pub struct SocksServer {
    /// The port it listens on, on 127.0.0.1.
    pub port: u16,
    destinations: Arc<Mutex<Vec<String>>>,
}

impl SocksServer {
    /// Starts the proxy.
    pub async fn start(config: SocksConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let destinations = Arc::new(Mutex::new(Vec::new()));
        let seen = destinations.clone();
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let config = config.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    // A client that gives up half way is not the test's concern.
                    let (Ok(()) | Err(_)) = serve(client, &config, &seen).await;
                });
            }
        });
        Self { port, destinations }
    }

    /// The proxy's URL with `scheme` (`socks5` or `socks5h`) and `userinfo`
    /// (`""` or `user:password@`).
    #[must_use]
    pub fn url(&self, scheme: &str, userinfo: &str) -> String {
        format!("{scheme}://{userinfo}127.0.0.1:{}", self.port)
    }

    /// Every destination a client connected to, as `host:port`, in order.
    #[must_use]
    pub fn destinations(&self) -> Vec<String> {
        self.destinations.lock().unwrap().clone()
    }
}

async fn serve(
    mut client: TcpStream,
    config: &SocksConfig,
    seen: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut head = [0; 2];
    client.read_exact(&mut head).await?;
    let mut methods = vec![0; usize::from(head[1])];
    client.read_exact(&mut methods).await?;
    let wanted = if config.login.is_some() { 2 } else { 0 };
    if !methods.contains(&wanted) {
        client.write_all(&[5, 0xFF]).await?;
        return Ok(());
    }
    client.write_all(&[5, wanted]).await?;
    if let Some((user, password)) = &config.login {
        let mut version_and_length = [0; 2];
        client.read_exact(&mut version_and_length).await?;
        let mut given_user = vec![0; usize::from(version_and_length[1])];
        client.read_exact(&mut given_user).await?;
        let mut length = [0; 1];
        client.read_exact(&mut length).await?;
        let mut given_password = vec![0; usize::from(length[0])];
        client.read_exact(&mut given_password).await?;
        let accepted = given_user == user.as_bytes() && given_password == password.as_bytes();
        client.write_all(&[1, u8::from(!accepted)]).await?;
        if !accepted {
            return Ok(());
        }
    }

    let mut request = [0; 4];
    client.read_exact(&mut request).await?;
    let host = match request[3] {
        1 => {
            let mut ip = [0; 4];
            client.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        4 => {
            let mut ip = [0; 16];
            client.read_exact(&mut ip).await?;
            format!("[{}]", std::net::Ipv6Addr::from(ip))
        }
        _ => {
            let mut length = [0; 1];
            client.read_exact(&mut length).await?;
            let mut name = vec![0; usize::from(length[0])];
            client.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
    };
    let mut port = [0; 2];
    client.read_exact(&mut port).await?;
    let destination = format!("{host}:{}", u16::from_be_bytes(port));
    seen.lock().unwrap().push(destination.clone());
    let Ok(mut server) = TcpStream::connect(destination.as_str()).await else {
        // Connection refused.
        client.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    let bound = server
        .local_addr()
        .unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0)));
    let mut reply = vec![5, 0, 0];
    match bound {
        SocketAddr::V4(addr) => {
            reply.push(1);
            reply.extend_from_slice(&addr.ip().octets());
        }
        SocketAddr::V6(addr) => {
            reply.push(4);
            reply.extend_from_slice(&addr.ip().octets());
        }
    }
    reply.extend_from_slice(&bound.port().to_be_bytes());
    client.write_all(&reply).await?;
    tokio::io::copy_bidirectional(&mut client, &mut server).await?;
    Ok(())
}

/// An HTTP proxy that answers every request, `CONNECT` included, with `407
/// Proxy Authentication Required`.
pub struct LoginWall {
    /// The port it listens on, on 127.0.0.1.
    pub port: u16,
}

impl LoginWall {
    /// Starts the proxy.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (Ok(()) | Err(_)) = turn_away(client).await;
                });
            }
        });
        Self { port }
    }

    /// The proxy's URL.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

async fn turn_away(client: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 2 {
        line.clear();
    }
    reader
        .get_mut()
        .write_all(
            b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
}
