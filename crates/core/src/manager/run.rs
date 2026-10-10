//! Running the engine on one download and recording how the run ended.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use tokio::runtime::Handle;
use tracing::{debug, warn};
use wirecatch_engine::header::{HeaderMap, HeaderName, HeaderValue};
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error as EngineError, Outcome, Progress, RateLimiter, Url,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, UserError};

use super::state::{Entry, Run, State, Stop};
use super::{Inner, Manager};
use crate::naming::partial_files;
use crate::speed::SpeedMeter;
use crate::store::Origin;
use crate::user_error;

/// How a run ended.
pub(super) enum RunEnd {
    /// The engine returned.
    Engine(Result<Outcome, EngineError>),
    /// The run's task ended without the engine returning: it panicked or was aborted.
    Vanished,
}

impl Manager {
    /// Starts the engine on `id`. The caller has already checked every limit.
    pub(super) fn start_run(&self, state: &mut State, id: DownloadId) {
        let connections = usize::from(state.settings.connections_per_download);
        let Some(entry) = state.downloads.get(&id) else {
            return;
        };
        let queue_limiter = state
            .queues
            .iter()
            .find(|s| s.queue.id == entry.row.queue)
            .map(|s| s.limiter.clone());
        let limiters = [Some(self.inner.global_limiter.clone()), queue_limiter]
            .into_iter()
            .flatten()
            .collect();
        match engine_request(entry, connections, limiters) {
            Ok(request) => self.launch(state, id, move |progress, cancel| {
                wirecatch_engine::download(request, progress, cancel)
            }),
            Err(error) => {
                if let Some(entry) = state.downloads.get_mut(&id) {
                    entry.row.status = DownloadStatus::Failed;
                    entry.row.error = Some(error);
                }
                self.save(state, id);
            }
        }
    }

    /// Runs `engine` as the run of `id`. Kept apart from [`start_run`](Self::start_run)
    /// so tests can stand in for the engine.
    fn launch<F, Fut>(&self, state: &mut State, id: DownloadId, engine: F)
    where
        F: FnOnce(Arc<Progress>, CancellationToken) -> Fut,
        Fut: Future<Output = Result<Outcome, EngineError>> + Send + 'static,
    {
        let run_id = state.ids.run();
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let (cancel, exited) = (CancellationToken::new(), CancellationToken::new());
        let progress = Arc::new(Progress::default());
        let guard = RunGuard {
            manager: Arc::downgrade(&self.inner),
            runtime: self.inner.runtime.clone(),
            id,
            run_id,
            exited: exited.clone(),
            reported: false,
        };
        let work = engine(progress.clone(), cancel.clone());
        let task = self.spawn(async move {
            let result = work.await;
            guard.report(RunEnd::Engine(result));
        });
        entry.run = Some(Run {
            id: run_id,
            cancel,
            exited,
            progress,
            stop: None,
            meter: SpeedMeter::default(),
            task: task.abort_handle(),
        });
        entry.resume_first = false;
        entry.row.status = DownloadStatus::Connecting;
        entry.row.error = None;
        debug!(id = id.0, "download started");
        self.save(state, id);
    }

    /// Records how a run ended and gives its slot to the next download.
    fn finish(&self, id: DownloadId, run_id: u64, end: RunEnd) {
        let now = self.inner.clock.unix_millis();
        let mut state = self.lock();
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        if entry.run.as_ref().is_none_or(|run| run.id != run_id) {
            return;
        }
        let Some(run) = entry.run.take() else {
            return;
        };
        let started = run.progress.file_name().is_some();
        let worked_before = started || entry.row.done > 0;
        if started {
            entry.row.done = run.progress.done();
            entry.row.total = run.progress.total();
        }
        entry.clear_live_fields();

        let mut discard_partial = false;
        match (end, run.stop) {
            (RunEnd::Engine(Ok(Outcome::Completed { bytes, .. })), _) => {
                entry.row.status = DownloadStatus::Completed;
                entry.row.total = Some(bytes);
                entry.row.done = bytes;
                entry.row.completed_at = Some(now);
                entry.row.error = None;
            }
            (_, Some(Stop::Pause | Stop::Remove))
            | (RunEnd::Engine(Ok(Outcome::Paused { .. })), None) => {
                entry.row.status = DownloadStatus::Paused;
            }
            (_, Some(Stop::Requeue)) => entry.row.status = DownloadStatus::Queued,
            // The status stays as it was, so the next start continues it.
            (_, Some(Stop::Shutdown)) => {}
            (_, Some(Stop::Cancel)) => {
                mark_cancelled(entry);
                discard_partial = true;
            }
            (RunEnd::Engine(Err(error)), None) => {
                warn!(id = id.0, %error, "download failed");
                let folder = entry.path().parent().unwrap_or(Path::new("")).to_path_buf();
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(user_error::describe(&error, &folder, worked_before));
            }
            (RunEnd::Vanished, None) => {
                warn!(id = id.0, "download task ended without a result");
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(user_error::stopped_unexpectedly());
            }
        }
        let partial = discard_partial.then(|| partial_files(entry.path()));
        self.save(&mut state, id);
        self.reconcile(&mut state);
        drop(state);
        if let Some(files) = partial {
            self.delete_in_background(files.to_vec());
        }
    }
}

/// Puts a download in the cancelled state, with nothing kept.
pub(super) fn mark_cancelled(entry: &mut Entry) {
    entry.row.status = DownloadStatus::Cancelled;
    entry.row.error = None;
    entry.row.done = 0;
}

/// Deletes files, ignoring ones already gone. Blocks on the disk.
pub(super) fn delete_files(paths: &[PathBuf]) {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!(path = %path.display(), error = %e, "could not delete file"),
        }
    }
}

/// Reports the end of a run exactly once, however its task ends. If the task
/// panics or is aborted, dropping the guard reports the run as vanished, so the
/// download never stays "running" with nothing behind it.
struct RunGuard {
    manager: Weak<Inner>,
    runtime: Handle,
    id: DownloadId,
    run_id: u64,
    exited: CancellationToken,
    reported: bool,
}

impl RunGuard {
    /// Records the engine's result and wakes everyone waiting for the run to end.
    fn report(mut self, end: RunEnd) {
        self.reported = true;
        if let Some(manager) = Manager::from_weak(&self.manager) {
            manager.finish(self.id, self.run_id, end);
        }
        self.exited.cancel();
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.reported {
            return;
        }
        // Reported from a fresh task: this drop may run while the manager's lock is
        // held (a task dropped on a runtime that is shutting down), and then the
        // fresh task is simply never run.
        let (manager, id, run_id) = (self.manager.clone(), self.id, self.run_id);
        let exited = self.exited.clone();
        self.runtime.spawn(async move {
            if let Some(manager) = Manager::from_weak(&manager) {
                manager.finish(id, run_id, RunEnd::Vanished);
            }
            exited.cancel();
        });
    }
}

/// The engine request for a download, always saving to the path the manager chose.
fn engine_request(
    entry: &Entry,
    connections: usize,
    rate_limiters: Vec<Arc<RateLimiter>>,
) -> Result<DownloadRequest, UserError> {
    let url = Url::parse(&entry.row.url).map_err(|e| {
        user_error::describe(
            &EngineError::InvalidUrl(e.to_string()),
            Path::new(""),
            false,
        )
    })?;
    let path = PathBuf::from(&entry.row.path);
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut request = DownloadRequest::new(url, dir);
    request.exact_path = Some(path);
    let options = &mut request.options;
    options.connections = connections;
    options.rate_limiters = rate_limiters;
    options.headers = header_map(&entry.origin);
    if let Some(agent) = &entry.origin.user_agent {
        options.user_agent = agent.clone();
    }
    Ok(request)
}

/// Request headers from what the browser handed over. A header that is not valid
/// HTTP is left out rather than failing the download.
fn header_map(origin: &Origin) -> HeaderMap {
    let mut map = HeaderMap::with_capacity(origin.headers.len());
    for header in &origin.headers {
        match (
            HeaderName::from_bytes(header.name.trim().as_bytes()),
            HeaderValue::from_str(header.value.trim()),
        ) {
            (Ok(name), Ok(value)) => {
                map.append(name, value);
            }
            _ => warn!(name = %header.name, "skipping a header that is not valid HTTP"),
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{KnownFolders, ManagerConfig};
    use wirecatch_protocol::app::AddRequest;
    use wirecatch_protocol::extension::Header;

    async fn paused_download() -> (Manager, DownloadId, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let folders = KnownFolders::under_home(dir.path().join("home"));
        let config = ManagerConfig::new(dir.path().join("data"), folders);
        let manager = Manager::open(config).await.unwrap();
        let row = manager
            .add_download(AddRequest {
                url: "https://example.com/a.bin".into(),
                queue: None,
                folder: None,
                file_name: None,
                start: false,
            })
            .await
            .unwrap();
        (manager, row.id, dir)
    }

    async fn panicking_engine() -> Result<Outcome, EngineError> {
        panic!("a bug in the engine");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_that_panics_is_recorded_as_failed() {
        let (manager, id, _dir) = paused_download().await;
        let exited = {
            let mut state = manager.lock();
            manager.launch(&mut state, id, |_, _| panicking_engine());
            state.downloads[&id].run.as_ref().unwrap().exited.clone()
        };
        tokio::time::timeout(Duration::from_secs(5), exited.cancelled())
            .await
            .expect("waiters were never told the run ended");
        let state = manager.lock();
        let entry = &state.downloads[&id];
        assert!(entry.run.is_none(), "the slot is still taken");
        assert_eq!(entry.row.status, DownloadStatus::Failed);
        assert_eq!(entry.row.error, Some(user_error::stopped_unexpectedly()));
    }

    #[test]
    fn invalid_headers_are_skipped() {
        let origin = Origin {
            user_agent: None,
            headers: vec![
                Header {
                    name: "Cookie".into(),
                    value: "a=b".into(),
                },
                Header {
                    name: "Bad Name".into(),
                    value: "x".into(),
                },
                Header {
                    name: "X-Ok".into(),
                    value: "line\nbreak".into(),
                },
            ],
        };
        let map = header_map(&origin);
        assert_eq!(map.len(), 1);
        assert_eq!(map["cookie"], "a=b");
    }
}
