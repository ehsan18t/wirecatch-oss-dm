//! Servers whose byte ranges do not work as advertised, through the real command line.

use std::time::Duration;

use wirecatch_test_support::PATIENCE;
use wirecatch_test_support::faults::{Ended, FaultServer, Generated, Plan, Scratch, cli, safe_end};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const LEN: u64 = 32 * 1024 * 1024;
const BOUND: Duration = PATIENCE.saturating_mul(2);

fn run(name: &str, server: &FaultServer, seed: u64) -> Ended {
    let scratch = Scratch::new(name);
    let ran = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    let ended = safe_end(&ran, &scratch, "f.bin", seed, LEN);
    if ended == Ended::Exact {
        assert_eq!(scratch.files(), ["f.bin"], "partial files left behind");
    }
    ended
}

#[test]
fn ranges_advertised_but_every_answer_is_the_whole_file() {
    let file = Generated::new(1, LEN);
    let server = FaultServer::start(move |_| file.whole().header("Accept-Ranges", "bytes"));
    assert_eq!(run("advertised", &server, 1), Ended::Exact);
}

#[test]
fn the_first_answer_is_partial_and_later_ones_are_whole() {
    let file = Generated::new(2, LEN);
    let server = FaultServer::start(move |req| {
        if req.no == 0 {
            file.ranged(req)
        } else {
            file.whole()
        }
    });
    // D34.3: a whole answer to a range request is `RangeNotHonored`, which stops the run.
    assert_eq!(run("then-whole", &server, 2), Ended::Refused);
}

#[test]
fn partial_answers_start_at_another_offset_than_asked() {
    let file = Generated::new(3, LEN);
    let server = FaultServer::start(move |req| {
        let Some((start, end)) = req.range else {
            return file.ranged(req);
        };
        if start == 0 {
            return file.ranged(req);
        }
        // One MiB before what was asked, with headers that say so.
        let moved = start - (1024 * 1024).min(start);
        let mut asked = req.clone();
        asked.range = Some((moved, end));
        file.ranged(&asked)
    });
    // Either ending is right: `safe_end` holds it to the rule.
    run("other-offset", &server, 3);
}

#[test]
fn partial_answers_without_a_content_range_header() {
    let file = Generated::new(4, LEN);
    let server = FaultServer::start(move |req| {
        let plan: Plan = file.ranged(req);
        if req.no == 0 {
            plan
        } else {
            plan.without("Content-Range")
        }
    });
    // Either ending is right: `safe_end` holds it to the rule.
    run("no-content-range", &server, 4);
}
