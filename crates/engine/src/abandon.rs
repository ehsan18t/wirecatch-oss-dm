//! Whether a download's future was dropped, for the file system work it had already queued on the blocking pool.
//!
//! That work still runs once the download is dropped, and must change no file then: its owner may already have deleted them, or started a new run whose part file has the same name. Each piece of work checks before it starts and again after each step that can be slow, which is when an owner gives up waiting and drops the download: after creating the folder (before old files are deleted and the part file is created), after each flush to the disk (before the resume file is written, before it replaces the old one, and before the finished file is renamed), and after waiting for the last checkpoint (before the finished file is sized).
//!
//! What remains is the moment between each last check and the change right after it: deleting the old part and resume files and creating the part file, replacing the resume file, sizing and renaming the finished file and deleting its resume file. Closing those would need the owner to wait for the queued work to end before it touches the files (a drain handle, carried to W9).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// Marks a download abandoned once its future is dropped: made first thing in a download and dropped with it.
pub(crate) struct Abandonment(Arc<Flags>);

/// What an [`Abandonment`] and its [`Abandoned`] copies share.
#[derive(Default)]
struct Flags {
    dropped: AtomicBool,
    /// Tests: count the download as dropped the next time a slow step ends.
    #[cfg(test)]
    drop_after_slow_step: AtomicBool,
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

    /// [`Error::Cancelled`] once the download was dropped, for work that must then not go on; no one receives it.
    pub fn check(&self) -> Result<()> {
        if self.is_set() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Tests: called right after a slow step ends (a flush to the disk, creating a folder), before the check that follows it; a test drops the download here.
    #[cfg(test)]
    pub fn slow_step_done(&self) {
        if self.0.drop_after_slow_step.load(Ordering::SeqCst) {
            self.0.dropped.store(true, Ordering::SeqCst);
        }
    }

    /// Tests: the download counts as dropped once a slow step ends, as when its owner gives up waiting during one.
    #[cfg(test)]
    pub fn drop_after_slow_step(&self) {
        self.0.drop_after_slow_step.store(true, Ordering::SeqCst);
    }
}
