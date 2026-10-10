mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use common::{Busy, Config, TestServer, test_data};
use wirecatch_engine::header::{AUTHORIZATION, COOKIE, HeaderValue, REFERER};
use wirecatch_engine::{CancellationToken, DownloadOptions, Error, Fetcher, RateLimiter, Url};

fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options.connections = 2;
    options
}

async fn fetcher(url: &Url, options: &DownloadOptions) -> Fetcher {
    Fetcher::new(url, options, CancellationToken::new())
        .await
        .unwrap()
}

fn temp_file(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-fetch-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("out.bin")
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
    let f = fetcher(&server.url("a/index.m3u8"), &options()).await;

    let whole = f.get(f.url(), None, 1 << 20).await.unwrap();
    assert_eq!(whole.body[..], data[..]);
    assert_eq!(whole.url, server.url("a/index.m3u8"));
    assert_eq!(
        whole.content_type.as_deref(),
        Some("application/vnd.apple.mpegurl")
    );

    let part = f.get(f.url(), Some(1000..1100), 1 << 20).await.unwrap();
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
    let f = fetcher(&server.url("seg.ts"), &options()).await;
    let part = f.get(f.url(), Some(20_000..20_010), 1 << 20).await.unwrap();
    assert_eq!(part.body[..], data[20_000..20_010]);

    let path = temp_file("noranges");
    let n = f
        .save(f.url(), Some(5..29_000), &path, |_| {})
        .await
        .unwrap();
    assert_eq!(n, 28_995);
    assert_eq!(std::fs::read(&path).unwrap()[..], data[5..29_000]);
}

#[tokio::test]
async fn answers_over_the_limit_are_refused() {
    let server = TestServer::start(test_data(10_000), Config::default()).await;
    let f = fetcher(&server.url("key"), &options()).await;
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
    let f = fetcher(&server.url("big.ts"), &options()).await;
    let path = temp_file("short");
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
    let f = fetcher(&server.url("big.ts"), &options()).await;
    let path = temp_file("drop");
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
    let f = fetcher(&manifest, &options).await;
    assert!(f.url().username().is_empty());

    f.get(f.url(), None, 1 << 20).await.unwrap();
    f.get(&cdn.url("seg1.ts"), None, 1 << 20).await.unwrap();
    // Credentials written into a link for another origin are not sent either.
    let with_userinfo = Url::parse(&format!(
        "{}/seg2.ts",
        cdn.base.replace("http://", "http://x:y@")
    ))
    .unwrap();
    f.get(&with_userinfo, None, 1 << 20).await.unwrap();

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
    let f = fetcher(&server.url("k"), &options()).await;
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
    let f = fetcher(&server.url("k"), &options).await;
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
        })
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
    let f = Fetcher::new(&server.url("k"), &options(), cancel.clone())
        .await
        .unwrap();
    let task = {
        let f = f.clone();
        tokio::spawn(async move { f.get(f.url(), None, 1000).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start = Instant::now();
    cancel.cancel();
    let result = task.await.unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(start.elapsed() < Duration::from_secs(1));

    let mut options = options();
    options.stall_timeout = Duration::from_millis(300);
    options.max_retries = 0;
    let f = fetcher(&server.url("k"), &options).await;
    assert!(matches!(
        f.get(f.url(), None, 1000).await,
        Err(Error::Stalled(_))
    ));
}

#[tokio::test]
async fn non_http_addresses_are_refused() {
    let server = TestServer::start(test_data(10), Config::default()).await;
    let f = fetcher(&server.url("k"), &options()).await;
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
    let f = fetcher(&server.url("k"), &options).await;
    let start = Instant::now();
    f.get(f.url(), None, 1 << 20).await.unwrap();
    // 200 kB at 400 kB/s is half a second, minus the limiter's small burst.
    assert!(start.elapsed() >= Duration::from_millis(300));
}
