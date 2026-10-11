//! Running the engine (or, for streams and media pages, a media job) on one
//! download and recording how the run ended.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use tokio::runtime::Handle;
use tracing::{debug, warn};

use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error as EngineError, Outcome, PathResolver, Progress,
    RateLimiter, Url,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, UserError};

use super::settle::path_resolver;
use super::state::{Entry, Gauge, Run, State, Stop};
use super::{Inner, Manager};
use crate::headers;
use crate::speed::SpeedMeter;
use crate::store::Origin;
use crate::user_error;

/// How a run ended.
pub(super) enum RunEnd {
    /// The engine returned.
    Engine(Result<Outcome, EngineError>),
    /// A media job returned.
    Media(Result<MediaDone, UserError>),
    /// The run's task ended without the engine returning: it panicked or was aborted.
    Vanished,
}

/// How a media job that did not fail ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum MediaDone {
    /// The file is complete at `path`, which may differ from the planned path
    /// in its extension.
    Completed { path: PathBuf, bytes: u64 },
    /// Stopped; starting it again continues.
    Paused,
}

/// A run's end, in the terms that decide the download's new state.
enum Ended {
    Completed {
        bytes: u64,
        path: Option<PathBuf>,
    },
    /// Ended without finishing or failing (cancelled, or an outcome added later).
    Stopped,
    Failed(UserError),
    Vanished,
}

impl Manager {
    /// Starts the engine on `id`. The caller has already checked every limit.
    pub(super) fn start_run(&self, state: &mut State, id: DownloadId) {
        // A sign-in older than a week is never sent, even before the hourly
        // look at old downloads has got to it.
        let now = self.inner.clock.unix_millis();
        if let Some(entry) = state.downloads.get_mut(&id)
            && super::sign_in::forget_if_old(entry, now)
        {
            self.save(state, id);
        }
        let run_id = state.ids.run();
        let answered = CancellationToken::new();
        let connections = usize::from(state.settings.connections_per_download);
        let Some(entry) = state.downloads.get(&id) else {
            return;
        };
        let queue_limiter = state
            .queues
            .iter()
            .find(|s| s.queue.id == entry.row.queue)
            .map(|s| s.limiter.clone());
        let limiters: Vec<Arc<RateLimiter>> =
            [Some(self.inner.global_limiter.clone()), queue_limiter]
                .into_iter()
                .flatten()
                .collect();
        if let Some(job) = entry.media_job.clone() {
            self.start_media_run(state, id, run_id, answered, job, connections, limiters);
            return;
        }
        // Until the server has answered once, the path is only a guess: let the
        // server's answer decide it. Afterwards it is fixed.
        let settle = (!entry.naming.settled).then(|| self.settler(id, run_id, answered.clone()));
        let target = match &settle {
            None => Target::Exact,
            Some(settle) => Target::Resolve(path_resolver(settle.clone())),
        };
        let mark = Mark {
            url: entry.row.url.clone(),
            referrer: headers::referrer(&entry.origin.headers).map(str::to_string),
        };
        match engine_request(entry, connections, limiters, target) {
            #[cfg(test)]
            Ok(_) if self.fake_engine().is_some() => {
                let fake = self.fake_engine();
                self.launch(state, id, run_id, answered, move |cancel| {
                    let work = super::fake::run(fake, settle, cancel);
                    (Gauge::Engine(Arc::default()), async move {
                        RunEnd::Engine(work.await)
                    })
                });
            }
            Ok(request) => self.launch(state, id, run_id, answered, move |cancel| {
                let progress = Arc::new(Progress::default());
                let work = download_marked(request, progress.clone(), cancel, mark);
                (Gauge::Engine(progress), async move {
                    RunEnd::Engine(work.await)
                })
            }),
            Err(error) => self.fail_to_start(state, id, error),
        }
    }

    /// Records that a download could not even start.
    pub(super) fn fail_to_start(&self, state: &mut State, id: DownloadId, error: UserError) {
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.row.status = DownloadStatus::Failed;
            entry.row.error = Some(super::sign_in::explain(&entry.origin, error));
        }
        self.save(state, id);
    }

    /// Runs `work` as the run of `id`. `work` gets the run's cancellation token
    /// and returns where its progress is read and the future that does the work.
    pub(super) fn launch<F, Fut>(
        &self,
        state: &mut State,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
        work: F,
    ) where
        F: FnOnce(CancellationToken) -> (Gauge, Fut),
        Fut: Future<Output = RunEnd> + Send + 'static,
    {
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let (cancel, exited) = (CancellationToken::new(), CancellationToken::new());
        let guard = RunGuard {
            manager: Arc::downgrade(&self.inner),
            runtime: self.inner.runtime.clone(),
            id,
            run_id,
            exited: exited.clone(),
            reported: false,
        };
        let (gauge, work) = work(cancel.clone());
        let cleanup = entry.cleanup.take();
        let task = self.spawn(async move {
            if let Some(deleted) = cleanup {
                deleted.cancelled().await;
            }
            let end = work.await;
            guard.report(end);
        });
        entry.run = Some(Run {
            id: run_id,
            answered,
            cancel,
            exited,
            gauge,
            marked: false,
            stop: None,
            meter: SpeedMeter::default(),
            task: task.abort_handle(),
        });
        entry.resume_first = false;
        entry.row.status = DownloadStatus::Connecting;
        entry.row.error = None;
        state.running.insert(id);
        debug!(id = id.0, "download started");
        self.save(state, id);
    }

    /// Records how a run ended and gives its slot to the next download.
    fn finish(&self, id: DownloadId, run_id: u64, end: RunEnd) {
        let now = self.inner.clock.unix_millis();
        let mut state = self.lock();
        let state = &mut *state;
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        if entry.run.as_ref().is_none_or(|run| run.id != run_id) {
            return;
        }
        let Some(run) = entry.run.take() else {
            return;
        };
        state.running.remove(&id);
        let reading = run.gauge.read();
        let started = reading.answered;
        let worked_before = started || entry.row.done > 0;
        if started {
            entry.row.done = reading.done;
            entry.row.total = reading.total;
        }
        entry.clear_live_fields();

        let ended = match end {
            RunEnd::Engine(Ok(Outcome::Completed { bytes, .. })) => {
                Ended::Completed { bytes, path: None }
            }
            RunEnd::Engine(Ok(_)) | RunEnd::Media(Ok(MediaDone::Paused)) => Ended::Stopped,
            RunEnd::Engine(Err(error)) => {
                warn!(id = id.0, %error, "download failed");
                let folder = entry.path().parent().unwrap_or(Path::new("")).to_path_buf();
                Ended::Failed(user_error::describe(&error, &folder, worked_before))
            }
            RunEnd::Media(Ok(MediaDone::Completed { path, bytes })) => Ended::Completed {
                bytes,
                path: Some(path),
            },
            RunEnd::Media(Err(error)) => {
                warn!(id = id.0, error = %error.message, "media download failed");
                Ended::Failed(error)
            }
            RunEnd::Vanished => Ended::Vanished,
        };

        match (ended, run.stop) {
            (Ended::Completed { bytes, path }, _) => {
                // The finished file now holds the name on disk.
                state.claimed.remove(entry.path());
                if let Some(path) = path {
                    entry.row.file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    entry.row.path = path.to_string_lossy().into_owned();
                    // Reserved by the media job while it put the file in place.
                    state.claimed.remove(entry.path());
                }
                // Already marked as downloaded from the internet: the run did
                // that before it reported the file (see `download_marked` and
                // the media job's `finish`).
                entry.row.status = DownloadStatus::Completed;
                entry.row.total = Some(bytes);
                entry.row.done = bytes;
                entry.row.completed_at = Some(now);
                // Cookies and headers were only needed to download it.
                entry.origin = Origin::default();
                entry.row.error = None;
            }
            // A cancelled run that was not asked to stop, or an outcome added to the
            // engine later: keep it paused, so nothing is lost.
            (_, Some(Stop::Pause | Stop::Remove)) | (Ended::Stopped, None) => {
                entry.row.status = DownloadStatus::Paused;
            }
            (_, Some(Stop::Requeue)) => entry.row.status = DownloadStatus::Queued,
            // The status stays as it was, so the next start continues it.
            (_, Some(Stop::Shutdown)) => {
                if entry.row.status == DownloadStatus::Processing {
                    entry.row.status = DownloadStatus::Downloading;
                }
            }
            (_, Some(Stop::Cancel)) => {
                mark_cancelled(entry);
                self.discard_partial(entry);
            }
            (_, Some(Stop::Restart)) => {
                mark_cancelled(entry);
                self.discard_partial(entry);
                entry.row.status = DownloadStatus::Queued;
            }
            (Ended::Failed(error), None) => {
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(super::sign_in::explain(&entry.origin, error));
            }
            (Ended::Vanished, None) => {
                warn!(id = id.0, "download task ended without a result");
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(user_error::stopped_unexpectedly());
            }
        }
        self.save(state, id);
        self.reconcile(state);
    }

    /// Deletes a download's partial files in the background. A run started before
    /// the deletion has finished waits for it, so it never writes into a file that
    /// is being deleted.
    pub(super) fn discard_partial(&self, entry: &mut Entry) {
        let files = entry.partial_paths();
        let deleted = CancellationToken::new();
        entry.cleanup = Some(deleted.clone());
        self.inner.runtime.spawn_blocking(move || {
            delete_files(&files);
            deleted.cancel();
        });
    }
}

/// Puts a download in the cancelled state, with nothing kept.
pub(super) fn mark_cancelled(entry: &mut Entry) {
    entry.row.status = DownloadStatus::Cancelled;
    entry.row.error = None;
    entry.row.done = 0;
}

/// Deletes files and folders, ignoring ones already gone. Blocks on the disk.
pub(super) fn delete_files(paths: &[PathBuf]) {
    for path in paths {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
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

/// Where a run saves its file.
enum Target {
    /// At the download's settled path.
    Exact,
    /// Wherever this resolver decides once the server has answered.
    Resolve(PathResolver),
}

/// What the Mark of the Web of a download says: where it came from.
pub(super) struct Mark {
    pub url: String,
    pub referrer: Option<String>,
}

/// Runs the engine on `request`. A finished file is marked as downloaded from
/// the internet before the run reports it, so it is never shown as finished
/// (or opened from the app) without the mark. The part file is normally marked
/// already while it downloads (see the ticker), and the mark moves with it when
/// the engine renames it; this covers a download that finishes before that.
async fn download_marked(
    request: DownloadRequest,
    progress: Arc<Progress>,
    cancel: CancellationToken,
    mark: Mark,
) -> Result<Outcome, EngineError> {
    let result = wirecatch_engine::download(request, progress, cancel).await;
    if let Ok(Outcome::Completed { path, .. }) = &result {
        let path = path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::zone::mark(&path, &mark.url, mark.referrer.as_deref());
        })
        .await;
    }
    result
}

/// The engine request for a download. The manager always decides the path, either
/// up front or through its resolver.
fn engine_request(
    entry: &Entry,
    connections: usize,
    rate_limiters: Vec<Arc<RateLimiter>>,
    target: Target,
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
    match target {
        Target::Exact => request.exact_path = Some(path),
        Target::Resolve(resolver) => request.resolve_path = Some(resolver),
    }
    let options = &mut request.options;
    options.connections = connections;
    options.rate_limiters = rate_limiters;
    options.headers = headers::to_map(&entry.origin.headers);
    if let Some(agent) = &entry.origin.user_agent {
        options.user_agent = agent.clone();
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{KnownFolders, ManagerConfig};
    use wirecatch_protocol::app::AddRequest;

    /// A file served at `/file.bin` on a local server; returns its address.
    /// Only the Windows Mark-of-the-Web test needs it.
    #[cfg(windows)]
    async fn serve_file(body: Vec<u8>) -> String {
        let app = axum::Router::new().route(
            "/file.bin",
            axum::routing::get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/file.bin")
    }

    /// A download so small it finishes before the ticker could mark its part
    /// file is still marked before the run reports it finished.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_download_is_marked_before_it_is_reported_finished() {
        let url = serve_file(b"tiny".to_vec()).await;
        let dir = tempfile::tempdir().unwrap();
        let mut request = DownloadRequest::new(Url::parse(&url).unwrap(), dir.path());
        request.exact_path = Some(dir.path().join("tiny.bin"));
        let mark = Mark {
            url: format!("{url}?token=secret"),
            referrer: Some("https://example.com/page".into()),
        };
        let outcome = download_marked(request, Arc::default(), CancellationToken::new(), mark)
            .await
            .unwrap();
        let Outcome::Completed { path, .. } = outcome else {
            panic!("not finished: {outcome:?}");
        };
        // Read at once: the run has reported the file, so the mark is there.
        let text = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display()))
            .expect("the finished file was reported without the mark");
        assert!(text.contains("ZoneId=3"), "{text}");
        assert!(
            text.contains("ReferrerUrl=https://example.com/page"),
            "{text}"
        );
        assert!(!text.contains("secret"), "{text}");
    }

    async fn paused_download() -> (Manager, DownloadId, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let folders = KnownFolders::under_home(dir.path().join("home"));
        let config = ManagerConfig::new(dir.path().join("data"), folders);
        let manager = Manager::open(config).await.unwrap();
        let id = manager
            .add_download(AddRequest {
                url: "https://example.com/a.bin".into(),
                queue: None,
                folder: None,
                file_name: None,
                start: false,
            })
            .await
            .unwrap();
        (manager, id, dir)
    }

    async fn panicking_engine() -> RunEnd {
        panic!("a bug in the engine");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_that_panics_is_recorded_as_failed() {
        let (manager, id, _dir) = paused_download().await;
        let exited = {
            let mut state = manager.lock();
            let run_id = state.ids.run();
            let answered = CancellationToken::new();
            manager.launch(&mut state, id, run_id, answered, |_| {
                (Gauge::Engine(Arc::default()), panicking_engine())
            });
            state.downloads[&id].run.as_ref().unwrap().exited.clone()
        };
        tokio::time::timeout(Duration::from_secs(5), exited.cancelled())
            .await
            .expect("waiters were never told the run ended");
        let state = manager.lock();
        let entry = &state.downloads[&id];
        assert!(entry.run.is_none(), "the slot is still taken");
        state.assert_consistent();
        assert_eq!(entry.row.status, DownloadStatus::Failed);
        assert_eq!(entry.row.error, Some(user_error::stopped_unexpectedly()));
    }
}
