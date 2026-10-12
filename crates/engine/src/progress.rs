//! Live counters for one download.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const UNKNOWN: u64 = u64::MAX;

/// Live counters for one download. Cheap to read from another task or thread.
///
/// Create one with [`Progress::default`], pass it to [`download`](fn@crate::download)
/// in an `Arc`, and read it from anywhere, for example a few times a second to
/// draw a progress bar. Speed is the change in [`done`](Self::done) over time.
#[derive(Debug)]
pub struct Progress {
    total: AtomicU64,
    done: AtomicU64,
    connections: AtomicUsize,
    /// Where the file goes, and its name, once the server has answered.
    target: Mutex<Option<Target>>,
}

#[derive(Debug)]
struct Target {
    file_name: String,
    path: PathBuf,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            total: AtomicU64::new(UNKNOWN),
            done: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            target: Mutex::new(None),
        }
    }
}

impl Progress {
    /// Size of the file, once known.
    pub fn total(&self) -> Option<u64> {
        let t = self.total.load(Ordering::Relaxed);
        (t != UNKNOWN).then_some(t)
    }

    /// Bytes received so far, including bytes kept from an earlier run.
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Connections currently receiving data.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::Relaxed)
    }

    /// The name the file is saved under, once decided.
    pub fn file_name(&self) -> Option<String> {
        self.target().as_ref().map(|t| t.file_name.clone())
    }

    /// The final destination path of the download.
    ///
    /// Set once the server has answered with the file; until then the link has
    /// not been proven to work. It is the path the file has once complete (the
    /// data meanwhile goes to this path plus [`PART_EXT`](crate::PART_EXT)),
    /// with the name the server suggested and any ` (1)` suffix already applied,
    /// so it can be saved and passed as
    /// [`DownloadRequest::exact_path`](crate::DownloadRequest::exact_path) on a
    /// later run. It stays `None` if the first request fails.
    pub fn path(&self) -> Option<PathBuf> {
        self.target().as_ref().map(|t| t.path.clone())
    }

    /// Locks the destination. A lock poisoned by a panic in another holder is
    /// taken over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives; the value
    /// is replaced whole, so it is never half written either.
    fn target(&self) -> MutexGuard<'_, Option<Target>> {
        self.target.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn start(
        &self,
        path: &Path,
        file_name: &str,
        total: Option<u64>,
        already_done: u64,
    ) {
        *self.target() = Some(Target {
            file_name: file_name.to_string(),
            path: path.to_path_buf(),
        });
        self.total
            .store(total.unwrap_or(UNKNOWN), Ordering::Relaxed);
        self.done.store(already_done, Ordering::Relaxed);
    }

    pub(crate) fn add(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Takes back bytes counted by [`add`](Self::add) that were never written and will be fetched again.
    pub(crate) fn take_back(&self, bytes: u64) {
        // A loop rather than `fetch_update` or `try_update`: one is deprecated, the other newer than the minimum Rust version.
        let mut done = self.done.load(Ordering::Relaxed);
        while let Err(now) = self.done.compare_exchange_weak(
            done,
            done.saturating_sub(bytes),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            done = now;
        }
    }

    pub(crate) fn connection_opened(&self) -> ConnectionGuard<'_> {
        self.connections.fetch_add(1, Ordering::Relaxed);
        ConnectionGuard(self)
    }
}

pub(crate) struct ConnectionGuard<'a>(&'a Progress);

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
    }
}
