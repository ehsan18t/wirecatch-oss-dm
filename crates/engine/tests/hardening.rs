//! Behavior toward servers: credentials and redirects, `Retry-After`, and
//! servers that limit how many connections one client may open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use wirecatch_engine::header::{
    AUTHORIZATION, COOKIE, HeaderValue, PROXY_AUTHORIZATION, REFERER, USER_AGENT,
};
use wirecatch_engine::units::{KIB, KIB_USIZE, MIB, MIB_USIZE};
use wirecatch_engine::{CancellationToken, DownloadRequest, Error, Outcome, Progress, download};
use wirecatch_test_support::http::{Busy, Config, Refuse, TestServer};
use wirecatch_test_support::{TestResult, private_dir, test_data};

mod common;
use common::{request, run};

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
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB),
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
    let dir = private_dir("cross-origin").unwrap();
    let mut req = request(&origin, "start", &dir, 4);
    with_browser_headers(&mut req);

    let path = completed_path(run(req).await.unwrap());
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
        assert_eq!(request.header("referer"), Some("https://site.example/"));
        assert_eq!(request.header("user-agent"), Some("TestBrowser/1.0"));
    }
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_origin_redirect_keeps_credentials() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB),
            redirect: Some(("/old".into(), "/files/new.bin".into())),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("same-origin").unwrap();
    let mut req = request(&server, "old", &dir, 4);
    with_browser_headers(&mut req);

    let path = completed_path(run(req).await.unwrap());
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
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_in_seconds_delays_the_first_request() {
    let server = TestServer::start(
        test_data(256 * KIB_USIZE),
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
    let dir = private_dir("retry-after-secs").unwrap();

    let path = completed_path(run(request(&server, "busy.bin", &dir, 4)).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen[0].status, 503);
    let waited = seen[1].at - seen[0].at;
    // The usual first backoff is under 0.75 s, so this proves the header was used.
    assert!(waited >= Duration::from_secs(1), "retried after {waited:?}");
    assert!(waited < Duration::from_secs(3), "retried after {waited:?}");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_as_a_date_is_not_retried_before_that_time() {
    // HTTP dates have whole seconds, so this lands 1 to 2 seconds from now.
    let not_before = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(2));
    let server = TestServer::start(
        test_data(256 * KIB_USIZE),
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
    let dir = private_dir("retry-after-date").unwrap();

    let path = completed_path(run(request(&server, "busy.bin", &dir, 4)).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen[0].status, 429);
    let deadline = httpdate::parse_http_date(&not_before).unwrap();
    assert!(
        seen[1].wall >= deadline,
        "retried {:?} before {not_before}",
        deadline.duration_since(seen[1].wall).unwrap_or_default()
    );
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_is_honored_in_the_middle_of_a_download() {
    // The first response is short; the request for the rest is answered with
    // 503 and Retry-After. With one connection there is no one else to take
    // over, so the connection must wait and try again.
    let server = TestServer::start(
        test_data(MIB_USIZE),
        Config {
            ranges: true,
            short_first: Some(256 * KIB),
            busy: Some(Busy {
                requests: 1..2,
                status: 503,
                retry_after: Some("1".into()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("retry-after-mid").unwrap();

    let path = completed_path(run(request(&server, "busy.bin", &dir, 1)).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!(seen[1].status, 503);
    assert_ne!(seen[1].header("range"), Some("bytes=0-"));
    assert_eq!(seen[2].header("range"), seen[1].header("range"));
    let waited = seen[2].at - seen[1].at;
    assert!(waited >= Duration::from_secs(1), "retried after {waited:?}");
    dir.remove().unwrap();
}

/// Eight connections against a server that serves two at a time.
async fn completes_within_the_server_connection_limit(refuse: Refuse, test: &str) -> TestResult {
    const ALLOWED: usize = 2;
    const CONNECTIONS: usize = 8;
    // Long enough (about 4 s) that connections that kept retrying instead of
    // stepping back would be refused at least four times each (at about 0, 0.6,
    // 1.6 and 3.6 s), well above the bound below even on a slow machine.
    let server = TestServer::start(
        test_data(8 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            max_connections: Some((ALLOWED, refuse)),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir(test)?;

    let started = Instant::now();
    let path = completed_path(run(request(&server, "limited.bin", &dir, CONNECTIONS)).await?);
    let took = started.elapsed();

    assert_eq!(std::fs::read(path)?, server.data);
    let max = server.stats.max_concurrent.load(Ordering::SeqCst);
    let refused = server.stats.refused.load(Ordering::SeqCst);
    assert!(max <= ALLOWED, "{max} connections served at once");
    assert!(refused >= 1, "the limit was never reached");
    // Each extra connection is refused twice, then steps back for good: 12.
    // The slack covers working connections that ask again a moment before the
    // server has counted their previous response as finished, which happens
    // more on a slow, busy machine.
    let surplus = CONNECTIONS - ALLOWED;
    assert!(
        refused <= 2 * surplus + 6,
        "{refused} refused requests: connections kept retrying"
    );
    // About 4 s of data; generous for a 2-core CI runner.
    assert!(took < Duration::from_secs(30), "took {took:?}");
    dir.remove()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_get_503() {
    completes_within_the_server_connection_limit(Refuse::Status(503), "limit-503")
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_get_429() {
    completes_within_the_server_connection_limit(Refuse::Status(429), "limit-429")
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completes_when_extra_connections_are_reset() {
    completes_within_the_server_connection_limit(Refuse::Reset, "limit-reset")
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extra_connections_told_to_wait_step_back_at_once() {
    // Refusals ask for a 20 s wait. Holding a part for that long would delay the
    // end by 20 s; stepping back lets the two allowed connections finish it.
    const ALLOWED: usize = 2;
    const CONNECTIONS: usize = 8;
    let server = TestServer::start(
        test_data(4 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(2 * MIB),
            max_connections: Some((ALLOWED, Refuse::Status(503))),
            refuse_retry_after: Some("20".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("limit-retry-after").unwrap();

    let started = Instant::now();
    let path = completed_path(
        run(request(&server, "wait.bin", &dir, CONNECTIONS))
            .await
            .unwrap(),
    );
    let took = started.elapsed();

    assert_eq!(std::fs::read(path).unwrap(), server.data);
    let refused = server.stats.refused.load(Ordering::SeqCst);
    assert!(refused >= 1, "the limit was never reached");
    assert!(
        refused <= CONNECTIONS - ALLOWED + 4,
        "{refused} refused requests: connections retried instead of stepping back"
    );
    // About 1 s of data; anything near 20 s means a part waited out Retry-After.
    assert!(took < Duration::from_secs(12), "took {took:?}");
    dir.remove().unwrap();
}

fn with_userinfo(mut url: wirecatch_engine::Url) -> Option<wirecatch_engine::Url> {
    url.set_username("alice").ok()?;
    url.set_password(Some("s3cret")).ok()?;
    Some(url)
}

/// `alice:s3cret` as an `Authorization` header value.
const ALICE: &str = "Basic YWxpY2U6czNjcmV0";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn url_credentials_reach_every_request_to_their_origin() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("userinfo").unwrap();
    let mut req = request(&server, "files/private.bin", &dir, 4);
    req.url = with_userinfo(req.url).unwrap();

    let path = completed_path(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    assert!(seen.len() > 1, "expected worker requests too");
    for request in &seen {
        assert_eq!(request.header("authorization"), Some(ALICE), "{request:?}");
    }
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn url_credentials_never_follow_a_redirect_to_another_origin() {
    let target = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(4 * MIB),
            ..Default::default()
        },
    )
    .await;
    let origin = TestServer::start(
        Vec::new(),
        Config {
            redirect: Some(("/start".into(), target.url("files/data.bin").to_string())),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("userinfo-cross").unwrap();
    let mut req = request(&origin, "start", &dir, 4);
    req.url = with_userinfo(req.url).unwrap();

    let path = completed_path(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), target.data);

    assert_eq!(origin.stats.seen()[0].header("authorization"), Some(ALICE));
    let seen = target.stats.seen();
    assert!(seen.len() > 1);
    for request in &seen {
        assert!(
            request.headers.get("authorization").is_none(),
            "{request:?}"
        );
    }
    dir.remove().unwrap();
}

/// Runs `req` in the background and cancels it once `ready` holds, then
/// expects the download to come back within two seconds.
async fn cancel_when(
    req: DownloadRequest,
    ready: impl Fn() -> bool,
) -> TestResult<wirecatch_engine::Result<Outcome>> {
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req, Arc::new(Progress::default()), cancel.clone()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "the expected state never came");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Let the engine settle into its wait.
    tokio::time::sleep(Duration::from_millis(200)).await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .map_err(|_| "cancelling did not stop the download within 2 s")?;
    Ok(stopped?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_while_the_server_never_answers_the_first_request() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
        Config {
            hang: Some(0..usize::MAX),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("hang-probe").unwrap();
    let mut req = request(&server, "hang.bin", &dir, 4);
    req.options.stall_timeout = Duration::from_secs(60);

    let stats = server.stats.clone();
    let result = cancel_when(req, || stats.requests.load(Ordering::SeqCst) >= 1)
        .await
        .unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_while_the_server_never_answers_mid_download() {
    // The first response is short; the request for the rest is never answered.
    let server = TestServer::start(
        test_data(MIB_USIZE),
        Config {
            ranges: true,
            short_first: Some(256 * KIB),
            hang: Some(1..usize::MAX),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("hang-mid").unwrap();
    let mut req = request(&server, "hang.bin", &dir, 1);
    req.options.stall_timeout = Duration::from_secs(60);

    let stats = server.stats.clone();
    let result = cancel_when(req, || stats.requests.load(Ordering::SeqCst) >= 2)
        .await
        .unwrap();
    assert!(matches!(result, Ok(Outcome::Paused { .. })), "{result:?}");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_never_answers_times_out_and_is_retried() {
    let server = TestServer::start(
        test_data(256 * KIB_USIZE),
        Config {
            ranges: true,
            hang: Some(0..1),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("hang-retry").unwrap();
    let mut req = request(&server, "hang.bin", &dir, 4);
    req.options.stall_timeout = Duration::from_millis(500);

    let outcome = tokio::time::timeout(Duration::from_secs(10), run(req))
        .await
        .expect("the download waited for an answer forever instead of timing out");
    let path = completed_path(outcome.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), server.data);

    let seen = server.stats.seen();
    let waited = seen[1].at - seen[0].at;
    assert!(
        waited >= Duration::from_millis(500),
        "retried after {waited:?}, before the timeout"
    );
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_during_a_long_retry_after_on_the_first_request() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
        Config {
            ranges: true,
            busy: Some(Busy {
                requests: 0..1,
                status: 503,
                retry_after: Some("60".into()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("retry-after-cancel-probe").unwrap();
    let stats = server.stats.clone();
    let result = cancel_when(request(&server, "busy.bin", &dir, 4), || {
        stats.requests.load(Ordering::SeqCst) >= 1
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_during_a_long_retry_after_mid_download() {
    let server = TestServer::start(
        test_data(MIB_USIZE),
        Config {
            ranges: true,
            short_first: Some(256 * KIB),
            busy: Some(Busy {
                requests: 1..2,
                status: 503,
                retry_after: Some("60".into()),
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("retry-after-cancel-mid").unwrap();
    let stats = server.stats.clone();
    let result = cancel_when(request(&server, "busy.bin", &dir, 1), || {
        stats.requests.load(Ordering::SeqCst) >= 2
    })
    .await
    .unwrap();
    assert!(matches!(result, Ok(Outcome::Paused { .. })), "{result:?}");
    dir.remove().unwrap();
}

/// Downloads `path` from `server` into `dir`.
async fn run_path(
    server: &TestServer,
    path: &str,
    dir: &Path,
) -> wirecatch_engine::Result<Outcome> {
    run(request(server, path, dir, 4)).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_loop_is_reported_as_such() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
        Config {
            redirect: Some(("/loop".into(), "/loop".into())),
            ..Default::default()
        },
    )
    .await;
    let error = run_path(&server, "loop", &private_dir("redirect-loop").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::TooManyRedirects { .. }), "{error:?}");
    // The first request plus 20 redirects followed.
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 21);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_to_another_scheme_is_refused() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
        Config {
            redirect: Some(("/ftp".into(), "ftp://files.example/a.bin".into())),
            ..Default::default()
        },
    )
    .await;
    let error = run_path(&server, "ftp", &private_dir("redirect-ftp").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidUrl(_)), "{error:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_without_location_is_reported_as_such() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
        Config {
            busy: Some(Busy {
                requests: 0..usize::MAX,
                status: 302,
                retry_after: None,
            }),
            ..Default::default()
        },
    )
    .await;
    let error = run_path(
        &server,
        "nowhere",
        &private_dir("redirect-nowhere").unwrap(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, Error::RedirectWithoutLocation { status: 302, .. }),
        "{error:?}"
    );
}

/// The file is replaced on the server by another of the same size between two
/// requests of one download, so the later answer carries another `ETag`. The
/// bytes of the two versions must not be stitched into one file reported as
/// complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_replaced_mid_download_is_reported_as_changed() -> TestResult {
    let server = TestServer::start(
        test_data(512 * KIB_USIZE),
        Config {
            ranges: true,
            etag: Some("\"v1\"".into()),
            // The first answer ends early, so the download asks again.
            short_first: Some(64 * KIB),
            ..Config::default()
        },
    )
    .await;
    let dir = private_dir("replaced")?;
    server.hold();
    let download = tokio::spawn(run(request(&server, "f.bin", dir.path(), 1)));
    // The first answer is decided once its request arrives; every later one
    // is about the new file.
    server
        .wait_for_request("f.bin", Duration::from_secs(10))
        .await;
    server.configure(|config| config.etag = Some("\"v2\"".into()));
    server.release();
    let result = download.await?;
    assert!(matches!(result, Err(Error::RemoteChanged)), "{result:?}");
    dir.remove()?;
    Ok(())
}

/// The same for a server without byte ranges: the download asks for the
/// whole file again after a dropped connection and skips what it has, which
/// must be the same file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_replaced_before_a_whole_answer_is_asked_again_is_reported_as_changed() -> TestResult
{
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            etag: Some("\"v1\"".into()),
            drop_first: Some((1, 500 * KIB)),
            ..Config::default()
        },
    )
    .await;
    let dir = private_dir("replaced-whole")?;
    server.hold();
    let download = tokio::spawn(run(request(&server, "f.bin", dir.path(), 1)));
    server
        .wait_for_request("f.bin", Duration::from_secs(10))
        .await;
    server.configure(|config| config.etag = Some("\"v2\"".into()));
    server.release();
    let result = download.await?;
    assert!(matches!(result, Err(Error::RemoteChanged)), "{result:?}");
    dir.remove()?;
    Ok(())
}
