//! The Test connection button: a request through one proxy (D71.5).

use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, Error as EngineError, Fetcher, PrivateNetwork, ProxyChoice, Url,
};
use wirecatch_protocol::proxy::ProxyTestResult;

/// Where the Test connection button sends its request: the project's own host, which the app
/// already reaches for the media add-on.
pub const PROXY_TEST_URL: &str = "https://github.com/";

/// How long a test may take in all.
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Sends one request to `url` through `choice` and says how it went (D71.5).
pub(crate) async fn test(
    choice: ProxyChoice,
    url: &Url,
    cancel: CancellationToken,
) -> ProxyTestResult {
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
    use wirecatch_engine::ManualProxies;
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
}
