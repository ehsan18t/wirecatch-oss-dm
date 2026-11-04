//! A file that really changes on the server during a download, bytes and all, through the real command line.

use std::time::Duration;

use wirecatch_test_support::PATIENCE;
use wirecatch_test_support::faults::{
    Ended, FaultServer, Generated, Plan, Req, Scratch, check_file, cli, safe_end,
};

const EXE: &str = env!("CARGO_BIN_EXE_wirecatch");
const MIB: u64 = 1024 * 1024;
const LEN: u64 = 64 * MIB;
const FLIP_AT: u64 = 8 * MIB;
const BOUND: Duration = PATIENCE.saturating_mul(2);

/// Every answer sends one MiB, so many requests come after the file changes at `FLIP_AT`. The `Date` makes a `Last-Modified` a strong validator (D34.2).
fn short(file: &Generated, req: &Req) -> Plan {
    let mut asked = req.clone();
    if let Some((start, _)) = req.range {
        asked.range = Some((start, Some((start + MIB).min(file.len))));
    }
    file.ranged(&asked)
        .header("Date", "Wed, 01 Jan 2025 00:00:00 GMT")
}

/// Serves `old` and then `new`. Nothing may end at the final name as a mix: the run refuses, and the next run saves the new version whole.
fn changes(name: &str, old: Generated, new: Generated) {
    let scratch = Scratch::new(name);
    let (seed, len) = (new.seed, new.len);
    let later = new.clone();
    let server = FaultServer::start(move |req| {
        short(
            if req.sent_before < FLIP_AT {
                &old
            } else {
                &new
            },
            req,
        )
    });
    let ran = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_ne!(
        ran.code,
        Some(0),
        "exit status 0 across a change; stdout: {}; compared with the new version: {:?}",
        ran.stdout,
        check_file(&scratch.downloads.join("f.bin"), seed, len)
    );
    assert_eq!(safe_end(&ran, &scratch, "f.bin", seed, len), Ended::Refused);

    server.set_handler(move |req| short(&later, req));
    let again = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_eq!(
        safe_end(&again, &scratch, "f.bin", seed, len),
        Ended::Exact,
        "{}",
        again.stderr
    );
    assert_eq!(scratch.files(), ["f.bin"], "partial files left behind");
}

#[test]
fn a_strong_etag_and_the_bytes_change_in_a_parallel_download() {
    changes("etag", Generated::new(1, LEN), Generated::new(2, LEN));
}

#[test]
fn a_weak_etag_and_the_bytes_change_in_a_parallel_download() {
    let weak = |seed| {
        let mut file = Generated::new(seed, LEN);
        file.etag = Some(format!("W/\"v{seed}\""));
        file
    };
    changes("weak", weak(3), weak(4));
}

#[test]
fn only_a_last_modified_and_the_bytes_change_in_a_parallel_download() {
    let dated = |seed, day: &str| {
        let mut file = Generated::new(seed, LEN).without_validators();
        file.last_modified = Some(format!("{day} GMT"));
        file
    };
    changes(
        "modified",
        dated(5, "Mon, 01 Jan 2024 00:00:00"),
        dated(6, "Tue, 02 Jan 2024 00:00:00"),
    );
}

#[test]
fn the_size_and_the_bytes_change_with_no_validators() {
    changes(
        "size",
        Generated::new(7, LEN).without_validators(),
        Generated::new(8, LEN + MIB).without_validators(),
    );
}

/// No ranges: the connection drops, and the answer to asking again is the new version.
#[test]
fn the_file_changes_before_a_whole_answer_is_asked_again() {
    let scratch = Scratch::new("whole");
    let (old, new) = (Generated::new(9, LEN), Generated::new(10, LEN));
    let later = new.clone();
    let server = FaultServer::start(move |req: &Req| {
        if req.no == 0 {
            let mut plan = old.whole();
            plan.send = FLIP_AT;
            plan
        } else {
            new.whole()
        }
    });
    let ran = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_eq!(safe_end(&ran, &scratch, "f.bin", 10, LEN), Ended::Refused);
    server.set_handler(move |_| later.whole());
    let again = cli(EXE, &scratch, &server.url("f.bin")).ended(BOUND);
    assert_eq!(safe_end(&again, &scratch, "f.bin", 10, LEN), Ended::Exact);
    assert_eq!(scratch.files(), ["f.bin"], "partial files left behind");
}
