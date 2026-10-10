mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Busy, Config, TestServer, test_data};
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error, Outcome, Progress, RemoteFile, download,
};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-it-{test}-{}", std::process::id()));
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

fn only_file(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Starts a download, waits until `at_least` bytes arrived, then cancels it.
async fn pause_midway(req: DownloadRequest, at_least: u64) -> Outcome {
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req, progress.clone(), cancel.clone()));
    while progress.done() < at_least {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel.cancel();
    task.await.unwrap().expect("pausing should not fail")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_download_is_byte_identical() {
    let server = TestServer::start(
        test_data(8 * MIB + 12_345),
        Config {
            ranges: true,
            per_connection_bps: Some(8 * MIB as u64),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("parallel");

    let outcome = run(request(&server, "files/big.bin", &dir, 8)).await;

    let Outcome::Completed {
        path,
        bytes,
        resumed_from,
        ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("big.bin"));
    assert_eq!(bytes, server.data.len() as u64);
    assert_eq!(resumed_from, 0);
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert!(
        server.stats.max_concurrent.load(Ordering::SeqCst) > 1,
        "expected parallel connections"
    );
    assert_eq!(
        only_file(&dir),
        vec!["big.bin"],
        "part and state files must be gone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_one_connection_without_range_support() {
    let server = TestServer::start(test_data(3 * MIB), Config::default()).await;
    let dir = temp_dir("norange");

    let outcome = run(request(&server, "plain.bin", &dir, 8)).await;

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert_eq!(server.stats.max_concurrent.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_length_download_completes() {
    let server = TestServer::start(
        test_data(MIB + 7),
        Config {
            omit_length: true,
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("unknown");

    let outcome = run(request(&server, "stream.bin", &dir, 8)).await;

    let Outcome::Completed { path, bytes, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(bytes, server.data.len() as u64);
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resumes_after_pause_without_downloading_again() {
    let len = 4 * MIB;
    let server = TestServer::start(
        test_data(len),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB as u64),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("resume");

    let paused = pause_midway(request(&server, "r.bin", &dir, 4), MIB as u64).await;
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");
    assert!(dir.join("r.bin.wcpart").exists() && dir.join("r.bin.wcstate").exists());

    // Keep the server throttled: unthrottled, localhost fills megabytes of socket
    // buffers before a connection is dropped, which would hide real waste.
    let sent_before = server.stats.bytes_sent.load(Ordering::SeqCst);
    server.configure(|c| c.per_connection_bps = Some(4 * MIB as u64));
    let outcome = run(request(&server, "r.bin", &dir, 4)).await;

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert!(resumed_from >= MIB as u64, "kept only {resumed_from} bytes");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    let sent = server.stats.bytes_sent.load(Ordering::SeqCst) - sent_before;
    let needed = len as u64 - resumed_from;
    assert!(
        sent < needed + (len / 8) as u64,
        "second run fetched {sent} bytes to fill {needed}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_over_when_the_remote_file_changed() {
    let server = TestServer::start(
        test_data(2 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB as u64),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("changed");

    let paused = pause_midway(request(&server, "c.bin", &dir, 4), 256 * KIB as u64).await;
    assert!(matches!(paused, Outcome::Paused { .. }));

    server.configure(|c| {
        c.per_connection_bps = None;
        c.etag = Some("\"v2\"".into());
    });
    let outcome = run(request(&server, "c.bin", &dir, 4)).await;

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("c.bin"), "should reuse the same name");
    assert_eq!(resumed_from, 0);
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_dropped_connections() {
    let server = TestServer::start(
        test_data(3 * MIB),
        Config {
            ranges: true,
            drop_first: Some((3, 300 * KIB as u64)),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry");

    let outcome = run(request(&server, "flaky.bin", &dir, 4)).await;

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert!(server.stats.requests.load(Ordering::SeqCst) > 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_without_range_support_by_skipping_received_bytes() {
    let server = TestServer::start(
        test_data(2 * MIB),
        Config {
            drop_first: Some((1, 500 * KIB as u64)),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("retry-norange");

    let outcome = run(request(&server, "flaky.bin", &dir, 4)).await;

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uses_content_disposition_and_never_overwrites() {
    let server = TestServer::start(
        test_data(64 * KIB),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("naming");
    std::fs::write(dir.join("report.pdf"), b"keep me").unwrap();

    let outcome = run(request(&server, "download?id=7", &dir, 4)).await;

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(dir.join("report.pdf")).unwrap(), b"keep me");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_file() {
    let server = TestServer::start(
        Vec::new(),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("empty");

    let outcome = run(request(&server, "empty.txt", &dir, 8)).await;

    let Outcome::Completed { path, bytes, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(bytes, 0);
    assert_eq!(std::fs::read(path).unwrap(), b"");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limiter_caps_combined_speed() {
    let server = TestServer::start(
        test_data(2 * MIB),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("limited");
    let mut req = request(&server, "slow.bin", &dir, 4);
    req.options.rate_limiters = vec![Arc::new(wirecatch_engine::RateLimiter::new(Some(
        MIB as u64,
    )))];

    let start = std::time::Instant::now();
    let outcome = run(req).await;
    let secs = start.elapsed().as_secs_f64();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    // 2 MiB at 1 MiB/s, less the quarter-second burst allowance.
    assert!(secs >= 1.6, "finished in {secs:.2} s despite the limit");
}

/// A caller may drop a download's future before it ends (a manager abandons a
/// run that does not stop in time). Nothing of that download may keep writing
/// afterwards: a resume file saved later could come back after the caller deleted
/// it, or overwrite a newer run's with stale ranges.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_download_never_writes_its_resume_file_again() {
    let server = TestServer::start(
        test_data(4 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(256 * KIB as u64),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("dropped");
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(
        request(&server, "d.bin", &dir, 2),
        progress.clone(),
        CancellationToken::new(),
    ));
    while progress.done() < 64 * KIB as u64 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // Let a save that was already running finish, then delete the files, as a
    // cancelled download's owner does.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (part, state) = (dir.join("d.bin.wcpart"), dir.join("d.bin.wcstate"));
    std::fs::remove_file(&part).unwrap();
    std::fs::remove_file(&state).unwrap();

    // Twenty checkpoint intervals later, nothing came back.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !state.exists(),
        "the dropped download wrote its resume file again"
    );
    assert!(!part.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exact_path_continues_after_the_link_changes() {
    let server = TestServer::start(
        test_data(3 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB as u64),
            etag: Some("\"same\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("exact");
    let target = dir.join("nested").join("video.mp4");

    let mut first = request(&server, "old-signed-link", &dir, 4);
    first.exact_path = Some(target.clone());
    let paused = pause_midway(first, 512 * KIB as u64).await;
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");

    server.configure(|c| c.per_connection_bps = None);
    let mut second = request(&server, "fresh-signed-link", &dir, 4);
    second.exact_path = Some(target.clone());
    let outcome = run(second).await;

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, target);
    assert!(
        resumed_from >= 512 * KIB as u64,
        "kept only {resumed_from} bytes"
    );
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}

/// A server whose first answer is `503` with `Retry-After: 1`, so there is a
/// second during which the link has not been proven yet.
async fn slow_to_answer(content_disposition: &str) -> TestServer {
    TestServer::start(
        test_data(256 * KIB),
        Config {
            ranges: true,
            content_disposition: Some(content_disposition.to_string()),
            busy: Some(Busy {
                requests: 0..1,
                status: 503,
                retry_after: Some("1".into()),
            }),
            ..Default::default()
        },
    )
    .await
}

/// Runs `req`, checks that no path is known while the server has not answered
/// yet, and returns the outcome with the path progress reported afterwards.
async fn run_watching_path(req: DownloadRequest) -> (Outcome, Option<PathBuf>) {
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(req, progress.clone(), CancellationToken::new()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        progress.path(),
        None,
        "path known before the server answered"
    );
    let outcome = task.await.unwrap().expect("download failed");
    (outcome, progress.path())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_is_the_server_chosen_name_once_it_answers() {
    let server = slow_to_answer("attachment; filename=\"report.pdf\"").await;
    let dir = temp_dir("progress-path");
    std::fs::write(dir.join("report.pdf"), b"keep me").unwrap();

    let (outcome, path) = run_watching_path(request(&server, "download?id=7", &dir, 4)).await;

    let Outcome::Completed { path: saved, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(saved, dir.join("report (1).pdf"));
    assert_eq!(path, Some(saved));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_is_the_exact_path_once_the_server_answers() {
    let server = slow_to_answer("attachment; filename=\"ignored.pdf\"").await;
    let dir = temp_dir("progress-exact");
    let target = dir.join("chosen").join("by-manager.bin");
    let mut req = request(&server, "file", &dir, 4);
    req.exact_path = Some(target.clone());

    let (outcome, path) = run_watching_path(req).await;

    assert!(matches!(outcome, Outcome::Completed { .. }), "{outcome:?}");
    assert_eq!(path, Some(target));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_stays_unset_when_the_first_request_fails() {
    let server = TestServer::start(
        test_data(KIB),
        Config {
            busy: Some(Busy {
                requests: 0..usize::MAX,
                status: 404,
                retry_after: None,
            }),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("progress-404");
    let progress = Arc::new(Progress::default());

    let result = download(
        request(&server, "missing.bin", &dir, 4),
        progress.clone(),
        CancellationToken::new(),
    )
    .await;

    assert!(
        matches!(
            result,
            Err(wirecatch_engine::Error::HttpStatus { status: 404, .. })
        ),
        "{result:?}"
    );
    assert_eq!(progress.path(), None);
    assert_eq!(progress.file_name(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn path_resolver_sees_the_server_answer_and_picks_the_path() {
    let server = TestServer::start(
        test_data(512 * KIB),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            content_type: Some("application/pdf".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("resolver");
    let folder = dir.join("Documents");
    std::fs::create_dir_all(&folder).unwrap();
    // The resolver owns naming: an existing file is replaced, no "(1)" suffix.
    std::fs::write(folder.join("report.pdf"), b"old version").unwrap();

    let seen: Arc<Mutex<Option<RemoteFile>>> = Arc::default();
    let mut req = request(&server, "download?id=7", &dir, 4);
    req.resolve_path = Some({
        let (seen, folder) = (seen.clone(), folder.clone());
        Arc::new(move |remote: &RemoteFile| {
            *seen.lock().unwrap() = Some(remote.clone());
            Ok(folder.join(&remote.suggested_name))
        })
    });
    let progress = Arc::new(Progress::default());

    let outcome = download(req, progress.clone(), CancellationToken::new())
        .await
        .expect("download failed");

    let remote = seen
        .lock()
        .unwrap()
        .clone()
        .expect("resolver was not called");
    assert_eq!(remote.suggested_name, "report.pdf");
    assert_eq!(remote.total, Some(512 * KIB as u64));
    assert_eq!(remote.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(remote.final_url, server.url("download?id=7"));
    assert!(remote.resumable);

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, folder.join("report.pdf"));
    assert_eq!(progress.path(), Some(path.clone()));
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert_eq!(only_file(&folder), vec!["report.pdf"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn path_resolver_can_refuse_the_file_with_nothing_written() {
    let server = TestServer::start(
        test_data(64 * KIB),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("resolver-refuses");
    let mut req = request(&server, "files/setup.exe", &dir, 4);
    req.resolve_path = Some(Arc::new(|remote: &RemoteFile| {
        Err(format!(
            "{} is not an allowed file type",
            remote.suggested_name
        ))
    }));
    let progress = Arc::new(Progress::default());

    let result = download(req, progress.clone(), CancellationToken::new()).await;

    match result {
        Err(Error::TargetRejected(reason)) => {
            assert_eq!(reason, "setup.exe is not an allowed file type");
        }
        other => panic!("expected TargetRejected, got {other:?}"),
    }
    assert!(
        only_file(&dir).is_empty(),
        "files written: {:?}",
        only_file(&dir)
    );
    assert_eq!(progress.path(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resolved_path_resumes_later_as_exact_path() {
    let server = TestServer::start(
        test_data(3 * MIB),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB as u64),
            etag: Some("\"same\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = temp_dir("resolver-resume");
    let target = dir.join("picked").join("video.mp4");

    let mut first = request(&server, "video", &dir, 4);
    first.resolve_path = Some({
        let target = target.clone();
        Arc::new(move |_: &RemoteFile| Ok(target.clone()))
    });
    let paused = pause_midway(first, 512 * KIB as u64).await;
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");

    server.configure(|c| c.per_connection_bps = None);
    let mut second = request(&server, "video", &dir, 4);
    second.exact_path = Some(target.clone());
    let outcome = run(second).await;

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, target);
    assert!(
        resumed_from >= 512 * KIB as u64,
        "kept only {resumed_from} bytes"
    );
    assert_eq!(std::fs::read(path).unwrap(), server.data);
}
