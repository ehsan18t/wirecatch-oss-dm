use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wirecatch_test_support::proxy::{SocksConfig, SocksServer};

use super::*;

/// A server on this computer that sends `hello` to whoever connects.
async fn greeter() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move { socket.write_all(b"hello").await });
        }
    });
    addr
}

fn proxy(url: &str) -> Socks5 {
    Socks5::from_url(&Url::parse(url).unwrap()).unwrap()
}

async fn read_greeting(mut stream: TcpStream) -> Vec<u8> {
    let mut greeting = Vec::new();
    stream.read_to_end(&mut greeting).await.unwrap();
    greeting
}

#[test]
fn only_socks5_urls_name_a_socks5_proxy() {
    for url in ["http://proxy:8080", "socks4://proxy:1080", "https://proxy"] {
        assert_eq!(Socks5::from_url(&Url::parse(url).unwrap()), None, "{url}");
    }
    let plain = proxy("socks5://proxy.example");
    assert_eq!(plain.address(), "proxy.example:1080");
    assert!(!plain.resolves_names());
    assert!(proxy("socks5h://[::1]:9050").resolves_names());
    assert_eq!(proxy("socks5h://[::1]:9050").address(), "[::1]:9050");
}

#[test]
fn the_login_is_decoded_and_never_shown() {
    let socks = proxy("socks5://al%20ice:p%40ss@proxy:1080");
    assert_eq!(socks.login, Some(("al ice".into(), "p@ss".into())));
    let shown = format!("{socks:?}");
    assert!(!shown.contains("p@ss") && !shown.contains("ice"), "{shown}");
}

#[tokio::test]
async fn connects_through_the_proxy_to_an_address() {
    let server = greeter().await;
    let socks = SocksServer::start(SocksConfig::default()).await;
    let stream = proxy(&socks.url("socks5", ""))
        .connect(&server.into())
        .await
        .unwrap();
    assert_eq!(read_greeting(stream).await, b"hello");
    assert_eq!(socks.destinations(), vec![server.to_string()]);
}

#[tokio::test]
async fn a_name_goes_to_the_proxy_unresolved() {
    let server = greeter().await;
    let socks = SocksServer::start(SocksConfig::default()).await;
    let to = Destination::Name("localhost".into(), server.port());
    // `localhost` may resolve to ::1 first, where nothing listens; the proxy
    // tries the name as it was sent, which is what this test is about.
    let _connected = proxy(&socks.url("socks5h", "")).connect(&to).await;
    assert_eq!(
        socks.destinations(),
        vec![format!("localhost:{}", server.port())]
    );
}

#[tokio::test]
async fn logs_in_when_the_proxy_asks() {
    let server = greeter().await;
    let socks = SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await;
    let stream = proxy(&socks.url("socks5", "alice:secret@"))
        .connect(&server.into())
        .await
        .unwrap();
    assert_eq!(read_greeting(stream).await, b"hello");
}

#[tokio::test]
async fn a_missing_or_wrong_login_is_refused_as_a_login() {
    let server = greeter().await;
    let socks = SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await;
    for userinfo in ["", "alice:wrong@"] {
        let failure = proxy(&socks.url("socks5", userinfo))
            .connect(&server.into())
            .await
            .unwrap_err();
        assert!(
            matches!(failure, SocksFailure::LoginRefused),
            "{userinfo}: {failure:?}"
        );
    }
    assert_eq!(socks.destinations(), Vec::<String>::new());
}

#[tokio::test]
async fn a_destination_the_proxy_cannot_reach_says_why() {
    // A port nothing listens on: bound, then closed.
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let socks = SocksServer::start(SocksConfig::default()).await;
    let failure = proxy(&socks.url("socks5", ""))
        .connect(&closed.into())
        .await
        .unwrap_err();
    assert!(
        matches!(
            failure,
            SocksFailure::Refused("the server refused the connection")
        ),
        "{failure:?}"
    );
}

#[tokio::test]
async fn a_proxy_that_is_not_there_is_a_proxy_failure() {
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let failure = proxy(&format!("socks5://{closed}"))
        .connect(&SocketAddr::from(([127, 0, 0, 1], 21)).into())
        .await
        .unwrap_err();
    assert!(matches!(failure, SocksFailure::Proxy(_)), "{failure:?}");
}
