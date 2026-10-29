//! Local test servers and helpers shared by the tests of every Wirecatch crate.
//!
//! Tests never reach the internet (docs/ENGINEERING.md, section 7), so the
//! servers they download from live here, once, instead of as a copy per crate.
//!
//! With the `servers` feature:
//!
//! - `http::TestServer`: an HTTP server for one byte buffer, with ranges,
//!   throttling, injected disconnects, busy answers and connection limits.
//! - `files::FileServer`: an HTTP server for a set of files by path, each of
//!   which can fail a set number of times or answer slowly.
//! - `ftp::FtpServer`: a scripted FTP and FTPS server.
//! - `silent::SilentServer`: a server that accepts connections and never
//!   answers.
//! - `proxy::SocksServer`: a SOCKS5 proxy, with or without a login, and
//!   `proxy::LoginWall`: an HTTP proxy that answers every request with `407`.
//! - `TestCertificate`: a self-signed certificate for the TLS servers.
//! - `http3` (with the `http3` feature): HTTPS servers that advertise and speak
//!   HTTP/3.
//!
//! With the `redaction` feature, `redaction`: the samples both redactions (the log's and the extension's) are tested against, read from `data/redaction.json`, which the extension's tests load too.
//!
//! With the `logs` feature, `logs::Logs`: what the code under test logs, as
//! structured events.
//!
//! Always, and without an HTTP or TLS stack:
//!
//! - [`TestResult`]: what a fallible test or helper returns.
//! - [`PATIENCE`]: how long any wait in a test may take before the test fails.
//! - [`test_data`], [`seeded_data`] and [`ts_segment`]: deterministic bytes to
//!   serve, and [`files_in`] to list what a test downloaded.
//! - [`private_dir`]: a new folder for one test, removed when the test ends
//!   (passed or failed), which only its owner can enter and which has a local
//!   endpoint name of its own for tests of the app's local endpoint.
//!
//! The crate does not depend on the engine, so the engine's own unit tests can
//! use it too.

#![expect(
    clippy::unwrap_used,
    reason = "test support: a panic here is the failing test's report, as clippy.toml already allows inside tests, and the whole crate is only ever used by tests"
)]

#[cfg(feature = "bridge")]
pub mod bridge;
#[cfg(feature = "servers")]
pub mod files;
#[cfg(feature = "servers")]
pub mod ftp;
#[cfg(feature = "servers")]
pub mod http;
#[cfg(feature = "http3")]
pub mod http3;
#[cfg(feature = "logs")]
pub mod logs;
#[cfg(feature = "servers")]
pub mod proxy;
#[cfg(feature = "redaction")]
pub mod redaction;
#[cfg(feature = "resume")]
pub mod resume;
#[cfg(feature = "servers")]
pub mod silent;
#[cfg(feature = "servers")]
mod tls;

#[cfg(feature = "servers")]
pub use tls::TestCertificate;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// What a test helper returns when it can fail, so the test that calls it
/// decides how to report the failure.
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How long any wait in a test may take before the test fails: a bound on a hang, never a measure of the work. A test that finishes in a second alone can take many times that on a machine busy with other builds, and must still pass there, so this is far above any honest wait. A limit the code under test enforces (an idle limit, a time limit) is not a wait: a test that expects such a limit not to run out drives a paused clock rather than racing the machine (docs/ENGINEERING.md, section 7).
pub const PATIENCE: Duration = Duration::from_secs(120);

/// A window in which something must not happen: a test that checks nothing came (no request, no event, no rerun) waits this long first. Short, since it can only miss a fault, never fail a correct run (docs/ENGINEERING.md, section 7).
pub const QUIET: Duration = Duration::from_millis(300);

/// A real ffmpeg from `WIRECATCH_TEST_FFMPEG` or the `PATH`.
///
/// Without one the test is skipped with a note, unless
/// `WIRECATCH_REQUIRE_FFMPEG=1` (set in CI): then it fails, so a missing
/// ffmpeg can never make these tests pass silently.
#[must_use]
pub fn real_ffmpeg() -> Option<PathBuf> {
    let configured = std::env::var_os("WIRECATCH_TEST_FFMPEG").map(PathBuf::from);
    let found = match &configured {
        Some(path) if path.is_file() => Some(path.clone()),
        Some(_) => None,
        None => {
            let name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|d| d.join(&name))
                    .find(|p| p.is_file())
            })
        }
    };
    if found.is_none() {
        let why = match &configured {
            Some(path) => format!(
                "WIRECATCH_TEST_FFMPEG is {}, which is not a file",
                path.display()
            ),
            None => "no ffmpeg on PATH and WIRECATCH_TEST_FFMPEG is not set".to_string(),
        };
        let required = std::env::var("WIRECATCH_REQUIRE_FFMPEG").is_ok_and(|v| v == "1");
        assert!(!required, "WIRECATCH_REQUIRE_FFMPEG=1 but {why}");
        eprintln!("skipped: {why}");
    }
    found
}

/// A new, empty folder for one test that only its owner can enter, removed on
/// drop, so a test that fails leaves nothing behind. It dereferences to its
/// [`Path`].
///
/// On Unix it is created with mode 0700, like the app's runtime folder, so a
/// socket inside it is as private as the app's. [`PrivateDir::endpoint_name`]
/// names a local endpoint of the folder's own, for tests that run a server on
/// it. `name` says which tests the folder belongs to; the process id, a
/// counter and the clock keep every call apart, also from folders a killed
/// earlier run left behind.
///
/// # Errors
///
/// The folder cannot be created, or (on Unix) its mode cannot be set.
pub fn private_dir(name: &str) -> TestResult<PrivateDir> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    // A run that was killed leaves its folders behind, and a later run can get
    // the same process id: the clock keeps their names apart.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let leaf = format!("wirecatch-test-{name}-{}-{n}-{nonce}", std::process::id());
    let path = std::env::temp_dir().join(&leaf);
    #[cfg(not(unix))]
    std::fs::create_dir(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().mode(OWNER_ONLY).create(&path)?;
        // The umask can only remove bits; set the exact mode anyway.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(OWNER_ONLY))?;
    }
    Ok(PrivateDir {
        path,
        leaf,
        removed: false,
    })
}

/// The mode of a folder only its owner can read, write and enter.
#[cfg(unix)]
const OWNER_ONLY: u32 = 0o700;

/// A folder from [`private_dir`], removed with everything in it on drop.
#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
    /// The folder's own name, unique to this call.
    leaf: String,
    /// Removed already by [`PrivateDir::remove`].
    removed: bool,
}

impl PrivateDir {
    /// Where the folder is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A local endpoint name nobody else uses, in the form the app's endpoint
    /// takes: a named pipe on Windows, a socket inside this folder on Unix.
    #[must_use]
    pub fn endpoint_name(&self) -> OsString {
        if cfg!(windows) {
            format!(r"\\.\pipe\{}", self.leaf).into()
        } else {
            self.path.join("app.sock").into_os_string()
        }
    }

    /// Removes the folder with everything in it, and says whether that worked.
    ///
    /// On Windows a file held open without delete sharing cannot be deleted, so
    /// a test that ends with `dir.remove()?` also catches such a handle left
    /// open by the code it tested. Dropping the folder cannot fail the test.
    /// When removing fails, the drop that follows tries again and reports a
    /// folder it cannot remove either, like any other left behind.
    ///
    /// # Errors
    ///
    /// The folder or something in it could not be removed; the error names the
    /// folder.
    pub fn remove(mut self) -> std::io::Result<()> {
        std::fs::remove_dir_all(&self.path).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!(
                    "could not remove {} ({error}); a file in it may still be open",
                    self.path.display()
                ),
            )
        })?;
        self.removed = true;
        Ok(())
    }
}

impl std::ops::Deref for PrivateDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for PrivateDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        // A drop cannot fail the test, so a folder that cannot be removed is only
        // reported; one the test already removed by hand is fine.
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("could not remove {}: {error}", self.path.display());
        }
    }
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

    /// A file held open without sharing keeps its folder on Windows: the error
    /// says which folder and why, and the folder is still there.
    #[cfg(windows)]
    #[test]
    fn a_folder_with_a_file_held_open_says_so() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = private_dir("open-file").unwrap();
        let path = dir.path().to_path_buf();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(dir.join("held.bin"))
            .unwrap();
        let error = dir.remove().unwrap_err().to_string();
        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains("still be open"), "{error}");
        assert!(path.exists());
        drop(held);
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn test_data_is_seed_one() {
        assert_eq!(test_data(64), seeded_data(1, 64));
        assert_ne!(seeded_data(1, 64), seeded_data(2, 64));
    }

    #[test]
    fn a_private_dir_is_its_own_and_goes_away_on_drop() -> TestResult {
        let first = private_dir("support")?;
        let second = private_dir("support")?;
        assert!(first.path().is_dir());
        assert_ne!(first.path(), second.path());
        assert_ne!(first.endpoint_name(), second.endpoint_name());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(first.path())?.permissions().mode();
            assert_eq!(mode & 0o777, OWNER_ONLY);
        }
        let path = first.path().to_path_buf();
        std::fs::write(path.join("inside"), b"x")?;
        drop(first);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn ts_segment_starts_with_the_sync_byte() {
        let segment = ts_segment(7, 32);
        assert_eq!(segment.first(), Some(&TS_SYNC_BYTE));
        assert_eq!(segment[1..], seeded_data(7, 32)[1..]);
        assert_eq!(ts_segment(7, 0), [] as [u8; 0]);
    }
}
