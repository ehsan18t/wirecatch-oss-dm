//! Whether a download's future was dropped, for the file system work it had
//! already queued on the blocking pool.
//!
//! That work still runs once the download is dropped, and must change no file
//! then: its owner may already have deleted them, or started a new run whose
//! part file has the same name. Each piece of work checks before it starts and
//! again after each flush to the disk, the step a slow disk makes long, and
//! the one during which an owner gives up waiting and drops the download. What
//! remains is the moment between that last check and the write or rename
//! right after it; closing it would need the owner to wait for the queued work
//! to end before it touches the files.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// Marks a download abandoned once its future is dropped: made first thing in
/// a download and dropped with it.
pub(crate) struct Abandonment(Arc<Flags>);

/// What an [`Abandonment`] and its [`Abandoned`] copies share.
#[derive(Default)]
struct Flags {
    dropped: AtomicBool,
    /// Count the download as dropped the next time a flush ends: set only by
    /// tests, which drop a download in the middle of its work this way.
    drop_at_flush: AtomicBool,
}

impl Abandonment {
    pub fn new() -> Self {
        Self(Arc::new(Flags::default()))
    }

    /// What queued work checks.
    pub fn flag(&self) -> Abandoned {
        Abandoned(self.0.clone())
    }
}

impl Drop for Abandonment {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

/// Whether the download's future was dropped; see [`Abandonment`].
#[derive(Clone)]
pub(crate) struct Abandoned(Arc<Flags>);

impl Abandoned {
    pub fn is_set(&self) -> bool {
        self.0.dropped.load(Ordering::SeqCst)
    }

    /// [`Error::Cancelled`] once the download was dropped, for work that must
    /// then not go on; no one receives it.
    pub fn check(&self) -> Result<()> {
        if self.is_set() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Called right after a flush to the disk ends, before the check that
    /// follows it; tests drop the download here.
    pub fn flushed(&self) {
        if self.0.drop_at_flush.load(Ordering::SeqCst) {
            self.0.dropped.store(true, Ordering::SeqCst);
        }
    }

    /// Tests: the download counts as dropped once a flush ends, as when its
    /// owner gives up waiting during a slow flush.
    #[cfg(test)]
    pub fn drop_at_flush(&self) {
        self.0.drop_at_flush.store(true, Ordering::SeqCst);
    }
}
