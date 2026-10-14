//! Capturing logs while other threads log, in a test binary of its own: whether tracing asks a capture about a call site depends on every subscriber in the process, so tests elsewhere in the binary would change what this one sees.

use std::thread;

use wirecatch_test_support::logs::Logs;

fn log_at_one_site() {
    tracing::warn!("logged at one site");
}

/// tracing works out once per call site whether anyone wants its events. Logged first by a thread with no subscriber of its own, while one capture is the only subscriber in the process, the site would be settled as wanted by nobody, for every thread.
#[test]
fn a_site_another_thread_logged_first_is_still_captured() {
    let (logs, _capturing) = Logs::capture();
    thread::spawn(log_at_one_site).join().unwrap();
    log_at_one_site();
    assert_eq!(logs.events().len(), 1, "{}", logs.text());
}
