//! The command line killed mid-download, then run again: the file must be byte for byte the original, and what was kept must not be fetched again.

use std::path::PathBuf;
use std::time::Duration;

use wirecatch_test_support::PATIENCE;
use wirecatch_test_support::faults::{
    Ended, FaultServer, Generated, Plan, Ran, Req, Running, Scratch, check_kept, cli,
    fills_overlapping, kept_bytes, kept_now, safe_end, wait_for,
};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const MIB: u64 = 1024 * 1024;
const LEN: u64 = 64 * MIB;
const NAME: &str = "f.bin";
const BOUND: Duration = PATIENCE.saturating_mul(2);

fn part(scratch: &Scratch) -> PathBuf {
    scratch.downloads.join("f.bin.wcpart")
}

fn state(scratch: &Scratch) -> PathBuf {
    scratch.downloads.join("f.bin.wcstate")
}

fn start(scratch: &Scratch, server: &FaultServer) -> Running {
    cli(EXE, scratch, &server.url(NAME))
}

/// Runs until half the file went out and the resume file says part of it is on disk, then kills the process and checks what the resume file claims.
fn killed_at_half(scratch: &Scratch, server: &FaultServer, seed: u64) -> Vec<(u64, u64)> {
    server.hold_at(LEN / 2);
    let mut run = start(scratch, server);
    let kept = wait_for("kept bytes in the resume file", || {
        kept_now(&state(scratch), LEN)
    });
    run.kill();
    // The last save before the kill, which may be newer than the one seen.
    let kept = kept_now(&state(scratch), LEN).unwrap_or(kept);
    if let Err(e) = check_kept(&part(scratch), &kept, seed) {
        panic!("{e}");
    }
    server.release();
    kept
}

fn ended(running: Running) -> Ran {
    match running.wait(BOUND) {
        Ok(ran) => ran,
        Err(e) => panic!("the run hung: {e}"),
    }
}

fn finish(scratch: &Scratch, server: &FaultServer, seed: u64) -> Ran {
    let ran = ended(start(scratch, server));
    assert_eq!(
        safe_end(&ran, scratch, NAME, seed, LEN),
        Ended::Exact,
        "{}",
        ran.stderr
    );
    assert_eq!(scratch.files(), [NAME], "partial files left behind");
    ran
}

#[test]
fn killed_mid_download_the_next_run_continues_to_the_exact_file() {
    let scratch = Scratch::new("kill");
    let file = Generated::new(1, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    let kept = killed_at_half(&scratch, &server, 1);
    let before = server.log().len();
    let ran = finish(&scratch, &server, 1);
    assert!(ran.stdout.contains("resumed"), "{}", ran.stdout);
    let again = fills_overlapping(&server.log()[before..], &kept, LEN);
    assert!(again.is_empty(), "asked again for {again:?} of {kept:?}");
}

/// Killed while data flows freely, over and over, so kills land in writes and in saves of the resume file.
#[test]
fn killed_again_and_again_while_data_flows() {
    const LARGE: u64 = 512 * MIB;
    const ROUNDS: u64 = 6;
    let scratch = Scratch::new("storm");
    let file = Generated::new(2, LARGE);
    let server = FaultServer::start(move |req: &Req| file.ranged(req).paced(4 * MIB));
    let mut most = 0;
    for round in 1..=ROUNDS {
        let mut run = start(&scratch, &server);
        let from = server.sent();
        server.wait(&format!("round {round}"), |sent, _| {
            sent >= from + LARGE / (ROUNDS + 2)
        });
        run.kill();
        if let Some(kept) = kept_now(&state(&scratch), LARGE) {
            check_kept(&part(&scratch), &kept, 2).unwrap();
            let now = kept_bytes(&kept);
            assert!(now >= most, "round {round}: kept {now} after {most}");
            println!("round {round}: {now} bytes kept, {} sent", server.sent());
            most = now;
        }
    }
    assert!(most > 0, "no kill came after a save");
    let ran = ended(start(&scratch, &server));
    assert_eq!(
        safe_end(&ran, &scratch, NAME, 2, LARGE),
        Ended::Exact,
        "{}",
        ran.stderr
    );
    assert_eq!(scratch.files(), [NAME], "partial files left behind");
}

#[test]
fn killed_before_any_byte_arrived_the_next_run_saves_the_same_name() {
    let scratch = Scratch::new("early");
    let file = Generated::new(3, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    server.hold_at(0);
    let mut run = start(&scratch, &server);
    server.wait("the first request", |_, requests| requests >= 1);
    wait_for("the part file", || part(&scratch).exists().then_some(()));
    run.kill();
    server.release();
    finish(&scratch, &server, 3);
}

#[test]
fn killed_without_range_support_the_next_run_starts_over_to_the_exact_file() {
    let scratch = Scratch::new("whole");
    let file = Generated::new(4, LEN);
    let server = FaultServer::start(move |_| file.whole());
    server.hold_at(LEN / 2);
    let mut run = start(&scratch, &server);
    server.wait("half the file", |sent, _| sent >= LEN / 2);
    wait_for("the part file", || part(&scratch).exists().then_some(()));
    run.kill();
    println!("left by the kill: {:?}", scratch.files());
    server.release();
    finish(&scratch, &server, 4);
}

#[test]
fn killed_with_an_unknown_size_the_next_run_starts_over_to_the_exact_file() {
    let scratch = Scratch::new("chunked");
    let file = Generated::new(5, LEN);
    let server = FaultServer::start(move |_| file.chunked());
    server.hold_at(LEN / 2);
    let mut run = start(&scratch, &server);
    server.wait("half the file", |sent, _| sent >= LEN / 2);
    run.kill();
    println!("left by the kill: {:?}", scratch.files());
    server.release();
    finish(&scratch, &server, 5);
}

#[test]
fn killed_then_the_file_changes_the_next_run_saves_the_new_version() {
    let scratch = Scratch::new("changed");
    let file = Generated::new(6, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    killed_at_half(&scratch, &server, 6);
    let new = Generated::new(7, LEN);
    server.set_handler(move |req| new.ranged(req));
    let ran = finish(&scratch, &server, 7);
    assert!(!ran.stdout.contains("resumed"), "{}", ran.stdout);
}

/// What a power cut can leave after a kill: a resume file newer than the data, here a part file cut to a quarter.
#[test]
fn a_part_file_shorter_than_the_resume_file_says() {
    let scratch = Scratch::new("cut");
    let file = Generated::new(8, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    killed_at_half(&scratch, &server, 8);
    let cut = std::fs::OpenOptions::new()
        .write(true)
        .open(part(&scratch))
        .unwrap();
    cut.set_len(LEN / 4).unwrap();
    drop(cut);
    finish(&scratch, &server, 8);
}

#[test]
fn a_temporary_resume_file_left_by_a_kill_is_cleaned_up() {
    let scratch = Scratch::new("tmp");
    let file = Generated::new(9, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    killed_at_half(&scratch, &server, 9);
    std::fs::write(
        scratch.downloads.join("f.bin.wcstate.tmp"),
        b"{\"version\":1,\"ur",
    )
    .unwrap();
    finish(&scratch, &server, 9);
}

/// Runs to the end after the resume file was damaged: the file must come out at its own name and nothing of the earlier run may stay behind.
fn starts_over_at_the_same_name(scratch: &Scratch, server: &FaultServer, seed: u64) {
    let ran = ended(start(scratch, server));
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert!(!ran.stdout.contains("resumed"), "{}", ran.stdout);
    assert_eq!(
        scratch.files(),
        [NAME],
        "another name, or partial files left behind"
    );
    assert_eq!(safe_end(&ran, scratch, NAME, seed, LEN), Ended::Exact);
}

#[test]
fn a_resume_file_torn_in_half_starts_over_at_the_same_name() {
    let scratch = Scratch::new("torn");
    let file = Generated::new(10, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    killed_at_half(&scratch, &server, 10);
    let whole = std::fs::read(state(&scratch)).unwrap();
    std::fs::write(state(&scratch), &whole[..whole.len() / 2]).unwrap();
    starts_over_at_the_same_name(&scratch, &server, 10);
}

#[test]
fn a_part_file_with_no_resume_file_starts_over_at_the_same_name() {
    let scratch = Scratch::new("nostate");
    let file = Generated::new(11, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    killed_at_half(&scratch, &server, 11);
    std::fs::remove_file(state(&scratch)).unwrap();
    starts_over_at_the_same_name(&scratch, &server, 11);
}

/// The new version is shorter or longer than the part file left by the kill: nothing of the old size may stay.
#[test]
fn killed_then_the_file_changes_size_the_next_run_saves_the_new_version() {
    for (case, len) in [("shrunk", LEN / 2), ("grown", LEN + 3 * MIB)] {
        let scratch = Scratch::new(case);
        let file = Generated::new(13, LEN);
        let server = FaultServer::start(move |req| file.ranged(req));
        killed_at_half(&scratch, &server, 13);
        let new = Generated::new(14, len);
        server.set_handler(move |req| new.ranged(req));
        let ran = ended(start(&scratch, &server));
        assert_eq!(
            safe_end(&ran, &scratch, NAME, 14, len),
            Ended::Exact,
            "{case}: {}",
            ran.stderr
        );
        assert_eq!(scratch.files(), [NAME], "{case}");
    }
}

#[test]
fn a_failed_run_after_a_kill_keeps_what_was_kept() {
    let scratch = Scratch::new("fail-then");
    let file = Generated::new(12, LEN);
    let honest = file.clone();
    let server = FaultServer::start(move |req| file.ranged(req));
    let kept = killed_at_half(&scratch, &server, 12);
    server.set_handler(|_| Plan {
        status: 404,
        headers: vec![("Content-Length".into(), "0".into())],
        seed: 12,
        from: 0,
        send: 0,
        chunked: false,
        pace: None,
        cut_off: false,
    });
    let failed = ended(start(&scratch, &server));
    assert_ne!(failed.code, Some(0));
    let still = kept_now(&state(&scratch), LEN).expect("the resume file is gone");
    assert!(
        kept_bytes(&still) >= kept_bytes(&kept),
        "{still:?} < {kept:?}"
    );
    server.set_handler(move |req| honest.ranged(req));
    let ran = finish(&scratch, &server, 12);
    assert!(ran.stdout.contains("resumed"), "{}", ran.stdout);
}
