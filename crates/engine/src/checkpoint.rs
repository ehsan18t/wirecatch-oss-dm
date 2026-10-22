//! The resume file, saved while a download runs and once more as it ends.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing::warn;

use crate::abandon::Abandoned;
use crate::download::Shared;
use crate::error::{Chain, IoAction, IoContext, Result};
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

    /// Whether the download was dropped, for the work that ends it.
    pub fn abandoned(&self) -> &Abandoned {
        &self.abandoned
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
        let shared = &self.shared;
        flush_then(part, &shared.part_path, &self.abandoned, || {
            ResumeState::new(
                &self.url,
                shared.request.total,
                shared.request.ranged,
                &self.validators,
                remaining,
            )
            .save(&self.path, &self.abandoned)
        })
    }

    /// Waits for a save still writing, then stops any later one.
    pub fn close(&self) {
        let mut saving = self.saving();
        saving.closed = true;
        saving.part = None;
    }
}

/// Flushes `part` (at `path`) to the disk, then runs `write`, unless the download was abandoned meanwhile: a slow flush is just when an owner gives up waiting and drops the download, and then deletes its files.
fn flush_then(
    part: &File,
    path: &Path,
    abandoned: &Abandoned,
    write: impl FnOnce() -> Result<()>,
) -> Result<()> {
    part.sync_data().context(IoAction::Flush, path)?;
    #[cfg(test)]
    abandoned.slow_step_done();
    if abandoned.is_set() {
        return Ok(());
    }
    write()
}

/// The state saves share, behind one lock.
struct Saving {
    /// Set once the download has ended.
    closed: bool,
    /// The part file, to flush to the disk, once a save has opened it.
    part: Option<File>,
}

/// The task that saves the resume file at every checkpoint while a download
/// runs. Dropping this stops it; a save it already queued writes nothing once
/// the download is [`Abandoned`].
pub(crate) struct Checkpoints {
    task: tokio::task::JoinHandle<()>,
}

impl Checkpoints {
    /// Saves every [`DownloadOptions::checkpoint_interval`](crate::DownloadOptions::checkpoint_interval) until the download
    /// is cancelled or this is dropped.
    pub fn spawn(saver: Arc<ResumeSaver>) -> Self {
        let every = saver.shared.opts.checkpoint_interval;
        let cancel = saver.shared.request.cancel.clone();
        let task = tokio::spawn(async move {
            let mut tick = ticker(every);
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
                    warn!(error = %Chain(&e), "could not save resume state; the next checkpoint tries again");
                }
            }
        });
        Self { task }
    }
}

/// Ticks every `every`. A save that takes longer than that (a disk slower than
/// the network, since each save flushes the part file) delays the next tick
/// rather than being followed by a burst of saves to catch up.
fn ticker(every: std::time::Duration) -> tokio::time::Interval {
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick
}

impl Drop for Checkpoints {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_download_dropped_during_the_flush_writes_no_resume_file() {
        let dir = wirecatch_test_support::private_dir("flush-then").unwrap();
        let path = dir.join("f.bin.wcpart");
        let part = File::create(&path).unwrap();
        let abandonment = crate::abandon::Abandonment::new();
        let abandoned = abandonment.flag();
        let mut wrote = false;
        flush_then(&part, &path, &abandoned, || {
            wrote = true;
            Ok(())
        })
        .unwrap();
        assert!(wrote, "a live download writes after the flush");

        abandoned.drop_after_slow_step();
        let mut wrote = false;
        flush_then(&part, &path, &abandoned, || {
            wrote = true;
            Ok(())
        })
        .unwrap();
        assert!(
            !wrote,
            "a download dropped during the flush wrote its resume file"
        );
        drop(part);
        drop(abandonment);
        dir.remove().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_save_delays_the_next_checkpoint_instead_of_bunching_them() {
        let every = Duration::from_secs(1);
        let mut tick = ticker(every);
        tick.tick().await;
        // A save that took three intervals.
        tokio::time::sleep(3 * every).await;
        tick.tick().await;
        let start = tokio::time::Instant::now();
        tick.tick().await;
        assert_eq!(
            start.elapsed(),
            every,
            "the missed checkpoints came at once"
        );
    }
}
