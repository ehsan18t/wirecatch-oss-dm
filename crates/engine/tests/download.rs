//! Downloads over HTTP through the public `download()` API, against the local test server: parallel connections, resuming, retries, naming and where the file goes.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wirecatch_engine::units::{KIB, KIB_USIZE, MIB, MIB_USIZE};
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error, ExistingFile, Outcome, PathRejected, PrivateNetwork,
    Progress, RemoteFile, download,
};
use wirecatch_test_support::http::{Busy, Config, TestServer};
use wirecatch_test_support::{TestResult, files_in, private_dir, test_data};

mod common;
use common::{request, run};

/// Starts a download, waits until `at_least` bytes arrived, then cancels it.
async fn pause_midway(req: DownloadRequest, at_least: u64) -> TestResult<Outcome> {
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req, progress.clone(), cancel.clone()));
    while progress.done() < at_least {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel.cancel();
    Ok(task.await??)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_download_is_byte_identical() {
    let server = TestServer::start(
        test_data(8 * MIB_USIZE + 12_345),
        Config {
            ranges: true,
            per_connection_bps: Some(8 * MIB),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("parallel").unwrap();

    let outcome = run(request(&server, "files/big.bin", &dir, 8))
        .await
        .unwrap();

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
        files_in(&dir),
        vec!["big.bin"],
        "part and state files must be gone"
    );
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_one_connection_without_range_support() {
    let server = TestServer::start(test_data(3 * MIB_USIZE), Config::default()).await;
    let dir = private_dir("norange").unwrap();

    let outcome = run(request(&server, "plain.bin", &dir, 8)).await.unwrap();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert_eq!(server.stats.max_concurrent.load(Ordering::SeqCst), 1);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_length_download_completes() {
    let server = TestServer::start(
        test_data(MIB_USIZE + 7),
        Config {
            omit_length: true,
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("unknown").unwrap();

    let outcome = run(request(&server, "stream.bin", &dir, 8)).await.unwrap();

    let Outcome::Completed { path, bytes, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(bytes, server.data.len() as u64);
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resumes_after_pause_without_downloading_again() {
    let len = 4 * MIB_USIZE;
    let server = TestServer::start(
        test_data(len),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("resume").unwrap();

    let paused = pause_midway(request(&server, "r.bin", &dir, 4), MIB)
        .await
        .unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");
    assert!(dir.join("r.bin.wcpart").exists() && dir.join("r.bin.wcstate").exists());

    // Keep the server throttled: unthrottled, localhost fills megabytes of socket
    // buffers before a connection is dropped, which would hide real waste.
    let sent_before = server.stats.bytes_sent.load(Ordering::SeqCst);
    server.configure(|c| c.per_connection_bps = Some(4 * MIB));
    let outcome = run(request(&server, "r.bin", &dir, 4)).await.unwrap();

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert!(resumed_from >= MIB, "kept only {resumed_from} bytes");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    let sent = server.stats.bytes_sent.load(Ordering::SeqCst) - sent_before;
    let needed = len as u64 - resumed_from;
    assert!(
        sent < needed + (len / 8) as u64,
        "second run fetched {sent} bytes to fill {needed}"
    );
    dir.remove().unwrap();
}

/// A resume file is data from outside the process: a hand edit, a torn write
/// or a bug can leave ranges that overlap, run past the end or are out of
/// order. Such a file is set aside and the download starts over, instead of
/// asking for bytes past the end or counting more bytes kept than the file has.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tampered_resume_file_starts_the_download_over() {
    let len = 2 * MIB;
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let range = |start: u64, end: u64| serde_json::json!({ "start": start, "end": end });
    for (case, remaining) in [
        ("past the end", vec![range(0, len + MIB)]),
        ("overlapping", vec![range(0, len), range(KIB, len)]),
        ("out of order", vec![range(MIB, len), range(0, KIB)]),
        ("empty", vec![range(KIB, KIB)]),
    ] {
        let dir = private_dir("tampered").unwrap();
        let paused = pause_midway(request(&server, "t.bin", &dir, 2), 64 * KIB)
            .await
            .unwrap();
        assert!(
            matches!(paused, Outcome::Paused { .. }),
            "{case}: {paused:?}"
        );
        let state_path = dir.join("t.bin.wcstate");
        let mut state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
        state["remaining"] = serde_json::Value::Array(remaining);
        std::fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();

        let outcome = run(request(&server, "t.bin", &dir, 2)).await;
        let Ok(Outcome::Completed {
            path, resumed_from, ..
        }) = outcome
        else {
            panic!("{case}: not completed: {outcome:?}")
        };
        assert_eq!(resumed_from, 0, "{case}");
        assert_eq!(std::fs::read(path).unwrap(), server.data, "{case}");
        dir.remove().unwrap();
    }
}

/// A process killed after the last range reached the disk, but before the part
/// file was renamed, leaves a resume file with no range left. The next run
/// finishes the file as it is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_file_with_nothing_left_finishes_the_whole_file() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("all-flushed").unwrap();
    let paused = pause_midway(request(&server, "f.bin", &dir, 2), 64 * KIB)
        .await
        .unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }), "{paused:?}");
    let state_path = dir.join("f.bin.wcstate");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    state["remaining"] = serde_json::json!([]);
    std::fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    std::fs::write(dir.join("f.bin.wcpart"), &server.data).unwrap();

    let outcome = run(request(&server, "f.bin", &dir, 2)).await.unwrap();
    let Outcome::Completed {
        path,
        bytes,
        resumed_from,
        ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!((bytes, resumed_from), (2 * MIB, 2 * MIB));
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

/// A disk error says what the engine was doing and with which file, so the
/// person can tell a full disk from a folder they cannot write to.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disk_error_names_the_file_and_what_failed() {
    let server = TestServer::start(
        test_data(64 * KIB_USIZE),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("disk-error").unwrap();
    // A file where the folder should be: the folder cannot be created.
    let blocked = dir.join("not-a-folder");
    std::fs::write(&blocked, b"x").unwrap();
    let error = run(request(&server, "f.bin", &blocked.join("inside"), 1))
        .await
        .unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("create the folder") && text.contains("inside"),
        "{text}"
    );
    // The system's own reason is the source, shown once in a chain of errors.
    let reason = std::error::Error::source(&error).unwrap().to_string();
    assert!(!text.contains(&reason), "{text}");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_over_when_the_remote_file_changed() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("changed").unwrap();

    let paused = pause_midway(request(&server, "c.bin", &dir, 4), 256 * KIB)
        .await
        .unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }));

    server.configure(|c| {
        c.per_connection_bps = None;
        c.etag = Some("\"v2\"".into());
    });
    let outcome = run(request(&server, "c.bin", &dir, 4)).await.unwrap();

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("c.bin"), "should reuse the same name");
    assert_eq!(resumed_from, 0);
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_dropped_connections() {
    let server = TestServer::start(
        test_data(3 * MIB_USIZE),
        Config {
            ranges: true,
            drop_first: Some((3, 300 * KIB)),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("retry").unwrap();

    let outcome = run(request(&server, "flaky.bin", &dir, 4)).await.unwrap();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert!(server.stats.requests.load(Ordering::SeqCst) > 3);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_without_range_support_by_skipping_received_bytes() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            drop_first: Some((1, 500 * KIB)),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("retry-norange").unwrap();

    let outcome = run(request(&server, "flaky.bin", &dir, 4)).await.unwrap();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uses_content_disposition_and_never_overwrites() {
    let server = TestServer::start(
        test_data(64 * KIB_USIZE),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("naming").unwrap();
    std::fs::write(dir.join("report.pdf"), b"keep me").unwrap();

    let outcome = run(request(&server, "download?id=7", &dir, 4))
        .await
        .unwrap();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(dir.join("report.pdf")).unwrap(), b"keep me");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
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
    let dir = private_dir("empty").unwrap();

    let outcome = run(request(&server, "empty.txt", &dir, 8)).await.unwrap();

    let Outcome::Completed { path, bytes, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(bytes, 0);
    assert_eq!(std::fs::read(path).unwrap(), b"");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limiter_caps_combined_speed() {
    let server = TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("limited").unwrap();
    let mut req = request(&server, "slow.bin", &dir, 4);
    req.options.rate_limiters = vec![Arc::new(wirecatch_engine::RateLimiter::new(Some(MIB)))];

    let start = std::time::Instant::now();
    let outcome = run(req).await.unwrap();
    let secs = start.elapsed().as_secs_f64();

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    // 2 MiB at 1 MiB/s, less the quarter-second burst allowance.
    assert!(secs >= 1.6, "finished in {secs:.2} s despite the limit");
    dir.remove().unwrap();
}

/// How long a pause may take to come back, far less than the limiter's wait.
const PROMPT: Duration = Duration::from_secs(3);

/// At 1 KiB/s the first chunk alone puts the limiter many seconds in debt. A pause must not wait for that, nor must a raised limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_or_lifting_the_limit_never_waits_out_the_rate_limiter() {
    let server = TestServer::start(
        test_data(MIB_USIZE),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("limited-pause").unwrap();
    let limiter = Arc::new(wirecatch_engine::RateLimiter::new(Some(KIB)));
    let mut req = request(&server, "slow.bin", &dir, 2);
    req.options.rate_limiters = vec![limiter.clone()];

    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    let deadline = tokio::time::Instant::now() + PROMPT;
    while progress.done() == 0 {
        assert!(tokio::time::Instant::now() < deadline, "nothing arrived");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel.cancel();
    let paused = tokio::time::timeout(PROMPT, task).await;
    let outcome = paused
        .expect("the pause waited for the rate limiter")
        .unwrap()
        .unwrap();
    let Outcome::Paused { remaining, .. } = outcome else {
        panic!("not paused: {outcome:?}")
    };
    let kept = server.data.len() as u64 - remaining;

    // Resumed at the same tiny limit, then lifted once a new chunk has put
    // the limiter in debt: the download finishes at once.
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(req, progress.clone(), CancellationToken::new()));
    let deadline = tokio::time::Instant::now() + PROMPT;
    while progress.done() <= kept {
        assert!(
            tokio::time::Instant::now() < deadline,
            "nothing new arrived"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    limiter.set_limit(None);
    let finished = tokio::time::timeout(PROMPT, task).await;
    let outcome = finished
        .expect("lifting the limit did not wake the download")
        .unwrap()
        .unwrap();
    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

/// File system work a download queued on the blocking pool runs even after
/// its future is dropped. Such work must change no file then: the owner may
/// already have deleted or reused them. Here the pool is held busy while the
/// download waits to prepare its target, and the download is dropped before
/// the pool frees up; the old part and resume files it would have replaced
/// must stay exactly as they were.
#[test]
fn a_dropped_download_leaves_its_files_alone() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let server = TestServer::start(
            test_data(64 * KIB_USIZE),
            Config {
                ranges: true,
                ..Default::default()
            },
        )
        .await;
        let dir = private_dir("dropped-early").unwrap();
        // Loads the system certificates now, which needs the blocking pool.
        run(request(&server, "warm.bin", &dir, 1)).await.unwrap();

        let target = dir.join("f.bin");
        let (part, state) = (dir.join("f.bin.wcpart"), dir.join("f.bin.wcstate"));
        std::fs::write(&part, b"old part").unwrap();
        std::fs::write(&state, b"not a resume file").unwrap();

        // The pool's only thread waits until it is let go.
        let (release, held) = std::sync::mpsc::channel::<()>();
        let holding = tokio::task::spawn_blocking(move || held.recv());

        let (choosing, chose) = tokio::sync::oneshot::channel();
        let choosing = Mutex::new(Some(choosing));
        let mut req = request(&server, "f.bin", &dir, 1);
        let chosen = target.clone();
        req.resolve_path = Some(Arc::new(move |_: &RemoteFile| {
            if let Some(tell) = choosing.lock().unwrap().take() {
                tell.send(()).unwrap();
            }
            Ok(chosen.clone())
        }));
        let task = tokio::spawn(run(req));
        // On this one thread the download runs until it waits for the pool,
        // with its preparation queued, before this resumes.
        chose.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        release.send(()).unwrap();
        holding.await.unwrap().unwrap();
        // The queue is first in, first out: this runs after the preparation.
        tokio::task::spawn_blocking(|| ()).await.unwrap();

        assert!(
            std::fs::read(&part).unwrap() == b"old part",
            "the part file was replaced"
        );
        assert!(
            std::fs::read(&state).unwrap() == b"not a resume file",
            "the resume file was replaced"
        );
        assert!(!target.exists());
        dir.remove().unwrap();
    });
}

/// A caller may drop a download's future before it ends (a manager abandons a
/// run that does not stop in time). Nothing of that download may keep writing
/// afterwards: a resume file saved later could come back after the caller deleted
/// it, or overwrite a newer run's with stale ranges.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_download_never_writes_its_resume_file_again() {
    let server = TestServer::start(
        test_data(4 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(256 * KIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("dropped").unwrap();
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(
        request(&server, "d.bin", &dir, 2),
        progress.clone(),
        CancellationToken::new(),
    ));
    while progress.done() < 64 * KIB {
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
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exact_path_continues_after_the_link_changes() {
    let server = TestServer::start(
        test_data(3 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"same\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("exact").unwrap();
    let target = dir.join("nested").join("video.mp4");

    let mut first = request(&server, "old-signed-link", &dir, 4);
    first.exact_path = Some(target.clone());
    let paused = pause_midway(first, 512 * KIB).await.unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");

    server.configure(|c| c.per_connection_bps = None);
    let mut second = request(&server, "fresh-signed-link", &dir, 4);
    second.exact_path = Some(target.clone());
    let outcome = run(second).await.unwrap();

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, target);
    assert!(resumed_from >= 512 * KIB, "kept only {resumed_from} bytes");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

/// A server whose first answer is `503` with `Retry-After: 1`, so there is a
/// second during which the link has not been proven yet.
async fn slow_to_answer(content_disposition: &str) -> TestServer {
    TestServer::start(
        test_data(256 * KIB_USIZE),
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
async fn run_watching_path(req: DownloadRequest) -> TestResult<(Outcome, Option<PathBuf>)> {
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(req, progress.clone(), CancellationToken::new()));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        progress.path(),
        None,
        "path known before the server answered"
    );
    let outcome = task.await??;
    Ok((outcome, progress.path()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_is_the_server_chosen_name_once_it_answers() {
    let server = slow_to_answer("attachment; filename=\"report.pdf\"").await;
    let dir = private_dir("progress-path").unwrap();
    std::fs::write(dir.join("report.pdf"), b"keep me").unwrap();

    let (outcome, path) = run_watching_path(request(&server, "download?id=7", &dir, 4))
        .await
        .unwrap();

    let Outcome::Completed { path: saved, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(saved, dir.join("report (1).pdf"));
    assert_eq!(path, Some(saved));
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_is_the_exact_path_once_the_server_answers() {
    let server = slow_to_answer("attachment; filename=\"ignored.pdf\"").await;
    let dir = private_dir("progress-exact").unwrap();
    let target = dir.join("chosen").join("by-manager.bin");
    let mut req = request(&server, "file", &dir, 4);
    req.exact_path = Some(target.clone());

    let (outcome, path) = run_watching_path(req).await.unwrap();

    assert!(matches!(outcome, Outcome::Completed { .. }), "{outcome:?}");
    assert_eq!(path, Some(target));
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_path_stays_unset_when_the_first_request_fails() {
    let server = TestServer::start(
        test_data(KIB_USIZE),
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
    let dir = private_dir("progress-404").unwrap();
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
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn path_resolver_sees_the_server_answer_and_picks_the_path() {
    let server = TestServer::start(
        test_data(512 * KIB_USIZE),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            content_type: Some("application/pdf".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("resolver").unwrap();
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
    assert_eq!(remote.total, Some(512 * KIB));
    assert_eq!(remote.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(remote.final_url, server.url("download?id=7"));
    assert!(remote.resumable);

    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, folder.join("report.pdf"));
    assert_eq!(progress.path(), Some(path.clone()));
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    assert_eq!(files_in(&folder), vec!["report.pdf"]);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn path_resolver_can_refuse_the_file_with_nothing_written() {
    let server = TestServer::start(
        test_data(64 * KIB_USIZE),
        Config {
            ranges: true,
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("resolver-refuses").unwrap();
    let mut req = request(&server, "files/setup.exe", &dir, 4);
    req.resolve_path = Some(Arc::new(|remote: &RemoteFile| {
        Err(PathRejected::new(format!(
            "{} is not an allowed file type",
            remote.suggested_name
        )))
    }));
    let progress = Arc::new(Progress::default());

    let result = download(req, progress.clone(), CancellationToken::new()).await;

    match result {
        Err(Error::TargetRejected(reason)) => {
            assert_eq!(reason.message(), "setup.exe is not an allowed file type");
        }
        other => panic!("expected TargetRejected, got {other:?}"),
    }
    assert!(
        files_in(&dir).is_empty(),
        "files written: {:?}",
        files_in(&dir)
    );
    assert_eq!(progress.path(), None);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resolved_path_resumes_later_as_exact_path() {
    let server = TestServer::start(
        test_data(3 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"same\"".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("resolver-resume").unwrap();
    let target = dir.join("picked").join("video.mp4");

    let mut first = request(&server, "video", &dir, 4);
    first.resolve_path = Some({
        let target = target.clone();
        Arc::new(move |_: &RemoteFile| Ok(target.clone()))
    });
    let paused = pause_midway(first, 512 * KIB).await.unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }), "got {paused:?}");

    server.configure(|c| c.per_connection_bps = None);
    let mut second = request(&server, "video", &dir, 4);
    second.exact_path = Some(target.clone());
    let outcome = run(second).await.unwrap();

    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, target);
    assert!(resumed_from >= 512 * KIB, "kept only {resumed_from} bytes");
    assert_eq!(std::fs::read(path).unwrap(), server.data);
    dir.remove().unwrap();
}

/// With the local network blocked, a download from a site (127.0.0.1 stands in
/// for one on the internet: its own origin stays reachable) may not be
/// redirected to a device at home, and the device is never contacted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_cannot_be_redirected_into_the_local_network_when_blocked() {
    let device = TestServer::start(test_data(10 * KIB_USIZE), Config::default()).await;
    let site = TestServer::start(
        test_data(10 * KIB_USIZE),
        Config {
            redirect: Some(("/video.mp4".into(), device.url("secret").to_string())),
            ..Default::default()
        },
    )
    .await;
    let dir = private_dir("private-redirect").unwrap();

    // The site's own file still downloads.
    let mut own = request(&site, "own.bin", &dir, 2);
    own.options.private_network = PrivateNetwork::Block;
    assert!(matches!(run(own).await.unwrap(), Outcome::Completed { .. }));

    let mut req = request(&site, "video.mp4", &dir, 2);
    req.options.private_network = PrivateNetwork::Block;
    req.options.max_retries = 0;
    let error = download(req, Arc::new(Progress::default()), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, Error::PrivateNetwork { .. }), "{error}");
    assert!(device.stats.seen().is_empty(), "the device was contacted");

    // Without blocking (the default), the same redirect is followed.
    let req = request(&site, "video.mp4", &dir, 2);
    assert!(matches!(run(req).await.unwrap(), Outcome::Completed { .. }));
    assert!(!device.stats.seen().is_empty());
    dir.remove().unwrap();
}

/// Runs `req` on a task, and once `at_least` bytes arrived, saves "theirs" at `taken` the way another program would, then waits for the download's end.
async fn another_program_saves_meanwhile(
    req: DownloadRequest,
    at_least: u64,
    taken: PathBuf,
) -> TestResult<wirecatch_engine::Result<Outcome>> {
    let progress = Arc::new(Progress::default());
    let task = tokio::spawn(download(req, progress.clone(), CancellationToken::new()));
    while progress.done() < at_least {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !taken.exists(),
        "a file is at the final name before the download finished"
    );
    std::fs::write(&taken, b"theirs")?;
    Ok(task.await?)
}

/// A slow server with a 2 MiB file, so a download is still running when another program saves its file.
async fn slow_server() -> TestServer {
    TestServer::start(
        test_data(2 * MIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(MIB),
            etag: Some("\"v1\"".into()),
            ..Default::default()
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_another_program_saves_meanwhile_is_never_replaced() {
    let server = slow_server().await;
    let dir = private_dir("taken-meanwhile").unwrap();
    let outcome = another_program_saves_meanwhile(
        request(&server, "t.bin", &dir, 2),
        256 * KIB,
        dir.join("t.bin"),
    )
    .await
    .unwrap()
    .unwrap();
    let Outcome::Completed { path, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(path, dir.join("t (1).bin"));
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_eq!(std::fs::read(dir.join("t.bin")).unwrap(), b"theirs");
    assert_eq!(files_in(&dir), ["t (1).bin", "t.bin"]);
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_continued_download_never_replaces_a_file_saved_meanwhile() {
    let server = slow_server().await;
    let dir = private_dir("taken-continued").unwrap();
    let paused = pause_midway(request(&server, "t.bin", &dir, 2), 256 * KIB)
        .await
        .unwrap();
    assert!(matches!(paused, Outcome::Paused { .. }), "{paused:?}");
    assert_eq!(files_in(&dir), ["t.bin.wcpart", "t.bin.wcstate"]);
    let outcome = another_program_saves_meanwhile(
        request(&server, "t.bin", &dir, 2),
        768 * KIB,
        dir.join("t.bin"),
    )
    .await
    .unwrap()
    .unwrap();
    let Outcome::Completed {
        path, resumed_from, ..
    } = outcome
    else {
        panic!("not completed: {outcome:?}")
    };
    assert!(
        resumed_from >= 256 * KIB,
        "the paused run was not continued"
    );
    assert_eq!(path, dir.join("t (1).bin"));
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert_eq!(std::fs::read(dir.join("t.bin")).unwrap(), b"theirs");
    dir.remove().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_exact_path_that_refuses_to_replace_keeps_the_data_for_later() {
    let server = slow_server().await;
    let dir = private_dir("taken-exact").unwrap();
    let path = dir.join("exact.bin");
    let refusing = || {
        let mut req = request(&server, "t.bin", &dir, 2);
        req.exact_path = Some(path.clone());
        req.if_exists = ExistingFile::Refuse;
        req
    };
    let error = another_program_saves_meanwhile(refusing(), 256 * KIB, path.clone())
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(&error, Error::TargetTaken { path: taken, .. } if *taken == path),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs");

    // Once the name is free, running the request again finishes from the data already there.
    std::fs::remove_file(&path).unwrap();
    let sent_before = server.stats.bytes_sent.load(Ordering::SeqCst);
    let outcome = run(refusing()).await.unwrap();
    let Outcome::Completed { resumed_from, .. } = outcome else {
        panic!("not completed: {outcome:?}")
    };
    assert_eq!(resumed_from, server.data.len() as u64);
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert!(server.stats.bytes_sent.load(Ordering::SeqCst) - sent_before < MIB);
    dir.remove().unwrap();
}
