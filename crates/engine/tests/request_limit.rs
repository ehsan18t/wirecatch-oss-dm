//! `DownloadOptions::max_request_bytes`: against a server that refuses any
//! request for more than a set amount (as some video servers slow such requests
//! down), every request keeps to the limit, and splitting, resuming, retries,
//! the stall timeout and speed limits all still work.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error, Outcome, Progress, RateLimiter, download,
};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::test_data;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-limit-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server with ranges that refuses requests for more than `limit` bytes.
async fn strict_server(len: usize, limit: u64, bps: Option<u64>) -> TestServer {
    TestServer::start(
        test_data(len),
        Config {
            ranges: true,
            per_connection_bps: bps,
            etag: Some("\"v1\"".into()),
            max_range: Some(limit),
            ..Default::default()
        },
    )
    .await
}

fn request(
    server: &TestServer,
    dir: &Path,
    connections: usize,
    limit: Option<u64>,
) -> DownloadRequest {
    let mut req = DownloadRequest::new(server.url("video.mp4"), dir);
    req.options.connections = connections;
    req.options.min_split = 128 * KIB as u64;
    req.options.buffer_size = 64 * KIB;
    req.options.stall_timeout = Duration::from_secs(5);
    req.options.checkpoint_interval = Duration::from_millis(100);
    req.options.max_request_bytes = limit;
    req
}

async fn run(req: DownloadRequest) -> Outcome {
    download(req, Arc::new(Progress::default()), CancellationToken::new())
        .await
        .expect("download failed")
}

fn completed(outcome: Outcome) -> (PathBuf, u64) {
    match outcome {
        Outcome::Completed {
            path, resumed_from, ..
        } => (path, resumed_from),
        other => panic!("not completed: {other:?}"),
    }
}

/// Every request the server saw asked for a closed range of at most `limit`
/// bytes and got it.
fn assert_all_within(server: &TestServer, limit: u64) {
    let seen = server.stats.seen();
    assert!(!seen.is_empty());
    for s in &seen {
        let (start, end) = s.range().unwrap_or_else(|| {
            panic!(
                "a request asked for no range or an open one: {:?}",
                s.header("range")
            )
        });
        let end = end.unwrap_or_else(|| panic!("an open range: {:?}", s.header("range")));
        assert!(end - start <= limit, "asked for {} bytes", end - start);
        assert_eq!(s.status, 206, "{:?} was refused", s.header("range"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_request_asks_for_more_than_the_limit_while_connections_split() {
    let limit = 512 * KIB as u64;
    // Slow enough per connection that idle connections split the busy ones.
    let server = strict_server(5 * MIB + 333, limit, Some(4 * MIB as u64)).await;
    let dir = temp_dir("split");

    let (path, _) = completed(run(request(&server, &dir, 4, Some(limit))).await);

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_all_within(&server, limit);
    assert!(
        server.stats.max_concurrent.load(Ordering::SeqCst) > 1,
        "expected parallel connections"
    );
    // Every byte arrived once, give or take ranges cut short by a split.
    let sent = server.stats.bytes_sent.load(Ordering::SeqCst);
    assert!(sent < server.data.len() as u64 + MIB as u64, "sent {sent}");
}

/// Without the limit the same server refuses the download: the test above
/// passes only because the limit is kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_the_limit_the_strict_server_refuses() {
    let server = strict_server(2 * MIB, 512 * KIB as u64, None).await;
    let dir = temp_dir("unlimited");

    let error = download(
        request(&server, &dir, 4, None),
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, Error::HttpStatus { status: 403, .. }),
        "{error:?}"
    );
    assert_eq!(server.stats.seen()[0].header("range"), Some("bytes=0-"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_connection_sends_its_requests_one_after_another_on_one_tcp_connection() {
    let limit = 256 * KIB as u64;
    let len = 3 * MIB + 5;
    let server = strict_server(len, limit, None).await;
    let dir = temp_dir("keepalive");

    let start = Instant::now();
    let (path, _) = completed(run(request(&server, &dir, 1, Some(limit))).await);
    let took = start.elapsed();

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_all_within(&server, limit);
    let seen = server.stats.seen();
    // Back to back from the first byte to the last, and nothing twice.
    let ranges: Vec<(u64, u64)> = seen
        .iter()
        .map(|s| {
            let (start, end) = s.range().unwrap();
            (start, end.unwrap().min(len as u64))
        })
        .collect();
    let expected: Vec<(u64, u64)> = (0..(len as u64).div_ceil(limit))
        .map(|n| (n * limit, ((n + 1) * limit).min(len as u64)))
        .collect();
    assert_eq!(ranges, expected);
    let connections: HashSet<_> = seen.iter().map(|s| s.peer).collect();
    assert_eq!(connections.len(), 1, "a new TCP connection per request");
    // The end of one request is not a failure: the next one goes out at once,
    // with no pause for a retry (13 retries would take over 6 s).
    assert!(took < Duration::from_secs(3), "took {took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_download_resumes_with_limited_requests() {
    let limit = 256 * KIB as u64;
    let len = 4 * MIB;
    let server = strict_server(len, limit, Some(MIB as u64)).await;
    let dir = temp_dir("resume");

    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(
        request(&server, &dir, 4, Some(limit)),
        progress.clone(),
        cancel.clone(),
    ));
    while progress.done() < MIB as u64 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel.cancel();
    let paused = task.await.unwrap().expect("pausing should not fail");
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");
    assert!(dir.join("video.mp4.wcstate").exists());

    let sent_before = server.stats.bytes_sent.load(Ordering::SeqCst);
    let seen_before = server.stats.seen().len();
    server.configure(|c| c.per_connection_bps = Some(4 * MIB as u64));
    let (path, resumed_from) = completed(run(request(&server, &dir, 4, Some(limit))).await);

    assert!(resumed_from >= MIB as u64, "kept only {resumed_from} bytes");
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_all_within(&server, limit);
    // The second run fetched what was missing, not the whole file again.
    let sent = server.stats.bytes_sent.load(Ordering::SeqCst) - sent_before;
    let needed = len as u64 - resumed_from;
    assert!(
        sent < needed + (len / 4) as u64,
        "second run fetched {sent} bytes to fill {needed}"
    );
    // It went on from where the first run stopped, not from the start.
    assert!(
        server.stats.seen()[seen_before..]
            .iter()
            .any(|s| s.range().is_some_and(|(start, _)| start > 0)),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn limited_requests_are_retried_after_dropped_connections() {
    let limit = 512 * KIB as u64;
    let server = TestServer::start(
        test_data(3 * MIB),
        Config {
            ranges: true,
            max_range: Some(limit),
            drop_first: Some((3, 100 * KIB as u64)),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry");

    let (path, _) = completed(run(request(&server, &dir, 4, Some(limit))).await);

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    for s in server.stats.seen() {
        let (start, end) = s.range().unwrap();
        assert!(end.unwrap() - start <= limit);
    }
}

/// A server that stops answering in the middle of a series of requests: the
/// stall timeout ends the wait and the request is sent again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_that_hangs_is_given_up_after_the_stall_timeout_and_sent_again() {
    let limit = 256 * KIB as u64;
    let server = TestServer::start(
        test_data(MIB),
        Config {
            ranges: true,
            max_range: Some(limit),
            hang: Some(2..3),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("stall");
    let mut req = request(&server, &dir, 1, Some(limit));
    req.options.stall_timeout = Duration::from_millis(500);

    let (path, _) = completed(run(req).await);

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    let seen = server.stats.seen();
    assert_eq!(
        seen[2].range(),
        seen[3].range(),
        "the hung request was sent again"
    );
    assert_eq!(seen.len(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn limited_requests_keep_to_the_speed_limit() {
    let limit = 128 * KIB as u64;
    let server = strict_server(2 * MIB, limit, None).await;
    let dir = temp_dir("speed");
    let mut req = request(&server, &dir, 4, Some(limit));
    req.options.rate_limiters = vec![Arc::new(RateLimiter::new(Some(MIB as u64)))];

    let start = Instant::now();
    let (path, _) = completed(run(req).await);
    let secs = start.elapsed().as_secs_f64();

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_all_within(&server, limit);
    // 2 MiB at 1 MiB/s, less the quarter-second burst allowance.
    assert!(secs >= 1.6, "finished in {secs:.2} s despite the limit");
}

/// A server without byte ranges sends the whole file to the first request, as
/// without the limit; a small file fits in the first request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn whole_answers_and_small_files_take_one_request() {
    let limit = 256 * KIB as u64;
    let plain = TestServer::start(test_data(MIB + 1), Config::default()).await;
    let dir = temp_dir("plain");
    let (path, _) = completed(run(request(&plain, &dir, 4, Some(limit))).await);
    assert_eq!(std::fs::read(&path).unwrap(), plain.data);
    assert_eq!(plain.stats.requests.load(Ordering::SeqCst), 1);

    let small = strict_server(100 * KIB, limit, None).await;
    let dir = temp_dir("small");
    let (path, _) = completed(run(request(&small, &dir, 4, Some(limit))).await);
    assert_eq!(std::fs::read(&path).unwrap(), small.data);
    assert_eq!(small.stats.requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        small.stats.seen()[0].header("range"),
        Some(format!("bytes=0-{}", limit - 1).as_str())
    );
}

/// A server that answers the limited first request with part of the file and
/// no size: the end of the file cannot be known from that, so the file is
/// asked for whole, as without the limit, instead of ending after one piece.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_range_answer_without_a_size_is_asked_for_whole() {
    let limit = 256 * KIB as u64;
    let server = TestServer::start(
        test_data(MIB + 17),
        Config {
            ranges: true,
            unknown_total: true,
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("no-size");

    let (path, _) = completed(run(request(&server, &dir, 4, Some(limit))).await);

    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    let asked: Vec<Option<String>> = server
        .stats
        .seen()
        .iter()
        .map(|s| s.header("range").map(str::to_string))
        .collect();
    assert_eq!(
        asked,
        [
            Some(format!("bytes=0-{}", limit - 1)),
            Some("bytes=0-".into())
        ]
    );
}
