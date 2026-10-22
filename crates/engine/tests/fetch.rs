//! `Fetcher`: single requests for playlists and segments, saved to memory or a file, with retries, speed limits and the local network guard.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use wirecatch_engine::header::{AUTHORIZATION, COOKIE, HeaderValue, REFERER};
use wirecatch_engine::units::MIB;
use wirecatch_engine::{
    CancellationToken, DownloadOptions, Error, Fetcher, PrivateNetwork, RateLimiter, Url,
};
use wirecatch_test_support::http::{Busy, Config, TestServer};
use wirecatch_test_support::{PATIENCE, private_dir, test_data};

fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options.connections = 2;
    options
}

async fn fetcher(url: &Url, options: &DownloadOptions) -> wirecatch_engine::Result<Fetcher> {
    Fetcher::new(url, options, CancellationToken::new()).await
}

#[tokio::test]
async fn gets_whole_resources_and_ranges() {
    let data = test_data(50_000);
    let server = TestServer::start(
        data.clone(),
        Config {
            ranges: true,
            content_type: Some("application/vnd.apple.mpegurl".into()),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("a/index.m3u8"), &options())
        .await
        .unwrap();

    let whole = f.get(f.url(), None, MIB).await.unwrap();
    assert_eq!(whole.body[..], data[..]);
    assert_eq!(whole.url, server.url("a/index.m3u8"));
    assert_eq!(
        whole.content_type.as_deref(),
        Some("application/vnd.apple.mpegurl")
    );

    let part = f.get(f.url(), Some(1000..1100), MIB).await.unwrap();
    assert_eq!(part.body[..], data[1000..1100]);
    let seen = server.stats.seen();
    assert_eq!(
        seen.last().unwrap().header("range"),
        Some("bytes=1000-1099")
    );
}

#[tokio::test]
async fn a_server_without_ranges_still_yields_the_right_bytes() {
    let data = test_data(30_000);
    let server = TestServer::start(data.clone(), Config::default()).await;
    let f = fetcher(&server.url("seg.ts"), &options()).await.unwrap();
    let part = f.get(f.url(), Some(20_000..20_010), MIB).await.unwrap();
    assert_eq!(part.body[..], data[20_000..20_010]);

    let dir = private_dir("noranges").unwrap();
    let path = dir.join("out.bin");
    let n = f
        .save(f.url(), Some(5..29_000), &path, |_| {})
        .await
        .unwrap();
    assert_eq!(n, 28_995);
    assert_eq!(std::fs::read(&path).unwrap()[..], data[5..29_000]);
    dir.remove().unwrap();
}

#[tokio::test]
async fn answers_over_the_limit_are_refused() {
    let server = TestServer::start(test_data(10_000), Config::default()).await;
    let f = fetcher(&server.url("key"), &options()).await.unwrap();
    let error = f.get(f.url(), None, 16).await.unwrap_err();
    assert!(
        matches!(error, Error::TooLarge { limit: 16, .. }),
        "{error}"
    );
    // Also without a Content-Length to check up front.
    server.configure(|c| c.omit_length = true);
    let error = f.get(f.url(), None, 16).await.unwrap_err();
    assert!(
        matches!(error, Error::TooLarge { limit: 16, .. }),
        "{error}"
    );
}

#[tokio::test]
async fn save_continues_from_the_byte_it_reached() {
    let data = test_data(400_000);
    let server = TestServer::start(
        data.clone(),
        Config {
            ranges: true,
            // The first answer ends cleanly after 100 000 bytes.
            short_first: Some(100_000),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("big.ts"), &options()).await.unwrap();
    let dir = private_dir("short").unwrap();
    let path = dir.join("out.bin");
    let counted = AtomicU64::new(0);
    let n = f
        .save(f.url(), Some(0..400_000), &path, |b| {
            counted.fetch_add(b, Ordering::Relaxed);
        })
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(counted.load(Ordering::Relaxed), data.len() as u64);
    assert_eq!(std::fs::read(&path).unwrap(), data);

    let ranges: Vec<_> = server
        .stats
        .seen()
        .iter()
        .map(|s| s.header("range").map(str::to_string))
        .collect();
    assert_eq!(
        ranges,
        [
            Some("bytes=0-399999".to_string()),
            Some("bytes=100000-399999".to_string())
        ]
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn save_survives_a_reset_without_counting_bytes_twice() {
    let data = test_data(400_000);
    let server = TestServer::start(
        data.clone(),
        Config {
            ranges: true,
            drop_first: Some((1, 100_000)),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("big.ts"), &options()).await.unwrap();
    let dir = private_dir("drop").unwrap();
    let path = dir.join("out.bin");
    let counted = AtomicU64::new(0);
    let n = f
        .save(f.url(), None, &path, |b| {
            counted.fetch_add(b, Ordering::Relaxed);
        })
        .await
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(counted.load(Ordering::Relaxed), data.len() as u64);
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(server.stats.requests.load(Ordering::SeqCst) >= 2);
    dir.remove().unwrap();
}

#[tokio::test]
async fn credentials_stay_with_their_origin() {
    let data = test_data(1000);
    let site = TestServer::start(data.clone(), Config::default()).await;
    let cdn = TestServer::start(data.clone(), Config::default()).await;
    let mut options = options();
    options
        .headers
        .insert(COOKIE, HeaderValue::from_static("session=secret"));
    options.headers.insert(
        REFERER,
        HeaderValue::from_static("http://site.example/watch?v=1"),
    );
    let manifest = Url::parse(&format!(
        "{}/index.m3u8",
        site.base.replace("http://", "http://user:pass@")
    ))
    .unwrap();
    let f = fetcher(&manifest, &options).await.unwrap();
    assert_eq!(f.url().username(), "");

    f.get(f.url(), None, MIB).await.unwrap();
    f.get(&cdn.url("seg1.ts"), None, MIB).await.unwrap();
    // Credentials written into a link for another origin are not sent either.
    let with_userinfo = Url::parse(&format!(
        "{}/seg2.ts",
        cdn.base.replace("http://", "http://x:y@")
    ))
    .unwrap();
    f.get(&with_userinfo, None, MIB).await.unwrap();

    let at_site = &site.stats.seen()[0];
    assert_eq!(at_site.header("cookie"), Some("session=secret"));
    assert!(at_site.header("authorization").is_some());
    for at_cdn in cdn.stats.seen() {
        assert_eq!(at_cdn.header(COOKIE.as_str()), None);
        assert_eq!(at_cdn.header(AUTHORIZATION.as_str()), None);
        assert_eq!(at_cdn.header("referer"), Some("http://site.example/"));
    }
}

#[tokio::test]
async fn retry_after_is_honored() {
    let server = TestServer::start(
        test_data(100),
        Config {
            busy: Some(Busy {
                requests: 0..1,
                status: 503,
                retry_after: Some("1".into()),
            }),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("k"), &options()).await.unwrap();
    let start = Instant::now();
    f.get(f.url(), None, 1000).await.unwrap();
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn errors_that_do_not_go_away_surface_after_the_retries() {
    let server = TestServer::start(
        test_data(100),
        Config {
            busy: Some(Busy {
                requests: 0..100,
                status: 500,
                retry_after: None,
            }),
            ..Config::default()
        },
    )
    .await;
    let mut options = options();
    options.max_retries = 2;
    let f = fetcher(&server.url("k"), &options).await.unwrap();
    let error = f.get(f.url(), None, 1000).await.unwrap_err();
    assert!(
        matches!(error, Error::HttpStatus { status: 500, .. }),
        "{error}"
    );
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 3);

    // A 404 is not retried at all.
    server.configure(|c| {
        c.busy = Some(Busy {
            requests: 0..1000,
            status: 404,
            retry_after: None,
        });
    });
    let before = server.stats.requests.load(Ordering::SeqCst);
    assert!(matches!(
        f.get(f.url(), None, 1000).await,
        Err(Error::HttpStatus { status: 404, .. })
    ));
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), before + 1);
}

#[tokio::test]
async fn cancelling_and_stalls_end_requests() {
    let server = TestServer::start(
        test_data(100),
        Config {
            hang: Some(0..100),
            ..Config::default()
        },
    )
    .await;
    let cancel = CancellationToken::new();
    // No stall can end the request before the cancel, however long the machine takes to send it.
    let mut patient = options();
    patient.stall_timeout = 2 * PATIENCE;
    let f = Fetcher::new(&server.url("k"), &patient, cancel.clone())
        .await
        .unwrap();
    let task = {
        let f = f.clone();
        tokio::spawn(async move { f.get(f.url(), None, 1000).await })
    };
    server.wait_for_request("k", PATIENCE).await;
    cancel.cancel();
    // The server never answers, so only the cancel can end the request.
    let result = tokio::time::timeout(PATIENCE, task)
        .await
        .expect("the cancelled request ended")
        .unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");

    let mut options = options();
    options.stall_timeout = Duration::from_millis(300);
    options.max_retries = 0;
    let f = fetcher(&server.url("k"), &options).await.unwrap();
    let stalled = f.get(f.url(), None, 1000).await.unwrap_err();
    // A timeout under a second is said as it is, not as "0 seconds".
    let text = stalled.to_string();
    assert!(text.contains("300ms") && !text.contains(" 0 "), "{text}");
}

#[tokio::test]
async fn non_http_addresses_are_refused() {
    let server = TestServer::start(test_data(10), Config::default()).await;
    let f = fetcher(&server.url("k"), &options()).await.unwrap();
    let key = Url::parse("skd://some-drm-key").unwrap();
    assert!(matches!(
        f.get(&key, None, 16).await,
        Err(Error::InvalidUrl(_))
    ));
    assert!(matches!(
        Fetcher::new(
            &Url::parse("file:///etc/passwd").unwrap(),
            &options(),
            CancellationToken::new()
        )
        .await,
        Err(Error::InvalidUrl(_))
    ));
}

#[tokio::test(start_paused = false)]
async fn rate_limits_apply() {
    let data = test_data(200_000);
    let server = TestServer::start(data, Config::default()).await;
    let mut options = options();
    options.rate_limiters = vec![Arc::new(RateLimiter::new(Some(400_000)))];
    let f = fetcher(&server.url("k"), &options).await.unwrap();
    let start = Instant::now();
    f.get(f.url(), None, MIB).await.unwrap();
    // 200 kB at 400 kB/s is half a second, less what the limiter saved up
    // while the fetcher was being made: at most a quarter of a second's worth,
    // which a slow machine always has time to save. A millisecond is left for
    // rounding.
    let least = Duration::from_millis(250 - 1);
    let took = start.elapsed();
    assert!(took >= least, "took {took:?}");
}

async fn blocking_fetcher(url: &Url, mode: PrivateNetwork) -> wirecatch_engine::Result<Fetcher> {
    Fetcher::with_private_network(url, &options(), CancellationToken::new(), mode).await
}

#[tokio::test]
async fn the_local_network_is_blocked_when_asked() {
    let site = TestServer::start(test_data(100), Config::default()).await;
    let device = TestServer::start(test_data(100), Config::default()).await;
    // The test servers are on 127.0.0.1, so `Block` stands in for a site on
    // the internet: its own origin stays reachable, everything local is not.
    let f = blocking_fetcher(&site.url("index.m3u8"), PrivateNetwork::Block)
        .await
        .unwrap();
    assert!(f.blocks_private_network());
    f.get(f.url(), None, 1000).await.unwrap();

    let device_port = device.url("x").port().unwrap();
    for target in [
        device.url("seg.ts").to_string(),
        format!("http://localhost:{device_port}/seg.ts"),
        "http://10.0.0.1/seg.ts".to_string(),
        "http://192.168.1.1/key".to_string(),
        "http://169.254.169.254/latest/meta-data".to_string(),
        "http://[::1]/seg.ts".to_string(),
        "http://[fe80::1]/seg.ts".to_string(),
        "http://0.0.0.0/seg.ts".to_string(),
    ] {
        let error = f
            .get(&Url::parse(&target).unwrap(), None, 1000)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::PrivateNetwork { .. }),
            "{target}: {error}"
        );
        assert!(
            error
                .to_string()
                .ends_with("points to a device on your own network, which is blocked for safety"),
            "{error}"
        );
        assert!(!error.is_retryable(), "refused without retries");
    }
    assert!(
        device.stats.seen().is_empty(),
        "the device was never contacted"
    );
}

#[tokio::test]
async fn a_redirect_into_the_local_network_is_blocked() {
    let device = TestServer::start(test_data(100), Config::default()).await;
    let site = TestServer::start(
        test_data(100),
        Config {
            redirect: Some(("/seg.ts".into(), device.url("secret").to_string())),
            ..Config::default()
        },
    )
    .await;
    let f = blocking_fetcher(&site.url("index.m3u8"), PrivateNetwork::Block)
        .await
        .unwrap();
    let dir = private_dir("fetch-redirect").unwrap();
    let path = dir.path().join("seg.ts");
    let error = f
        .save(&site.url("seg.ts"), None, &path, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(error, Error::PrivateNetwork { .. }), "{error}");
    assert_eq!(site.stats.seen().len(), 1, "the site answered the redirect");
    assert!(device.stats.seen().is_empty(), "the hop was never sent");
    dir.remove().unwrap();
}

#[tokio::test]
async fn the_local_network_is_allowed_for_a_local_start_or_when_asked() {
    let site = TestServer::start(test_data(100), Config::default()).await;
    let device = TestServer::start(test_data(100), Config::default()).await;
    // A playlist that is itself on the local network (a media server at home)
    // may point to other local addresses.
    let auto = fetcher(&site.url("index.m3u8"), &options()).await.unwrap();
    assert!(!auto.blocks_private_network());
    auto.get(&device.url("a.ts"), None, 1000).await.unwrap();
    let allow = blocking_fetcher(&site.url("index.m3u8"), PrivateNetwork::Allow)
        .await
        .unwrap();
    allow.get(&device.url("b.ts"), None, 1000).await.unwrap();
    assert_eq!(device.stats.seen().len(), 2);

    // A public starting address turns blocking on by itself. Nothing is
    // fetched from it: the decision is made when the fetcher is created.
    let public = Url::parse("http://93.184.215.14/index.m3u8").unwrap();
    let auto = fetcher(&public, &options()).await.unwrap();
    assert!(auto.blocks_private_network());
}

/// The resource is replaced on the server between two requests of one
/// transfer, so the answer that continues it carries another `ETag`: the bytes
/// of the two versions must not be joined.
#[tokio::test]
async fn a_resource_replaced_between_requests_is_reported_as_changed() {
    let server = TestServer::start(
        test_data(200_000),
        Config {
            ranges: true,
            etag: Some("\"v1\"".into()),
            // The first answer ends early, so the fetcher asks again.
            short_first: Some(50_000),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("seg.ts"), &options()).await.unwrap();
    server.hold();
    let get = tokio::spawn({
        let f = f.clone();
        async move { f.get(f.url(), Some(0..200_000), MIB).await }
    });
    server.wait_for_request("seg.ts", PATIENCE).await;
    server.configure(|config| config.etag = Some("\"v2\"".into()));
    server.release();
    let result = get.await.unwrap().map(|fetched| fetched.body.len());
    assert!(matches!(result, Err(Error::RemoteChanged)), "{result:?}");
}

/// The same with a server that ignores ranges: the fetcher asks for the rest
/// and gets the whole resource again, whose start it skips, so it must be the
/// same resource.
#[tokio::test]
async fn a_resource_replaced_before_a_whole_answer_is_asked_again_is_reported_as_changed() {
    let server = TestServer::start(
        test_data(2_000_000),
        Config {
            etag: Some("\"v1\"".into()),
            drop_first: Some((1, 500_000)),
            ..Config::default()
        },
    )
    .await;
    let f = fetcher(&server.url("seg.ts"), &options()).await.unwrap();
    server.hold();
    let get = tokio::spawn({
        let f = f.clone();
        async move { f.get(f.url(), None, 4 * MIB).await }
    });
    server.wait_for_request("seg.ts", PATIENCE).await;
    server.configure(|config| config.etag = Some("\"v2\"".into()));
    server.release();
    let result = get.await.unwrap().map(|fetched| fetched.body.len());
    assert!(matches!(result, Err(Error::RemoteChanged)), "{result:?}");
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 2);
}

/// A name the caller saw as public may resolve to a local address when the
/// engine looks it up (DNS rebinding); `localhost` stands in for such a name.
/// With the local network blocked, the starting host is held to the same rule
/// as every other name, not let through because its own lookup was local.
#[tokio::test]
async fn a_start_name_that_resolves_to_the_local_network_is_blocked_when_asked() {
    let site = TestServer::start(test_data(100), Config::default()).await;
    let port = site.url("x").port().unwrap();
    let url = Url::parse(&format!("http://localhost:{port}/index.m3u8")).unwrap();
    let error = match blocking_fetcher(&url, PrivateNetwork::Block).await {
        Err(error) => error,
        Ok(f) => match f.get(f.url(), None, 1000).await {
            Err(error) => error,
            Ok(_) => panic!("the local address was reached"),
        },
    };
    assert!(matches!(error, Error::PrivateNetwork { .. }), "{error}");
    assert!(site.stats.seen().is_empty(), "the site was contacted");
}
