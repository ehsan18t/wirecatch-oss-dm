//! The file system work a download queues on the blocking pool, which outlives the download's future when that is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Notify;

/// What a download wrote, saved, renamed or deleted on the blocking pool, for an owner that drops the download and then touches its files.
///
/// Work already queued when the download's future is dropped still runs. It changes no file once it notices (see [`Error::Cancelled`](crate::Error::Cancelled)), but a moment remains between that check and the change after it, and a write already under way finishes. An owner that drops a download and then deletes its files, or starts another download on the same path, waits for [`finished`](Self::finished) first.
///
/// Clones share the work: give one to [`DownloadRequest::file_work`](crate::DownloadRequest::file_work), or to several downloads to wait for all of them.
#[derive(Debug, Clone, Default)]
pub struct FileWork(Arc<Count>);

#[derive(Debug, Default)]
struct Count {
    running: AtomicUsize,
    idle: Notify,
}

/// Counts one piece of work as running until dropped.
struct Ticket(Arc<Count>);

impl Drop for Ticket {
    fn drop(&mut self) {
        if self.0.running.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

impl FileWork {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits until no work of these downloads is running or queued. Meant for after they ended or were dropped: work a running download queues later is not waited for.
    pub async fn finished(&self) {
        loop {
            let idle = self.0.idle.notified();
            tokio::pin!(idle);
            // Registered before the count is read, so a last ticket dropped in between still wakes it.
            idle.as_mut().enable();
            if self.0.running.load(Ordering::Acquire) == 0 {
                return;
            }
            idle.await;
        }
    }

    /// Runs `work` on the blocking pool as part of this work: an owner's own work on the download's files is waited for with it.
    pub fn spawn_blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> tokio::task::JoinHandle<T> {
        self.0.running.fetch_add(1, Ordering::AcqRel);
        let ticket = Ticket(self.0.clone());
        tokio::task::spawn_blocking(move || {
            let done = work();
            drop(ticket);
            done
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    /// Work queued by a task that is then aborted still runs; waiting for it covers it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn waits_for_work_whose_task_was_aborted() {
        let files = FileWork::new();
        let (release, held) = mpsc::channel::<()>();
        let (started_tx, started) = tokio::sync::oneshot::channel();
        let queued = files.clone();
        let task = tokio::spawn(async move {
            queued
                .spawn_blocking(move || {
                    started_tx.send(()).unwrap();
                    held.recv().unwrap();
                })
                .await
                .unwrap();
        });
        started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        let mut finished = Box::pin(files.finished());
        assert!(
            futures_poll_once(&mut finished).await.is_pending(),
            "finished while the work still ran"
        );
        release.send(()).unwrap();
        tokio::time::timeout(wirecatch_test_support::PATIENCE, finished)
            .await
            .expect("never finished");
    }

    async fn futures_poll_once<F: std::future::Future + Unpin>(
        future: &mut F,
    ) -> std::task::Poll<F::Output> {
        std::future::poll_fn(|cx| std::task::Poll::Ready(std::pin::Pin::new(&mut *future).poll(cx)))
            .await
    }
}
