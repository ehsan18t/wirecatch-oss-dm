//! Live counters for one download.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const UNKNOWN: u64 = u64::MAX;

/// Live counters for one download. Cheap to read from another task or thread.
///
/// Create one with [`Progress::default`], pass it to [`download`](crate::download)
/// in an `Arc`, and read it from anywhere, for example a few times a second to
/// draw a progress bar. Speed is the change in [`done`](Self::done) over time.
#[derive(Debug)]
pub struct Progress {
    total: AtomicU64,
    done: AtomicU64,
    connections: AtomicUsize,
    file_name: Mutex<Option<String>>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            total: AtomicU64::new(UNKNOWN),
            done: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            file_name: Mutex::new(None),
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
        self.file_name.lock().unwrap().clone()
    }

    pub(crate) fn start(&self, file_name: &str, total: Option<u64>, already_done: u64) {
        *self.file_name.lock().unwrap() = Some(file_name.to_string());
        self.total
            .store(total.unwrap_or(UNKNOWN), Ordering::Relaxed);
        self.done.store(already_done, Ordering::Relaxed);
    }

    pub(crate) fn add(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
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
