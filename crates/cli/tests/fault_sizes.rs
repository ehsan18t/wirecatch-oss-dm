//! Servers that say a wrong size, through the real command line.

use std::time::Duration;

use wirecatch_test_support::PATIENCE;
use wirecatch_test_support::faults::{Ended, FaultServer, Generated, Req, Scratch, cli, safe_end};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const MIB: u64 = 1024 * 1024;
const LEN: u64 = 16 * MIB;
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
fn content_length_larger_than_the_body_once() {
    let file = Generated::new(1, LEN);
    let server = FaultServer::start(move |req| {
        let mut plan = file.ranged(req);
        if req.no == 0 {
            plan.send = LEN / 3;
        }
        plan
    });
    assert_eq!(run("short-once", &server, 1), Ended::Exact);
}

#[test]
fn content_length_larger_than_the_body_once_without_ranges() {
    let file = Generated::new(2, LEN);
    let server = FaultServer::start(move |req| {
        let mut plan = file.whole();
        if req.no == 0 {
            plan.send = LEN / 3;
        }
        plan
    });
    assert_eq!(run("short-once-whole", &server, 2), Ended::Exact);
}

#[test]
fn content_length_larger_than_the_body_every_time_is_refused() {
    let file = Generated::new(3, LEN);
    let server = FaultServer::start(move |_| {
        let mut plan = file.whole();
        plan.send = LEN / 3;
        plan
    });
    assert_eq!(run("short-always", &server, 3), Ended::Refused);
}

/// The server says `CLAIM` bytes but has only `LEN`: its answers end at the real end, cut off, and past it there is a 416.
#[test]
fn a_size_larger_than_the_real_file_is_refused() {
    const CLAIM: u64 = LEN + 2 * MIB;
    let file = Generated::new(4, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let Some((start, end)) = req.range else {
            return file.ranged(req);
        };
        if start >= LEN {
            return file
                .ranged(req)
                .header("Content-Range", format!("bytes */{LEN}"));
        }
        let end = end.unwrap_or(CLAIM).min(CLAIM);
        let mut asked = req.clone();
        asked.range = Some((start, Some(end.min(LEN))));
        let mut plan = file.ranged(&asked);
        plan.send = end.min(LEN) - start;
        plan.header(
            "Content-Range",
            format!("bytes {start}-{}/{CLAIM}", end - 1),
        )
        .header("Content-Length", (end - start).to_string())
    });
    assert_eq!(run("claim-larger", &server, 4), Ended::Refused);
}

/// The same, with answers that end cleanly at the real end, and a past-the-end answer that keeps to the claimed size.
#[test]
fn a_size_larger_than_the_real_file_with_clean_answers_is_refused() {
    const CLAIM: u64 = LEN + 2 * MIB;
    let file = Generated::new(5, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let Some((start, end)) = req.range else {
            return file.ranged(req);
        };
        if start >= LEN {
            return file
                .ranged(req)
                .header("Content-Range", format!("bytes */{CLAIM}"));
        }
        let end = end.unwrap_or(CLAIM).min(LEN);
        let mut asked = req.clone();
        asked.range = Some((start, Some(end)));
        file.ranged(&asked).header(
            "Content-Range",
            format!("bytes {start}-{}/{CLAIM}", end - 1),
        )
    });
    assert_eq!(run("claim-larger-clean", &server, 5), Ended::Refused);
}

#[test]
fn the_size_in_later_answers_differs_from_the_first() {
    let file = Generated::new(6, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let plan = file.ranged(req);
        match req.range {
            Some((start, end)) if req.no > 0 => {
                let end = end.unwrap_or(LEN).min(LEN);
                plan.header(
                    "Content-Range",
                    format!("bytes {start}-{}/{}", end - 1, LEN + MIB),
                )
            }
            _ => plan,
        }
    });
    assert_eq!(run("total-differs", &server, 6), Ended::Refused);
}

/// A range request answered to the end of the file, as servers that ignore the last byte of a range do.
#[test]
fn answers_longer_than_the_range_asked_for() {
    let file = Generated::new(7, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut open = req.clone();
        open.range = req.range.map(|(start, _)| (start, None));
        file.ranged(&open)
    });
    assert_eq!(run("longer-answers", &server, 7), Ended::Exact);
}

#[test]
fn every_answer_is_short_with_matching_headers() {
    const PIECE: u64 = 256 * 1024;
    let file = Generated::new(8, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut short = req.clone();
        let start = req.range.map_or(0, |(start, _)| start);
        short.range = Some((start, Some((start + PIECE).min(LEN))));
        file.ranged(&short)
    });
    assert_eq!(run("always-short", &server, 8), Ended::Exact);
}

#[test]
fn content_range_promises_more_than_content_length() {
    let file = Generated::new(9, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.ranged(req);
        if plan.status == 206 && plan.send > 1024 {
            plan.send /= 2;
            let send = plan.send;
            plan = plan.header("Content-Length", send.to_string());
        }
        plan
    });
    assert_eq!(run("range-over-length", &server, 9), Ended::Exact);
}

/// The body runs on past the range its headers name, even past the end of the file.
#[test]
fn content_length_longer_than_content_range() {
    let file = Generated::new(10, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.ranged(req);
        if plan.status == 206 {
            plan.send += MIB;
            let send = plan.send;
            plan = plan.header("Content-Length", send.to_string());
        }
        plan
    });
    assert_eq!(run("length-over-range", &server, 10), Ended::Exact);
}

#[test]
fn a_chunked_answer_cut_off_once() {
    let file = Generated::new(11, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.chunked();
        if req.no == 0 {
            plan.send = LEN / 3;
            plan.cut_off = true;
        }
        plan
    });
    assert_eq!(run("chunked-once", &server, 11), Ended::Exact);
}

/// Nothing ties the retry to the first answer, so the run starts over from byte 0 rather than joining them.
#[test]
fn a_chunked_answer_without_validators_cut_off_once_starts_over() {
    let file = Generated::new(16, LEN).without_validators();
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.chunked();
        if req.no == 0 {
            plan.send = LEN / 3;
            plan.cut_off = true;
        }
        plan
    });
    assert_eq!(run("chunked-once-untagged", &server, 16), Ended::Exact);
}

/// Starting over never gets further than the last attempt, which is not progress: the run ends.
#[test]
fn a_chunked_answer_without_validators_cut_off_every_time_is_refused() {
    let file = Generated::new(17, LEN).without_validators();
    let server = FaultServer::start(move |_| {
        let mut plan = file.chunked();
        plan.send = LEN / 3;
        plan.cut_off = true;
        plan
    });
    assert_eq!(run("chunked-always-untagged", &server, 17), Ended::Refused);
}

#[test]
fn a_chunked_answer_cut_off_every_time_is_refused() {
    let file = Generated::new(12, LEN);
    let server = FaultServer::start(move |_| {
        let mut plan = file.chunked();
        plan.send = LEN / 3;
        plan.cut_off = true;
        plan
    });
    assert_eq!(run("chunked-always", &server, 12), Ended::Refused);
}

/// Partial answers with matching headers and no bytes at all, forever: the run has to end.
#[test]
fn empty_partial_answers_forever_end_the_run() {
    let file = Generated::new(13, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.ranged(req);
        if req.no == 0 {
            plan.send = MIB;
        } else if plan.status == 206 {
            plan.send = 0;
            plan = plan.header("Content-Length", "0");
        }
        plan
    });
    assert_eq!(run("empty-answers", &server, 13), Ended::Refused);
}

/// A partial answer may be shorter than asked (RFC 9110, section 15.3.7) and its headers say where it ends: asking for the rest is normal work, not a failure to wait out.
#[test]
fn a_clean_short_answer_is_not_treated_as_a_failure() {
    const PIECE: u64 = 256 * 1024;
    let scratch = Scratch::new("short-clean");
    let file = Generated::new(14, LEN);
    let server = FaultServer::start(move |req: &Req| {
        let mut short = req.clone();
        let start = req.range.map_or(0, |(start, _)| start);
        short.range = Some((start, Some((start + PIECE).min(LEN))));
        file.ranged(&short)
    });
    let ran = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_eq!(safe_end(&ran, &scratch, "f.bin", 14, LEN), Ended::Exact);
    let waits = ran.stderr.matches("connection failed, retrying").count();
    println!(
        "{} requests, {waits} waits, {:?}",
        server.log().len(),
        ran.took
    );
    assert_eq!(waits, 0, "valid short answers were waited out as failures");
}

/// A size no disk holds: the run ends with an error and leaves nothing behind. The server stops at 64 MiB in total, so this writes next to nothing even if the size were accepted.
#[test]
fn a_size_no_disk_can_hold_is_refused() {
    const CLAIM: u64 = 1 << 50;
    let scratch = Scratch::new("huge");
    let file = Generated::new(15, CLAIM);
    let server = FaultServer::start(move |req: &Req| {
        let mut plan = file.ranged(req);
        plan.send = if req.sent_before >= 64 * MIB {
            0
        } else {
            plan.send.min(8 * MIB)
        };
        plan
    });
    let ran = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_ne!(ran.code, Some(0));
    println!("{}\nfiles: {:?}", ran.stderr.trim(), scratch.files());
    assert!(
        scratch.files().is_empty(),
        "left behind: {:?}",
        scratch.files()
    );
}
