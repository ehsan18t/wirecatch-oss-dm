//! The resume file, saved while a download runs and once more as it ends.

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing::warn;

use crate::download::Shared;
use crate::error::{Error, IoAction, IoContext, Result};
use crate::http::Validators;
use crate::state::ResumeState;
use crate::storage;

/// Writes the resume file, one save at a time.
///
/// A checkpoint already running on the blocking pool is not stopped by
/// aborting its task, and two saves at once would race on the same temporary
/// file, so saves hold `saving`. Once the download has ended, it is marked
/// closed and a late checkpoint writes nothing (it could otherwise put back a
/// resume file next to a finished download).
///
/// The resume file only ever lists as done what is on the disk itself: each
/// save first flushes the part file to the disk, then writes the ranges that
/// were done before that flush. So a power cut or a crash of the system, not
/// only of the process, never leaves a resume file that counts bytes the disk
/// never received (which would read back as zeros).
///
/// Every method blocks, on file I/O or on a save in progress, so it is only
/// called on the blocking pool.
pub(crate) struct ResumeSaver {
    shared: Arc<Shared>,
    /// The URL the resume file records, without a password.
    url: String,
    path: PathBuf,
    validators: Validators,
    /// What saves share.
    saving: Mutex<Saving>,
    /// Set if the download's future is dropped before it ends: a save already
    /// queued on the blocking pool then writes nothing either.
    abandoned: Abandoned,
}

impl ResumeSaver {
    /// A saver for the download `shared`, recording `url` and `validators` in
    /// the resume file at `path`.
    pub fn new(
        shared: Arc<Shared>,
        url: String,
        path: PathBuf,
        validators: Validators,
        abandoned: Abandoned,
    ) -> Self {
        Self {
            shared,
            url,
            path,
            validators,
            saving: Mutex::new(Saving {
                closed: false,
                part: None,
            }),
            abandoned,
        }
    }

    /// Locks `saving`. A lock poisoned by a panic in another holder is taken
    /// over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives.
    fn saving(&self) -> MutexGuard<'_, Saving> {
        self.saving.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Saves the ranges still to download, unless the download has ended.
    pub fn save(&self) -> Result<()> {
        let mut saving = self.saving();
        if saving.closed || self.abandoned.is_set() {
            return Ok(());
        }
        // Read before the flush: every range done by now was written before it.
        let remaining = self.shared.table.lock().remaining();
        let part = match &mut saving.part {
            Some(part) => part,
            empty => {
                let path = &self.shared.part_path;
                empty.insert(storage::open_for_write(path).context(IoAction::OpenFile, path)?)
            }
        };
        part.sync_data()
            .context(IoAction::Flush, &self.shared.part_path)?;
        let shared = &self.shared;
        ResumeState::new(
            &self.url,
            shared.request.total,
            shared.request.ranged,
            &self.validators,
            remaining,
        )
        .save(&self.path)
    }

    /// Waits for a save still writing, then stops any later one.
    pub fn close(&self) {
        let mut saving = self.saving();
        saving.closed = true;
        saving.part = None;
    }
}

/// The state saves share, behind one lock.
struct Saving {
    /// Set once the download has ended.
    closed: bool,
    /// The part file, to flush to the disk, once a save has opened it.
    part: Option<File>,
}

/// Marks a download abandoned once its future is dropped: made first thing in
/// a download and dropped with it.
///
/// File system work the download already queued on the blocking pool still
/// runs then, and must change no file: its owner may already have deleted them
/// or started a new run. Each such piece of work checks [`Abandoned`] first.
pub(crate) struct Abandonment(Arc<AtomicBool>);

impl Abandonment {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// What queued work checks.
    pub fn flag(&self) -> Abandoned {
        Abandoned(self.0.clone())
    }
}

impl Drop for Abandonment {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Whether the download's future was dropped; see [`Abandonment`].
#[derive(Clone)]
pub(crate) struct Abandoned(Arc<AtomicBool>);

impl Abandoned {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// [`Error::Cancelled`] once the download was dropped, for work that must
    /// then not start; no one receives it.
    pub fn check(&self) -> Result<()> {
        if self.is_set() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The task that saves the resume file at every checkpoint while a download
/// runs. Dropping this stops it; a save it already queued writes nothing once
/// the download is [`Abandoned`].
pub(crate) struct Checkpoints {
    task: tokio::task::JoinHandle<()>,
}

impl Checkpoints {
    /// Saves every [`DownloadOptions::checkpoint_interval`] until the download
    /// is cancelled or this is dropped.
    pub fn spawn(saver: Arc<ResumeSaver>) -> Self {
        let every = saver.shared.opts.checkpoint_interval;
        let cancel = saver.shared.request.cancel.clone();
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.tick().await;
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    // Once asked to stop, the download saves its state itself on
                    // the way out.
                    () = cancel.cancelled() => return,
                }
                let save = saver.clone();
                if let Err(e) = storage::blocking(move || save.save()).await {
                    warn!(error = %e, "could not save resume state; the next checkpoint tries again");
                }
            }
        });
        Self { task }
    }
}

impl Drop for Checkpoints {
    fn drop(&mut self) {
        self.task.abort();
    }
}
