//! The process hosting a `Manager` killed mid-download, then a new manager on the same data: the list comes back and every file ends byte for byte the original.
//!
//! The killed manager runs in a child process: this test binary, started again with only [`manager_in_a_child_process`], which does nothing unless the variables below are set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use wirecatch_core::{ComputerProxy, KnownFolders, Manager, ManagerConfig};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{AddRequest, DownloadStatus, DownloadSummary};
use wirecatch_test_support::faults::{
    FaultServer, Generated, Plan, Req, Running, Scratch, check_file, check_kept, fills_overlapping,
    kept_bytes, kept_now, wait_for,
};

use crate::common::{TestResult, is_running, row, tune, wait_until};

const ROOT_VARIABLE: &str = "WIRECATCH_FAULT_ROOT";
const ADD_VARIABLE: &str = "WIRECATCH_FAULT_ADD";
const MIB: u64 = 1024 * 1024;
const CONNECTIONS: u8 = 8;

async fn open(root: &Path) -> TestResult<Manager> {
    let folders = KnownFolders::under_home(root.join("home"));
    let mut config = ManagerConfig::new(root.join("data"), folders)
        .with_computer_proxy(ComputerProxy::Given(None));
    config.addon_sources.ytdlp_releases = None;
    Ok(Manager::open(config).await?)
}

fn settings(manager: &Manager) -> TestResult {
    tune(manager, |s| {
        s.connections_per_download = CONNECTIONS;
        s.extension.ask_before_download = false;
    })
}

async fn add(manager: &Manager, url: &str) -> TestResult<DownloadId> {
    Ok(manager
        .add_download(AddRequest {
            url: url.into(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        })
        .await?)
}

/// Opens a manager and keeps it running until killed, or until the parent is gone and standard input closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "started by the other tests of this module"]
async fn manager_in_a_child_process() -> TestResult {
    let Some(root) = std::env::var_os(ROOT_VARIABLE) else {
        return Ok(());
    };
    let manager = open(Path::new(&root)).await?;
    settings(&manager)?;
    for line in std::env::var(ADD_VARIABLE).unwrap_or_default().lines() {
        add(&manager, line).await?;
    }
    tokio::task::spawn_blocking(|| {
        let mut sink = String::new();
        while std::io::stdin().read_line(&mut sink).is_ok_and(|n| n > 0) {}
    })
    .await?;
    manager.shutdown().await;
    Ok(())
}

fn child(scratch: &Scratch, add: &[String]) -> Running {
    let mut command = Command::new(std::env::current_exe().unwrap_or_else(|e| panic!("{e}")));
    command
        .args([
            "--exact",
            "fault_kill::manager_in_a_child_process",
            "--ignored",
            "--nocapture",
        ])
        .env(ROOT_VARIABLE, &scratch.root)
        .env(ADD_VARIABLE, add.join("\n"))
        .env("WIRECATCH_LOG_DIR", &scratch.logs);
    Running::start(command, &scratch.logs, true)
}

/// Every file under `dir` whose name ends in `ending`.
fn find(dir: &Path, ending: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find(&path, ending));
        } else if path.to_string_lossy().ends_with(ending) {
            found.push(path);
        }
    }
    found
}

/// Every file under the home folder, sorted.
fn every_file(scratch: &Scratch) -> Vec<PathBuf> {
    let mut all = find(&scratch.root.join("home"), "");
    all.sort();
    all
}

fn names(scratch: &Scratch) -> Vec<String> {
    let mut names: Vec<_> = every_file(scratch)
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

async fn completed(manager: &Manager, id: DownloadId) -> DownloadSummary {
    wait_until(&format!("download {id:?} to complete"), || {
        let now = row(manager, id);
        assert_ne!(now.status, DownloadStatus::Failed, "{:?}", now.error);
        (now.status == DownloadStatus::Completed).then_some(now)
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_manager_continues_and_the_file_is_exact() -> TestResult {
    const LEN: u64 = 128 * MIB;
    let scratch = Scratch::new("manager-kill");
    let file = Generated::new(1, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    server.hold_at(LEN / 2);

    let mut run = child(&scratch, &[server.url("f.bin")]);
    let home = scratch.root.join("home");
    let kept = wait_for("kept bytes in the resume file", || {
        find(&home, ".wcstate")
            .first()
            .and_then(|state| kept_now(state, LEN))
    });
    run.kill();
    let state = find(&home, ".wcstate").remove(0);
    let kept = kept_now(&state, LEN).unwrap_or(kept);
    let part = find(&home, ".wcpart").remove(0);
    check_kept(&part, &kept, 1)?;

    let before = server.log().len();
    let manager = open(&scratch.root).await?;
    let rows = manager.list_downloads();
    let [first] = &rows[..] else {
        panic!("the list came back as {rows:?}");
    };
    assert!(
        is_running(first) || first.status == DownloadStatus::Queued,
        "it did not continue on its own: {:?}",
        first.status
    );
    // The list saves progress every 2 seconds, so its numbers can trail the resume file until the engine reads it.
    server.release();

    let done = completed(&manager, first.id).await;
    assert_eq!((done.done, done.total), (LEN, Some(LEN)));
    println!("sha256 {}", check_file(Path::new(&done.path), 1, LEN)?);
    let again = fills_overlapping(&server.log()[before..], &kept, LEN);
    assert!(again.is_empty(), "asked again for {again:?} of {kept:?}");
    assert_eq!(every_file(&scratch), [PathBuf::from(&done.path)]);
    manager.shutdown().await;
    Ok(())
}

/// A file of its own at each path.
fn per_path(req: &Req, len: u64, seeds: &BTreeMap<String, u64>) -> Plan {
    Generated::new(seeds[req.path.trim_start_matches('/')], len).ranged(req)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeated_kills_keep_the_list_and_every_file_right() -> TestResult {
    const LEN: u64 = 64 * MIB;
    const ROUNDS: u64 = 4;
    let scratch = Scratch::new("manager-storm");
    let seeds: BTreeMap<String, u64> = [
        ("a.bin", 11),
        ("b.bin", 12),
        ("c.bin", 13),
        ("paused.bin", 14),
    ]
    .map(|(name, seed)| (name.to_string(), seed))
    .into();
    let by_path = seeds.clone();
    let server = FaultServer::start(move |req| per_path(req, LEN, &by_path).paced(8 * MIB));

    // One paused with some of its file, by a manager that stops properly.
    let manager = open(&scratch.root).await?;
    settings(&manager)?;
    let paused = add(&manager, &server.url("paused.bin")).await?;
    wait_until("part of the file", || {
        (row(&manager, paused).done > 0).then_some(())
    })
    .await;
    manager.pause(paused)?;
    wait_until("the pause", || {
        (row(&manager, paused).status == DownloadStatus::Paused).then_some(())
    })
    .await;
    manager.shutdown().await;
    drop(manager);

    let urls: Vec<_> = ["a.bin", "b.bin", "c.bin"]
        .map(|name| server.url(name))
        .into();
    for round in 0..ROUNDS {
        let mut run = child(&scratch, if round == 0 { &urls } else { &[] });
        let from = server.sent();
        server.wait(&format!("round {round}"), |sent, _| {
            sent >= from + 3 * LEN / 8
        });
        run.kill();
        println!("round {round}: {} sent", server.sent());
    }

    let manager = open(&scratch.root).await?;
    let rows = manager.list_downloads();
    assert_eq!(rows.len(), 4, "{rows:?}");
    assert_eq!(
        row(&manager, paused).status,
        DownloadStatus::Paused,
        "a paused download started by itself"
    );
    for listed in &rows {
        if listed.id != paused {
            let done = completed(&manager, listed.id).await;
            let seed = seeds[&done.file_name];
            println!(
                "{}: sha256 {}",
                done.file_name,
                check_file(Path::new(&done.path), seed, LEN)?
            );
        }
    }
    manager.resume(paused)?;
    let done = completed(&manager, paused).await;
    println!(
        "paused: sha256 {}",
        check_file(Path::new(&done.path), 14, LEN)?
    );
    assert_eq!(names(&scratch), ["a.bin", "b.bin", "c.bin", "paused.bin"]);
    manager.shutdown().await;
    Ok(())
}

/// Kills land around the moment short downloads finish: between the rename and the list's save.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kills_during_a_burst_of_short_downloads_lose_or_double_nothing() -> TestResult {
    const LEN: u64 = 2 * MIB;
    const FILES: u64 = 60;
    const ROUNDS: u64 = 5;
    let scratch = Scratch::new("manager-burst");
    let seeds: BTreeMap<String, u64> = (0..FILES)
        .map(|n| (format!("f{n:02}.bin"), 100 + n))
        .collect();
    let by_path = seeds.clone();
    let server = FaultServer::start(move |req| per_path(req, LEN, &by_path).paced(32 * MIB));
    let urls: Vec<_> = seeds.keys().map(|name| server.url(name)).collect();

    for round in 0..ROUNDS {
        let mut run = child(&scratch, if round == 0 { &urls } else { &[] });
        let from = server.sent();
        server.wait(&format!("round {round}"), |sent, _| {
            sent >= from + FILES * LEN / (ROUNDS + 3)
        });
        run.kill();
        println!(
            "round {round}: {} sent, {} files",
            server.sent(),
            every_file(&scratch).len()
        );
    }

    let manager = open(&scratch.root).await?;
    let rows = manager.list_downloads();
    assert_eq!(rows.len() as u64, FILES, "{rows:?}");
    for listed in &rows {
        let done = completed(&manager, listed.id).await;
        let asked = done.url.rsplit('/').next().unwrap_or_default();
        check_file(Path::new(&done.path), seeds[asked], LEN)?;
    }
    assert_eq!(names(&scratch), seeds.keys().cloned().collect::<Vec<_>>());
    manager.shutdown().await;
    Ok(())
}

/// Killed while the row's path is still a guess, then with the path known and no byte yet: the server's name wins and one file comes of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_before_the_server_answered_one_file_comes_of_it() -> TestResult {
    const LEN: u64 = 8 * MIB;
    for (case, hold_heads) in [("no-answer", true), ("no-byte", false)] {
        let scratch = Scratch::new(case);
        let file = Generated::new(20, LEN);
        let server = FaultServer::start(move |req| {
            file.ranged(req)
                .header("Content-Disposition", "attachment; filename=\"report.pdf\"")
        });
        if hold_heads {
            server.hold_heads();
        } else {
            server.hold_at(0);
        }
        let mut run = child(&scratch, &[server.url("get?id=1")]);
        server.wait("the first request", |_, requests| requests >= 1);
        if !hold_heads {
            wait_for("the part file", || {
                find(&scratch.root.join("home"), ".wcpart")
                    .first()
                    .map(|_| ())
            });
        }
        run.kill();
        server.release();

        let manager = open(&scratch.root).await?;
        let rows = manager.list_downloads();
        // The list is saved off the path of a run, so a kill right after the first request can land before the row is on disk: then nothing of the download may be left either.
        if hold_heads && rows.is_empty() {
            assert_eq!(every_file(&scratch), [] as [PathBuf; 0], "{case}");
            manager.shutdown().await;
            continue;
        }
        let [first] = &rows[..] else {
            panic!("{case}: the list came back as {rows:?}");
        };
        let done = completed(&manager, first.id).await;
        assert_eq!(done.file_name, "report.pdf", "{case}");
        println!(
            "{case}: sha256 {}",
            check_file(Path::new(&done.path), 20, LEN)?
        );
        assert_eq!(every_file(&scratch), [PathBuf::from(&done.path)], "{case}");
        manager.shutdown().await;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_then_the_file_changes_the_new_version_is_saved_whole() -> TestResult {
    const LEN: u64 = 64 * MIB;
    let scratch = Scratch::new("manager-changed");
    let file = Generated::new(30, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    server.hold_at(LEN / 2);
    let mut run = child(&scratch, &[server.url("f.bin")]);
    let home = scratch.root.join("home");
    wait_for("kept bytes in the resume file", || {
        find(&home, ".wcstate")
            .first()
            .and_then(|state| kept_now(state, LEN))
    });
    run.kill();
    let new = Generated::new(31, LEN);
    server.set_handler(move |req| new.ranged(req));
    server.release();

    let manager = open(&scratch.root).await?;
    let rows = manager.list_downloads();
    let [first] = &rows[..] else {
        panic!("the list came back as {rows:?}");
    };
    let done = completed(&manager, first.id).await;
    println!("sha256 {}", check_file(Path::new(&done.path), 31, LEN)?);
    assert_eq!(every_file(&scratch), [PathBuf::from(&done.path)]);
    manager.shutdown().await;
    Ok(())
}

/// The server fails after the restart: the download stops with its error, and retrying continues from what was kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_then_the_server_fails_then_a_retry_continues() -> TestResult {
    const LEN: u64 = 64 * MIB;
    let scratch = Scratch::new("manager-fail");
    let file = Generated::new(40, LEN);
    let honest = file.clone();
    let server = FaultServer::start(move |req| file.ranged(req));
    server.hold_at(LEN / 2);
    let mut run = child(&scratch, &[server.url("f.bin")]);
    let home = scratch.root.join("home");
    let kept = wait_for("kept bytes in the resume file", || {
        find(&home, ".wcstate")
            .first()
            .and_then(|state| kept_now(state, LEN))
    });
    run.kill();
    let state = find(&home, ".wcstate").remove(0);
    let kept = kept_now(&state, LEN).unwrap_or(kept);
    server.release();
    server.set_handler(|_| Plan {
        status: 404,
        headers: vec![("Content-Length".into(), "0".into())],
        seed: 40,
        from: 0,
        send: 0,
        chunked: false,
        pace: None,
        cut_off: false,
    });

    let manager = open(&scratch.root).await?;
    let id = manager.list_downloads().remove(0).id;
    let failed = wait_until("the failure", || {
        let now = row(&manager, id);
        (now.status == DownloadStatus::Failed).then_some(now)
    })
    .await;
    println!("failed: {:?}", failed.error);
    let still = kept_now(&state, LEN).ok_or("the resume file is gone")?;
    assert!(
        kept_bytes(&still) >= kept_bytes(&kept),
        "{still:?} < {kept:?}"
    );

    let before = server.log().len();
    server.set_handler(move |req| honest.ranged(req));
    manager.retry(id)?;
    let done = completed(&manager, id).await;
    println!("sha256 {}", check_file(Path::new(&done.path), 40, LEN)?);
    let again = fills_overlapping(&server.log()[before..], &kept, LEN);
    assert!(again.is_empty(), "asked again for {again:?} of {kept:?}");
    assert_eq!(every_file(&scratch), [PathBuf::from(&done.path)]);
    manager.shutdown().await;
    Ok(())
}

/// The resume file is unreadable after a kill: the download starts over and nothing of the earlier run stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_with_a_torn_resume_file_the_download_starts_over_at_its_path() -> TestResult {
    const LEN: u64 = 64 * MIB;
    let scratch = Scratch::new("manager-torn");
    let file = Generated::new(50, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    server.hold_at(LEN / 2);
    let mut run = child(&scratch, &[server.url("f.bin")]);
    let home = scratch.root.join("home");
    wait_for("kept bytes in the resume file", || {
        find(&home, ".wcstate")
            .first()
            .and_then(|state| kept_now(state, LEN))
    });
    run.kill();
    server.release();
    let state = find(&home, ".wcstate").remove(0);
    let whole = std::fs::read(&state)?;
    std::fs::write(&state, &whole[..whole.len() / 2])?;

    let manager = open(&scratch.root).await?;
    let id = manager.list_downloads().remove(0).id;
    let done = completed(&manager, id).await;
    println!("sha256 {}", check_file(Path::new(&done.path), 50, LEN)?);
    assert_eq!(done.file_name, "f.bin");
    assert_eq!(every_file(&scratch), [PathBuf::from(&done.path)]);
    manager.shutdown().await;
    Ok(())
}
