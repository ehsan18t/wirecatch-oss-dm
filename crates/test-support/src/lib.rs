//! Local test servers and helpers shared by the tests of every Wirecatch crate.
//!
//! Tests never reach the internet (docs/ENGINEERING.md, section 7), so the
//! servers they download from live here, once, instead of as a copy per crate:
//!
//! - [`http::TestServer`]: an HTTP server for one byte buffer, with ranges,
//!   throttling, injected disconnects, busy answers and connection limits.
//! - [`ftp::FtpServer`]: a scripted FTP and FTPS server.
//! - `http3` (with the `http3` feature): HTTPS servers that advertise and speak
//!   HTTP/3.
//! - [`test_data`]: deterministic bytes to serve, and [`fresh_dir`] and
//!   [`files_in`] for the folders tests download into.
//! - [`TestCertificate`]: a self-signed certificate for the TLS servers.
//!
//! The crate does not depend on the engine, so the engine's own unit tests can
//! use it too.

#![expect(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "test support: a panic here is the failing test's report, as clippy.toml already allows inside tests, and the whole crate is only ever used by tests"
)]

pub mod ftp;
pub mod http;
#[cfg(feature = "http3")]
pub mod http3;
mod tls;

pub use tls::TestCertificate;

use std::path::{Path, PathBuf};

/// What a test helper returns when it can fail, so the test that calls it
/// decides how to report the failure.
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// An empty folder for one test, under the system's temporary folder: anything
/// an earlier run left there is removed first. `name` must be unique among the
/// tests of one test binary; the process id keeps binaries apart.
#[must_use]
pub fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wirecatch-test-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The names of the entries in `dir`, sorted.
#[must_use]
pub fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Deterministic pseudo-random bytes, so a misplaced range is always detected.
#[must_use]
pub fn test_data(len: usize) -> Vec<u8> {
    // The xorshift64 generator; the seed is any value other than zero.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let [low, ..] = x.to_le_bytes();
            low
        })
        .collect()
}
