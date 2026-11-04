//! Files over 4 GiB, through the real command line. Each test holds about 4.2 GiB on disk and removes it at once, so run them one at a time: `--test-threads=1 --ignored`.

use std::time::Duration;

use wirecatch_test_support::PATIENCE;
use wirecatch_test_support::faults::{
    Ended, FaultServer, Generated, Req, Scratch, check_kept, cli, fills_overlapping, kept_now,
    safe_end, wait_for,
};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const MIB: u64 = 1024 * 1024;
const FOUR_GIB: u64 = 4096 * MIB;
const LEN: u64 = FOUR_GIB + 123 * MIB + 45_678;
const NAME: &str = "large.bin";
const BOUND: Duration = PATIENCE.saturating_mul(10);

fn saved(scratch: &Scratch, server: &FaultServer, seed: u64) -> String {
    let ran = cli(EXE, scratch, &server.url(NAME)).ended(BOUND);
    assert_eq!(
        safe_end(&ran, scratch, NAME, seed, LEN),
        Ended::Exact,
        "{}",
        ran.stderr
    );
    assert_eq!(scratch.files(), [NAME]);
    ran.stdout
}

#[test]
#[ignore = "needs 4.2 GiB of disk"]
fn over_4_gib_over_parallel_connections() {
    let scratch = Scratch::new("large-parallel");
    let file = Generated::new(1, LEN);
    let server = FaultServer::start(move |req| file.ranged(req));
    saved(&scratch, &server, 1);
}

#[test]
#[ignore = "needs 4.2 GiB of disk"]
fn over_4_gib_without_range_support() {
    let scratch = Scratch::new("large-whole");
    let file = Generated::new(2, LEN);
    let server = FaultServer::start(move |_| file.whole());
    saved(&scratch, &server, 2);
}

#[test]
#[ignore = "needs 4.2 GiB of disk"]
fn over_4_gib_with_an_unknown_size() {
    let scratch = Scratch::new("large-chunked");
    let file = Generated::new(3, LEN);
    let server = FaultServer::start(move |_| file.chunked());
    saved(&scratch, &server, 3);
}

/// A connection for a range above 4 GiB drops once, then the process is killed, and the next run continues. At 5 GiB over 8 connections, parts start above 4 GiB, so requests, writes and the resume file all hold offsets 32 bits cannot.
#[test]
#[ignore = "needs 5 GiB of disk"]
fn over_4_gib_dropped_above_the_boundary_then_killed_then_continued() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    const LARGER: u64 = 5120 * MIB + 45_678;
    let scratch = Scratch::new("large-kill");
    let file = Generated::new(4, LARGER);
    let dropped = Arc::new(AtomicBool::new(false));
    let once = Arc::clone(&dropped);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.ranged(req);
        if req.range.is_some_and(|(start, _)| start > FOUR_GIB)
            && !once.swap(true, Ordering::SeqCst)
        {
            plan.send = plan.send.min(MIB);
        }
        plan
    });
    server.hold_at(LARGER / 2);
    let state = scratch.downloads.join("large.bin.wcstate");
    let mut run = cli(EXE, &scratch, &server.url(NAME));
    server.wait("half the file", |sent, _| sent >= LARGER / 2);
    let kept = wait_for("kept bytes above 4 GiB", || {
        kept_now(&state, LARGER).filter(|kept| kept.iter().any(|&(start, _)| start > FOUR_GIB))
    });
    run.kill();
    assert!(
        dropped.load(Ordering::SeqCst),
        "no request started above 4 GiB"
    );
    let kept = kept_now(&state, LARGER).unwrap_or(kept);
    check_kept(&scratch.downloads.join("large.bin.wcpart"), &kept, 4).unwrap();
    server.release();
    let before = server.log().len();
    let ran = cli(EXE, &scratch, &server.url(NAME)).ended(BOUND);
    assert_eq!(
        safe_end(&ran, &scratch, NAME, 4, LARGER),
        Ended::Exact,
        "{}",
        ran.stderr
    );
    assert_eq!(scratch.files(), [NAME]);
    assert!(ran.stdout.contains("resumed"), "{}", ran.stdout);
    let fills = &server.log()[before..];
    let again = fills_overlapping(fills, &kept, LARGER);
    assert!(again.is_empty(), "asked again for {again:?} of {kept:?}");
    assert!(
        fills
            .iter()
            .any(|s| s.req.range.is_some_and(|(start, _)| start > FOUR_GIB)),
        "the second run asked for nothing above 4 GiB"
    );
}
