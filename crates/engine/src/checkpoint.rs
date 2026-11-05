//! The resume file, saved while a download runs and once more as it ends.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing::warn;

use crate::abandon::Abandoned;
use crate::download::Shared;
use crate::error::{Chain, Error, IoAction, IoContext, Result};
use crate::http::Validators;
use crate::segment::ByteRange;
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
/// A failed flush may have lost bytes that a later flush then passes without, so it ends the saving and stops the run, whose later bytes could never be counted: the resume file stays as the last good save left it, and [`close`](Self::close) reports the failure.
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
                failed_flush: None,
                saved: None,
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

    /// Saves the ranges still to download, unless the download has ended or they are what the resume file already says: a stalled download has no reason to flush every second.
    pub fn save(&self) -> Result<()> {
        let mut saving = self.saving();
        if saving.closed || saving.failed_flush.is_some() || self.abandoned.is_set() {
            return Ok(());
        }
        // Read before the flush: every range done by now was written before it.
        let remaining = self.shared.table.lock().remaining();
        let Saving {
            part,
            failed_flush,
            saved,
            ..
        } = &mut *saving;
        if saved.as_ref() == Some(&remaining) {
            return Ok(());
        }
        let part = match part {
            Some(part) => part,
            empty => {
                let path = &self.shared.part_path;
                empty.insert(storage::open_for_write(path).context(IoAction::OpenFile, path)?)
            }
        };
        let shared = &self.shared;
        let flush = || {
            #[cfg(test)]
            if shared.file_work.flush_fails() {
                return Err(std::io::Error::other("write-back failed"));
            }
            part.sync_data()
        };
        let result = flush_then(
            flush,
            &shared.part_path,
            &self.abandoned,
            failed_flush,
            || {
                ResumeState::new(
                    &self.url,
                    shared.request.total,
                    shared.request.ranged,
                    &self.validators,
                    remaining.clone(),
                )
                .save(&self.path, &self.abandoned)?;
                *saved = Some(remaining);
                Ok(())
            },
        );
        if failed_flush.is_some() {
            shared.request.cancel.cancel();
        }
        result
    }

    /// A save whose failed flush ends the download at once: the first one, before anything is fetched.
    pub fn save_or_fail(&self) -> Result<()> {
        self.save()?;
        match self.saving().failed_flush.take() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }

    /// Waits for a save still writing, then stops any later one. Gives the failed flush of any save of this download.
    pub fn close(&self) -> Result<()> {
        let mut saving = self.saving();
        saving.closed = true;
        saving.part = None;
        match saving.failed_flush.take() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

/// Runs `flush` on the part file at `path`, then `write`, unless the download was abandoned meanwhile: a slow flush is just when an owner gives up waiting and drops the download, and then deletes its files. A failed flush goes to `failed_flush` and nothing is written.
fn flush_then(
    flush: impl FnOnce() -> std::io::Result<()>,
    path: &Path,
    abandoned: &Abandoned,
    failed_flush: &mut Option<Error>,
    write: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if let Err(failure) = flush().context(IoAction::Flush, path) {
        warn!(error = %Chain(&failure), "could not flush the part file; stopping the download");
        *failed_flush = Some(failure);
        return Ok(());
    }
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
    /// The first flush that failed; no save writes after it.
    failed_flush: Option<Error>,
    /// The ranges the resume file holds, once this run has written it.
    saved: Option<Vec<ByteRange>>,
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
                if let Err(e) =
                    storage::blocking(&saver.shared.file_work, move || save.save()).await
                {
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

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;
    use url::Url;
    use wirecatch_test_support::resume::kept_ranges;
    use wirecatch_test_support::{PATIENCE, private_dir, test_data};

    use crate::download::{DownloadRequest, download};
    use crate::progress::Progress;
    use crate::units::{KIB_USIZE, MIB_USIZE};

    #[test]
    fn a_download_dropped_during_the_flush_writes_no_resume_file() {
        let dir = wirecatch_test_support::private_dir("flush-then").unwrap();
        let path = dir.join("f.bin.wcpart");
        let part = File::create(&path).unwrap();
        let abandonment = crate::abandon::Abandonment::new();
        let abandoned = abandonment.flag();
        let mut failed_flush = None;
        let mut wrote = false;
        flush_then(
            || part.sync_data(),
            &path,
            &abandoned,
            &mut failed_flush,
            || {
                wrote = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(wrote, "a live download writes after the flush");

        abandoned.drop_after_slow_step();
        let mut wrote = false;
        flush_then(
            || part.sync_data(),
            &path,
            &abandoned,
            &mut failed_flush,
            || {
                wrote = true;
                Ok(())
            },
        )
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

    /// Answers `/f.bin` with half of its `len` bytes and holds back the rest.
    async fn half_then_hold(len: usize) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            let mut buf = [0u8; KIB_USIZE];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.unwrap();
                head.extend_from_slice(&buf[..n]);
            }
            let body = test_data(len);
            let answer = format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n");
            socket.write_all(answer.as_bytes()).await.unwrap();
            socket.write_all(&body[..len / 2]).await.unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        Url::parse(&format!("http://127.0.0.1:{port}/f.bin")).unwrap()
    }

    fn assert_flush_error<T: std::fmt::Debug>(result: &Result<T>) {
        assert!(
            matches!(
                result,
                Err(Error::Io {
                    action: IoAction::Flush,
                    ..
                })
            ),
            "the failed flush was forgotten: {result:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_checkpoint_flush_stops_the_run_with_the_rest_still_to_come() {
        let len = MIB_USIZE;
        let url = half_then_hold(len).await;
        let dir = private_dir("failed-flush").unwrap();
        let mut request = DownloadRequest::new(url, dir.to_path_buf());
        request.options.checkpoint_interval = Duration::from_millis(1);
        // A longer wait for the held half than `PATIENCE`, so only the flush can end the run.
        request.options.stall_timeout = 2 * PATIENCE;
        // Small enough that the first half reaches the file, which is what a checkpoint saves.
        request.options.buffer_size = 64 * KIB_USIZE;
        // The first save, before anything is fetched, passes; the first checkpoint's fails.
        request.file_work.fail_flush_after(1);
        let run = download(
            request,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        );
        let result = tokio::time::timeout(PATIENCE, run)
            .await
            .expect("the failed flush did not stop the download");
        assert_flush_error(&result);
        let kept = kept_ranges(&dir.join("f.bin.wcstate"), len as u64).unwrap();
        assert!(
            kept.is_empty(),
            "the resume file counts bytes after the failed flush: {kept:?}"
        );
        dir.remove().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_pause_whose_last_flush_fails_reports_the_failure() {
        let len = MIB_USIZE;
        let url = half_then_hold(len).await;
        let dir = private_dir("failed-flush-paused").unwrap();
        let mut request = DownloadRequest::new(url, dir.to_path_buf());
        // No checkpoint comes first: the flush that fails is the pause's own.
        request.options.checkpoint_interval = 2 * PATIENCE;
        request.file_work.fail_flush_after(1);
        let progress = Arc::new(Progress::default());
        let cancel = CancellationToken::new();
        let run = tokio::spawn(download(request, progress.clone(), cancel.clone()));
        tokio::time::timeout(PATIENCE, async {
            while progress.done() == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("no byte arrived");
        cancel.cancel();
        let result = tokio::time::timeout(PATIENCE, run)
            .await
            .expect("the download hung")
            .unwrap();
        assert_flush_error(&result);
        dir.remove().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_download_whose_first_flush_failed_ends_at_once() {
        let len = MIB_USIZE;
        let url = half_then_hold(len).await;
        let dir = private_dir("failed-flush-first").unwrap();
        let request = DownloadRequest::new(url, dir.to_path_buf());
        request.file_work.fail_flush_after(0);
        let run = download(
            request,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        );
        let result = tokio::time::timeout(PATIENCE, run)
            .await
            .expect("the download hung");
        assert_flush_error(&result);
        dir.remove().unwrap();
    }
}
