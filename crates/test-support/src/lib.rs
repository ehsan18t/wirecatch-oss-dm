//! Local test servers and helpers shared by the tests of every Wirecatch crate.
//!
//! Tests never reach the internet (docs/ENGINEERING.md, section 7), so the
//! servers they download from live here, once, instead of as a copy per crate:
//!
//! - [`http::TestServer`]: an HTTP server for one byte buffer, with ranges,
//!   throttling, injected disconnects, busy answers and connection limits.
//! - [`files::FileServer`]: an HTTP server for a set of files by path, each of
//!   which can fail a set number of times or answer slowly.
//! - [`ftp::FtpServer`]: a scripted FTP and FTPS server.
//! - [`silent::SilentServer`]: a server that accepts connections and never
//!   answers.
//! - `http3` (with the `http3` feature): HTTPS servers that advertise and speak
//!   HTTP/3.
//! - [`test_data`], [`seeded_data`] and [`ts_segment`]: deterministic bytes to
//!   serve, and [`fresh_dir`] and
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

pub mod files;
pub mod ftp;
pub mod http;
#[cfg(feature = "http3")]
pub mod http3;
pub mod silent;
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

/// Multiplies a seed into the xorshift64 generator's first state (2^64
/// divided by the golden ratio), so seeds next to each other start far apart.
const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;
/// The first byte of every MPEG transport stream packet.
const TS_SYNC_BYTE: u8 = 0x47;

/// Deterministic pseudo-random bytes, so a misplaced range is always detected.
/// The same as [`seeded_data`] with seed 1.
#[must_use]
pub fn test_data(len: usize) -> Vec<u8> {
    seeded_data(1, len)
}

/// Deterministic pseudo-random bytes, different for each `seed`, for tests
/// that serve several files and must tell them apart.
#[must_use]
pub fn seeded_data(seed: u64, len: usize) -> Vec<u8> {
    // The xorshift64 generator. Its state must never be zero, which `| 1` ensures.
    let mut x = seed.wrapping_mul(GOLDEN_RATIO_64) | 1;
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

/// [`seeded_data`] that starts with the MPEG-TS sync byte, so a stream
/// segment, and a track joined from such segments, is recognised as `.ts`.
#[must_use]
pub fn ts_segment(seed: u64, len: usize) -> Vec<u8> {
    let mut data = seeded_data(seed, len);
    if let Some(first) = data.first_mut() {
        *first = TS_SYNC_BYTE;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_is_seed_one() {
        assert_eq!(test_data(64), seeded_data(1, 64));
        assert_ne!(seeded_data(1, 64), seeded_data(2, 64));
    }

    #[test]
    fn ts_segment_starts_with_the_sync_byte() {
        let segment = ts_segment(7, 32);
        assert_eq!(segment.first(), Some(&TS_SYNC_BYTE));
        assert_eq!(segment[1..], seeded_data(7, 32)[1..]);
        assert_eq!(ts_segment(7, 0), [] as [u8; 0]);
    }
}
