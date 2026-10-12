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
//! - [`test_data`]: deterministic bytes to serve.
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
