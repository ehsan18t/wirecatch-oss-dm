//! The private network guard with an HTTP proxy set in the environment. A
//! test binary of its own, with a single test, because it changes the process
//! environment.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wirecatch_engine::{CancellationToken, DownloadOptions, Error, Fetcher, PrivateNetwork, Url};

/// A forward proxy on this computer that answers every request with `ok` and
/// keeps each request line.
async fn start_proxy() -> std::io::Result<(u16, Arc<Mutex<Vec<String>>>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut len = 0;
                while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf[len..]).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => len += n,
                    }
                }
                let text = String::from_utf8_lossy(&buf[..len]);
                let line = text.lines().next().unwrap_or_default().to_string();
                log.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(line);
                let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
                if let Err(error) = socket.write_all(answer).await {
                    // The client hung up first: its own request reports that, not the proxy.
                    drop(error);
                }
            });
        }
    });
    Ok((port, seen))
}

#[tokio::test(flavor = "current_thread")]
async fn the_guard_checks_targets_but_trusts_the_proxy_the_user_set() {
    let (port, seen) = start_proxy().await.unwrap();
    // A proxy named by a host that resolves to this computer, as a proxy on
    // the local network (`proxy.corp` at 10.0.0.5) would.
    let proxy = format!("http://localhost:{port}");
    for variable in ["HTTP_PROXY", "http_proxy"] {
        // SAFETY: this test binary runs this one test, on one thread, so
        // nothing else reads the environment meanwhile.
        unsafe { std::env::set_var(variable, &proxy) };
    }

    // A public address, so the guard is on and no name has to be looked up.
    let start = Url::parse("http://93.184.215.14/stream/index.m3u8").unwrap();
    let mut options = DownloadOptions::default();
    options.max_retries = 0;
    options.stall_timeout = Duration::from_secs(5);
    let fetcher = Fetcher::with_private_network(
        &start,
        &options,
        CancellationToken::new(),
        PrivateNetwork::Block,
    )
    .await
    .unwrap();
    assert!(fetcher.blocks_private_network());

    // The request reaches the proxy although the proxy's name is local.
    let answer = fetcher.get(&start, None, 1024).await.unwrap();
    assert_eq!(&answer.body[..], b"ok");
    assert_eq!(
        *seen.lock().unwrap(),
        ["GET http://93.184.215.14/stream/index.m3u8 HTTP/1.1"]
    );

    // Targets on the local network are still refused before anything is sent:
    // written as an address, inside an IPv6 one, or a name that resolves only
    // to this computer.
    for target in [
        "http://10.0.0.1/key",
        "http://[64:ff9b::a9fe:a9fe]/latest/meta-data",
        "http://100.100.100.100/",
        "http://localhost:1/key",
    ] {
        let error = fetcher
            .get(&Url::parse(target).unwrap(), None, 1024)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::PrivateNetwork { .. }),
            "{target}: {error}"
        );
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "nothing else reached the proxy"
    );

    for variable in ["HTTP_PROXY", "http_proxy"] {
        // SAFETY: as above.
        unsafe { std::env::remove_var(variable) };
    }
}
