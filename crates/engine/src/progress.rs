//! Live counters for one download.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    can_continue: AtomicBool,
    /// [`Retry::attempt`], 0 while data flows.
    retry: AtomicU32,
    /// [`Retry::of`].
    retries: AtomicU32,
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
            can_continue: AtomicBool::new(false),
            retry: AtomicU32::new(0),
            retries: AtomicU32::new(0),
            target: Mutex::new(None),
        }
    }
}

/// A download trying again after a connection failed or dropped: see [`Progress::retry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Retry {
    /// Which try this is, counting from 1. A failed attempt that received data starts the count
    /// again.
    pub attempt: u32,
    /// How many tries a connection makes before it gives up
    /// ([`DownloadOptions::max_retries`](crate::DownloadOptions::max_retries)).
    pub of: u32,
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

    /// Whether a later run can continue from what is on disk, as the server's answer says: it takes byte ranges and gave the size. Without that a later run starts from the beginning. `false` until the server has answered.
    pub fn can_continue(&self) -> bool {
        self.can_continue.load(Ordering::Relaxed)
    }

    /// The try a connection is making again after it failed or dropped, while no new data has
    /// arrived since on any connection; `None` while data flows.
    pub fn retry(&self) -> Option<Retry> {
        let attempt = self.retry.load(Ordering::Relaxed);
        // Another connection still receiving clears the try only with its next chunk.
        (attempt > 0 && self.connections() == 0).then(|| Retry {
            attempt,
            of: self.retries.load(Ordering::Relaxed),
        })
    }

    pub(crate) fn retrying(&self, attempt: u32, of: u32) {
        self.retries.store(of, Ordering::Relaxed);
        self.retry.fetch_max(attempt, Ordering::Relaxed);
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
    /// later run. It stays `None` if the first request fails. In a download
    /// into a folder, a file another program saves under that name before the
    /// download finishes keeps it, and the download takes the next free name:
    /// [`Outcome::Completed`](crate::Outcome::Completed) says where the file
    /// went.
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
        can_continue: bool,
    ) {
        self.total
            .store(total.unwrap_or(UNKNOWN), Ordering::Relaxed);
        self.done.store(already_done, Ordering::Relaxed);
        self.can_continue.store(can_continue, Ordering::Relaxed);
        // Last: a reader that sees the name also sees what an earlier run kept.
        *self.target() = Some(Target {
            file_name: file_name.to_string(),
            path: path.to_path_buf(),
        });
    }

    pub(crate) fn add(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
        // A load first: every chunk passes here, and a store would contend between connections.
        if self.retry.load(Ordering::Relaxed) != 0 {
            self.retry.store(0, Ordering::Relaxed);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_shows_only_while_no_connection_receives() {
        let progress = Progress::default();
        let receiving = progress.connection_opened();
        progress.retrying(1, 3);
        assert_eq!(progress.retry(), None, "another connection still receives");
        drop(receiving);
        assert_eq!(progress.retry(), Some(Retry { attempt: 1, of: 3 }));
    }

    #[test]
    fn new_data_clears_the_retry() {
        let progress = Progress::default();
        progress.retrying(2, 3);
        progress.add(10);
        assert_eq!(progress.retry(), None);
    }
}
