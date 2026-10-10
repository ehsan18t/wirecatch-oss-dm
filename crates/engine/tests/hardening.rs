//! Behavior toward servers: credentials and redirects, `Retry-After`, and
//! servers that limit how many connections one client may open.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use common::{Busy, Config, Refuse, TestServer, test_data};
use wirecatch_engine::header::{
    AUTHORIZATION, COOKIE, HeaderValue, PROXY_AUTHORIZATION, REFERER, USER_AGENT,
};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, download};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-hd-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn request(server: &TestServer, path: &str, dir: &Path, connections: usize) -> DownloadRequest {
    let mut req = DownloadRequest::new(server.url(path), dir);
    req.options.connections = connections;
    req.options.min_split = 128 * KIB as u64;
    req.options.buffer_size = 64 * KIB;
    req.options.stall_timeout = Duration::from_secs(5);
    req.options.checkpoint_interval = Duration::from_millis(100);
    req
}

async fn run(req: DownloadRequest) -> Outcome {
    download(req, Arc::new(Progress::default()), CancellationToken::new())
        .await
        .expect("download failed")
}

fn completed_path(outcome: Outcome) -> PathBuf {
    match outcome {
        Outcome::Completed { path, .. } => path,
        other => panic!("not completed: {other:?}"),
    }
}

/// Headers a browser hands over: credentials plus harmless context.
fn with_browser_headers(req: &mut DownloadRequest) {
    let headers = &mut req.options.headers;
    headers.insert(COOKIE, HeaderValue::from_static("session=secret"));
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
    headers.insert(
        PROXY_AUTHORIZATION,
        HeaderValue::from_static("Basic c2VjcmV0"),
    );
    headers.insert("x-api-key", HeaderValue::from_static("secret"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://site.example/page"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("TestBrowser/1.0"));
}

const CREDENTIALS: [&str; 4] = [
    "cookie",
    "authorization",
    "proxy-authorization",
    "x-api-key",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credentials_never_follow_a_redirect_to_another_origin() {
    // Throttled so several workers really do send their own requests.
    let target = TestServer::start(
        test_data(2 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB as u64),
            ..Default::default()
        },
    )
    .await;
    // Same host, different port: a different origin.
    let origin = TestServer::start(
        Vec::new(),
        Config {
            redirect: Some(("/start".into(), target.url("files/data.bin").to_string())),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("cross-origin");
    let mut req = request(&origin, "start", &dir, 4);
    with_browser_headers(&mut req);

    let path = completed_path(run(req).await);
    assert_eq!(std::fs::read(path).unwrap(), target.data);

    // The origin the headers were given for received all of them.
    let first = origin.stats.seen();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].header("cookie"), Some("session=secret"));
    assert_eq!(first[0].header("authorization"), Some("Bearer secret"));

    // The other origin got none of the credentials: not on the redirected first
    // request, and not on any worker request sent straight to it afterwards.
    let seen = target.stats.seen();
    assert!(
        seen.iter().any(|s| s.header("range") == Some("bytes=0-")),
        "the redirected first request is missing"
    );
    assert!(
        seen.iter()
            .filter(|s| s.header("range") != Some("bytes=0-"))
            .count()
            >= 1,
        "expected worker requests too"
    );
    for request in &seen {
        for name in CREDENTIALS {
            assert!(
                request.headers.get(name).is_none(),
                "{name} reached the other origin: {request:?}"
            );
        }
        assert_eq!(request.header("referer"), Some("https://site.example/page"));
        assert_eq!(request.header("user-agent"), Some("TestBrowser/1.0"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_origin_redirect_keeps_credentials() {
    let server = TestServer::start(
        test_data(2 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB as u64),
            redirect: Some(("/old".into(), "/files/new.bin".into())),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("same-origin");
    let mut req = request(&server, "old", &dir, 4);
    with_browser_headers(&mut req);

    let path = completed_path(run(req).await);
    assert_eq!(path, dir.join("new.bin"), "named after the redirect target");
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen[0].path, "/old");
    assert!(seen.iter().filter(|s| s.path == "/files/new.bin").count() > 1);
    for request in &seen {
        for name in CREDENTIALS {
            assert!(
                request.headers.get(name).is_some(),
                "{name} missing on {}",
                request.path
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_in_seconds_delays_the_first_request() {
    let server = TestServer::start(
        test_data(256 * KIB),
        Config {
            ranges: true,
            busy: Some(Busy {
                requests: 0..1,
                status: 503,
                retry_after: Some("1".into()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry-after-secs");

    let path = completed_path(run(request(&server, "busy.bin", &dir, 4)).await);
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen[0].status, 503);
    let waited = seen[1].at - seen[0].at;
    // The usual first backoff is under 0.75 s, so this proves the header was used.
    assert!(waited >= Duration::from_secs(1), "retried after {waited:?}");
    assert!(waited < Duration::from_secs(3), "retried after {waited:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_as_a_date_is_not_retried_before_that_time() {
    // HTTP dates have whole seconds, so this lands 1 to 2 seconds from now.
    let not_before = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(2));
    let server = TestServer::start(
        test_data(256 * KIB),
        Config {
            ranges: true,
            busy: Some(Busy {
                requests: 0..1,
                status: 429,
                retry_after: Some(not_before.clone()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry-after-date");

    let path = completed_path(run(request(&server, "busy.bin", &dir, 4)).await);
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen[0].status, 429);
    let deadline = httpdate::parse_http_date(&not_before).unwrap();
    assert!(
        seen[1].wall >= deadline,
        "retried {:?} before {not_before}",
        deadline.duration_since(seen[1].wall).unwrap_or_default()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_is_honored_in_the_middle_of_a_download() {
    // The first response breaks off; the retry for the rest is answered with
    // 503 and Retry-After. With one connection there is no one else to take
    // over, so the connection must wait and try again. Throttled so the client
    // has read the first response's headers before the break (on Windows a reset
    // discards whatever the client has not read yet).
    let server = TestServer::start(
        test_data(MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(2 * MIB as u64),
            drop_first: Some((1, 256 * KIB as u64)),
            busy: Some(Busy {
                requests: 1..2,
                status: 503,
                retry_after: Some("1".into()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry-after-mid");

    let path = completed_path(run(request(&server, "busy.bin", &dir, 1)).await);
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen[1].status, 503);
    assert_ne!(seen[1].header("range"), Some("bytes=0-"));
    assert_eq!(seen[2].header("range"), seen[1].header("range"));
    let waited = seen[2].at - seen[1].at;
    assert!(waited >= Duration::from_secs(1), "retried after {waited:?}");
}

/// Eight connections against a server that serves two at a time.
async fn completes_within_the_server_connection_limit(refuse: Refuse, test: &str) {
    const ALLOWED: usize = 2;
    const CONNECTIONS: usize = 8;
    // Long enough (about 2 s) that connections that kept retrying instead of
    // stepping back would be refused a third time.
    let server = TestServer::start(
        test_data(6 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(2 * MIB as u64),
            max_connections: Some((ALLOWED, refuse)),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir(test);

    let started = Instant::now();
    let path = completed_path(run(request(&server, "limited.bin", &dir, CONNECTIONS)).await);
    let took = started.elapsed();

    assert_eq!(std::fs::read(path).unwrap(), server.data);
    let max = server.stats.max_concurrent.load(Ordering::SeqCst);
    let refused = server.stats.refused.load(Ordering::SeqCst);
    assert!(max <= ALLOWED, "{max} connections served at once");
    assert!(refused >= 1, "the limit was never reached");
    // Each extra connection is refused twice, then steps back for good. A little
    // slack covers a working connection that asks again a moment before the
    // server has counted its previous response as finished.
    let surplus = CONNECTIONS - ALLOWED;
    assert!(
        refused <= 2 * surplus + 3,
        "{refused} refused requests: connections kept retrying"
    );
    assert!(took < Duration::from_secs(8), "took {took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_get_503() {
    completes_within_the_server_connection_limit(Refuse::Status(503), "limit-503").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_get_429() {
    completes_within_the_server_connection_limit(Refuse::Status(429), "limit-429").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_are_reset() {
    completes_within_the_server_connection_limit(Refuse::Reset, "limit-reset").await;
}
