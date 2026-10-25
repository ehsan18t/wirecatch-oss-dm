//! The Test connection button: a request through each proxy a setting names (D71.5).

use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, Error as EngineError, Fetcher, ManualProxies, PrivateNetwork, ProxyChoice,
    Url,
};
use wirecatch_protocol::proxy::ProxyTestResult;

/// Where the Test connection button sends its request: the project's own host, which the app
/// already reaches for the media add-on.
pub const PROXY_TEST_URL: &str = "https://github.com/";

/// How long a test may take in all.
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Sends a request to `url` through each proxy `choice` sets, whatever its exceptions and
/// protocol, and says how it went (D71.5): the first that fails, or that they all worked. A
/// choice that sets none (System, No proxy) sends one request as it would go.
pub(crate) async fn test(
    choice: ProxyChoice,
    url: &Url,
    cancel: CancellationToken,
) -> ProxyTestResult {
    let ProxyChoice::Manual(manual) = &choice else {
        return test_one(choice, url, cancel).await;
    };
    let mut tried: Vec<&String> = Vec::new();
    for proxy in [&manual.all, &manual.http, &manual.https, &manual.ftp]
        .into_iter()
        .flatten()
    {
        if tried.contains(&proxy) {
            continue;
        }
        tried.push(proxy);
        let mut alone = ManualProxies::default();
        alone.all = Some(proxy.clone());
        let result = test_one(ProxyChoice::Manual(alone), url, cancel.clone()).await;
        if result != ProxyTestResult::Works {
            return result;
        }
    }
    ProxyTestResult::Works
}

/// Sends one request to `url` through `choice` and says how it went.
async fn test_one(choice: ProxyChoice, url: &Url, cancel: CancellationToken) -> ProxyTestResult {
    // The address is the app's own (`PROXY_TEST_URL`), never one a page gave.
    let rule = PrivateNetwork::Allow;
    let mut options = crate::headers::download_options(rule, choice);
    options.connect_timeout = Duration::from_secs(10);
    options.stall_timeout = Duration::from_secs(15);
    // The person waits on the answer: the first failure is it.
    options.max_retries = 1;
    let attempt = async {
        let fetcher = Fetcher::with_private_network(url, &options, cancel, rule).await?;
        fetcher.get(url, Some(0..1), 1).await
    };
    match tokio::time::timeout(TEST_TIMEOUT, attempt).await {
        Err(_) => ProxyTestResult::TimedOut,
        Ok(Ok(_)) => ProxyTestResult::Works,
        Ok(Err(error)) => match error {
            // An answer came back through the proxy, whatever it was.
            EngineError::HttpStatus { .. }
            | EngineError::TooLarge { .. }
            | EngineError::RangeNotHonored { .. } => ProxyTestResult::Works,
            EngineError::ProxyLoginRefused => ProxyTestResult::LoginRefused,
            EngineError::Network(e) if e.is_timeout() => ProxyTestResult::TimedOut,
            other => ProxyTestResult::Unreachable {
                reason: other.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_test_support::http::{Config, TestServer};
    use wirecatch_test_support::proxy::{LoginWall, SocksConfig, SocksServer};
    use wirecatch_test_support::silent::SilentServer;

    use super::*;

    #[tokio::test]
    async fn the_test_says_whether_the_proxy_works() {
        let site = TestServer::start(b"hello".to_vec(), Config::default()).await;
        let url = site.url("/");
        let through = |proxy: &str| {
            let mut manual = ManualProxies::default();
            manual.all = Some(proxy.to_string());
            ProxyChoice::Manual(manual)
        };

        let socks = SocksServer::start(SocksConfig::default()).await;
        let works = test(
            through(&socks.url("socks5", "")),
            &url,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(works, ProxyTestResult::Works);
        assert_ne!(socks.destinations(), Vec::<String>::new());

        let wall = LoginWall::start().await;
        let refused = test(through(&wall.url()), &url, CancellationToken::new()).await;
        assert_eq!(refused, ProxyTestResult::LoginRefused);

        let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let missing = test(
            through(&format!("http://{closed}")),
            &url,
            CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(missing, ProxyTestResult::Unreachable { .. }),
            "{missing:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_proxy_that_never_answers_times_out() {
        let silent = SilentServer::start().await;
        let mut manual = ManualProxies::default();
        let address = silent.url("/");
        manual.all = Some(format!(
            "http://{}:{}",
            address.host_str().unwrap(),
            address.port().unwrap()
        ));
        let url = Url::parse("http://files.example/").unwrap();
        let result = test(ProxyChoice::Manual(manual), &url, CancellationToken::new()).await;
        assert_eq!(result, ProxyTestResult::TimedOut);
    }

    #[tokio::test]
    async fn the_test_goes_through_every_proxy_set_whatever_the_exceptions() {
        let site = TestServer::start(b"hello".to_vec(), Config::default()).await;
        let url = site.url("/");
        let wall = LoginWall::start().await;
        let socks = SocksServer::start(SocksConfig::default()).await;

        // The exceptions name the test's own host: without them the proxy would not be tried.
        let mut excepted = ManualProxies::default();
        excepted.all = Some(wall.url());
        excepted.exceptions = "127.0.0.1".into();
        let result = test(
            ProxyChoice::Manual(excepted),
            &url,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, ProxyTestResult::LoginRefused);

        // A proxy only for another protocol is tried too.
        let mut per_protocol = ManualProxies::default();
        per_protocol.https = Some(socks.url("socks5", ""));
        per_protocol.ftp = Some(wall.url());
        let result = test(
            ProxyChoice::Manual(per_protocol),
            &url,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, ProxyTestResult::LoginRefused);
    }
}
