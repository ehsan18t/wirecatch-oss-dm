//! Offsets above 4 GiB through the real command line, on a sparse part file, so no test needs the disk space or the time of a whole file: an earlier run is set up that says it has everything below the boundary, and only the bytes above it travel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wirecatch_test_support::faults::{
    FaultServer, Generated, Req, Scratch, check_kept, cli, kept_now, wait_for, write_resume_file,
};
use wirecatch_test_support::{PATIENCE, TestResult};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const MIB: u64 = 1024 * 1024;
const FOUR_GIB: u64 = 4096 * MIB;
const TAIL: u64 = 64 * MIB;
/// Past the boundary by a little more than the bytes that travel, and not on a whole number of MiB.
const LEN: u64 = FOUR_GIB + TAIL + 45_678;
const FETCH_FROM: u64 = FOUR_GIB + 12_345;
const NAME: &str = "large.bin";
const BOUND: Duration = PATIENCE.saturating_mul(2);

/// A part file of `LEN` bytes, sparse where the system allows, with a resume file that says only the bytes from `FETCH_FROM` are missing.
fn earlier_run(scratch: &Scratch, server: &FaultServer, file: &Generated) -> TestResult {
    let part = scratch.downloads.join("large.bin.wcpart");
    std::fs::File::create(&part)?.set_len(LEN)?;
    write_resume_file(
        &scratch.downloads.join("large.bin.wcstate"),
        &server.url(NAME),
        file,
        &[(FETCH_FROM, LEN)],
    )?;
    Ok(())
}

/// The file is at its final name at its full size, with the right bytes where they were fetched, and nothing below `FETCH_FROM` was asked for after the probe.
fn assert_tail_saved(scratch: &Scratch, seed: u64, stdout: &str) -> TestResult {
    assert!(stdout.contains("resumed"), "{stdout}");
    assert_eq!(scratch.files(), [NAME]);
    let path = scratch.downloads.join(NAME);
    assert_eq!(std::fs::metadata(&path)?.len(), LEN);
    if let Err(e) = check_kept(&path, &[(FETCH_FROM, LEN)], seed) {
        panic!("{e}");
    }
    Ok(())
}

fn asked_from(req: &Req) -> u64 {
    req.range.map_or(0, |(start, _)| start)
}

#[test]
fn a_run_continues_above_4_gib_over_parallel_connections() -> TestResult {
    let scratch = Scratch::new("large-parallel");
    let file = Generated::new(1, LEN);
    let answers = file.clone();
    let server = FaultServer::start(move |req| answers.ranged(req));
    earlier_run(&scratch, &server, &file)?;
    let ran = cli(EXE, &scratch, &server.url(NAME)).ended(BOUND);
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert_tail_saved(&scratch, 1, &ran.stdout)?;
    let fills: Vec<u64> = server.log()[1..]
        .iter()
        .map(|seen| asked_from(&seen.req))
        .collect();
    assert!(
        fills.len() > 1,
        "one connection did all the work: {fills:?}"
    );
    assert!(
        fills.iter().all(|&start| start >= FETCH_FROM),
        "asked again below the bytes missing: {fills:?}"
    );
    Ok(())
}

/// A connection drops above the boundary, the process is killed, and what the resume file then says above 4 GiB is true and is not fetched again.
#[test]
fn a_run_dropped_and_killed_above_4_gib_continues_to_the_end() -> TestResult {
    let scratch = Scratch::new("large-kill");
    let file = Generated::new(2, LEN);
    let answers = file.clone();
    let dropped = Arc::new(AtomicBool::new(false));
    let once = Arc::clone(&dropped);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = answers.ranged(req);
        if asked_from(req) > FOUR_GIB && !once.swap(true, Ordering::SeqCst) {
            plan.send = plan.send.min(MIB);
        }
        plan
    });
    earlier_run(&scratch, &server, &file)?;
    server.hold_at(TAIL / 2);
    let state = scratch.downloads.join("large.bin.wcstate");
    let part = scratch.downloads.join("large.bin.wcpart");
    let mut run = cli(EXE, &scratch, &server.url(NAME));
    let above = |kept: Vec<(u64, u64)>| -> Vec<(u64, u64)> {
        kept.into_iter()
            .map(|(start, end)| (start.max(FETCH_FROM), end))
            .filter(|&(start, end)| start < end)
            .collect()
    };
    let kept = wait_for("kept bytes above 4 GiB", || {
        kept_now(&state, LEN)
            .map(above)
            .filter(|kept| !kept.is_empty())
    });
    run.kill();
    assert!(dropped.load(Ordering::SeqCst), "no connection was dropped");
    let kept = kept_now(&state, LEN).map_or(kept, above);
    if let Err(e) = check_kept(&part, &kept, 2) {
        panic!("{e}");
    }
    server.release();

    let before = server.log().len();
    let ran = cli(EXE, &scratch, &server.url(NAME)).ended(BOUND);
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert_tail_saved(&scratch, 2, &ran.stdout)?;
    let log = server.log();
    let again: Vec<u64> = log[before + 1..]
        .iter()
        .map(|seen| asked_from(&seen.req))
        .filter(|start| kept.iter().any(|&(from, to)| from <= *start && *start < to))
        .collect();
    assert!(
        again.is_empty(),
        "asked again from {again:?}, kept {kept:?}"
    );
    Ok(())
}
