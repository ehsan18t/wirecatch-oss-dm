//! Running the engine (or, for streams and media pages, a media job) on one
//! download and recording how the run ended.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use tokio::runtime::Handle;
use tracing::{debug, warn};

use wirecatch_engine::{
    CancellationToken, DownloadRequest, Error as EngineError, ExistingFile, FileWork, Outcome,
    PathResolver, Progress, Url,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, UserError};

use super::added_pages::AnsweredPage;
use super::policy::RunPolicy;
use super::settle::path_resolver;
use super::state::{Entry, Gauge, OnExisting, Run, State, Stop};
use super::{Inner, Manager};
use crate::headers;
use crate::headers::Origin;
use crate::naming::Claimed;
use crate::speed::SpeedMeter;
use wirecatch_sites::local_network;

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

impl Ended {
    /// How run `end` of download `id`, saving into `folder`, ended. A failure
    /// is described for the person, in words that depend on whether anything
    /// had arrived before it (`worked_before`), on who chose the link and on the proxy it went through.
    fn of(end: RunEnd, id: DownloadId, folder: &Path, worked_before: bool, entry: &Entry) -> Self {
        match end {
            RunEnd::Engine(Ok(Outcome::Completed { bytes, .. })) => {
                Ended::Completed { bytes, path: None }
            }
            RunEnd::Engine(Ok(_)) | RunEnd::Media(Ok(MediaDone::Paused)) => Ended::Stopped,
            RunEnd::Engine(Err(error)) => {
                warn!(id = id.0, %error, "download failed");
                Ended::Failed(wirecatch_messages::describe_run(
                    &error,
                    folder,
                    worked_before,
                    entry.chosen_by,
                    entry.through_proxy.as_deref(),
                ))
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
        }
    }
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
        let Some((queue, url)) = state
            .downloads
            .get(&id)
            .map(|e| (e.row.queue, e.row.url.clone()))
        else {
            return;
        };
        let (label, through) = state.proxy_route(queue, &url);
        if let Some(entry) = state.downloads.get_mut(&id) {
            entry.row.proxy = Some(label);
            entry.through_proxy = through;
        }
        let Some(entry) = state.downloads.get(&id) else {
            return;
        };
        let policy = self.run_policy(state, entry.row.queue);
        if let Some(job) = entry.media_job.clone() {
            self.start_media_run(state, id, run_id, answered, job, policy);
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
        match engine_request(entry, policy, target) {
            #[cfg(test)]
            Ok(request) if self.fake_engine().is_some() => {
                let fake = self.fake_engine();
                let rule = request.options.private_network;
                self.launch(state, id, run_id, answered, move |cancel, _| {
                    let work = super::fake::run(fake, settle, cancel, rule);
                    (Gauge::Engine(Arc::default()), async move {
                        RunEnd::Engine(work.await)
                    })
                });
            }
            Ok(mut request) => {
                self.launch(state, id, run_id, answered, move |cancel, file_work| {
                    request.file_work = file_work;
                    let progress = Arc::new(Progress::default());
                    let work = download_marked(request, progress.clone(), cancel, mark);
                    (Gauge::Engine(progress), async move {
                        RunEnd::Engine(work.await)
                    })
                });
            }
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

    /// Runs `work` as the run of `id`. `work` gets the run's cancellation token and the
    /// file work its engine downloads share, and returns where its progress is read and the
    /// future that does the work.
    pub(super) fn launch<F, Fut>(
        &self,
        state: &mut State,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
        work: F,
    ) where
        F: FnOnce(CancellationToken, FileWork) -> (Gauge, Fut),
        Fut: Future<Output = RunEnd> + Send + 'static,
    {
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let (cancel, exited) = (CancellationToken::new(), CancellationToken::new());
        let file_work = FileWork::new();
        let guard = RunGuard {
            manager: Arc::downgrade(&self.inner),
            runtime: self.inner.runtime.clone(),
            id,
            run_id,
            exited: exited.clone(),
            file_work: file_work.clone(),
            reported: false,
        };
        let (gauge, work) = work(cancel.clone(), file_work);
        let cleanup = entry.cleanup.take();
        let task = self.spawn(async move {
            if let Some(deleted) = cleanup {
                deleted.cancelled().await;
            }
            let end = work.await;
            guard.report(end).await;
        });
        entry.run = Some(Run {
            id: run_id,
            answered,
            cancel,
            exited,
            gauge,
            marked: false,
            stop: None,
            found: None,
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
        let mut guard = self.lock();
        let state = &mut *guard;
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        if entry.run.as_ref().is_none_or(|run| run.id != run_id) {
            return;
        }
        let Some(mut run) = entry.run.take() else {
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
        // A file saved under its name meanwhile holds the path: it moves to the next free name.
        if run.stop.is_none() && matches!(end, RunEnd::Engine(Err(EngineError::TargetTaken { .. })))
        {
            entry.held_for_another_name();
            self.save(state, id);
            drop(guard);
            self.take_next_free_name(id);
            return;
        }
        let found = run.found.take();

        let folder = entry.path().parent().unwrap_or(Path::new("")).to_path_buf();
        let ended = Ended::of(end, id, &folder, worked_before, entry);
        let mut answered_page = None;
        match (ended, run.stop) {
            (Ended::Completed { bytes, path }, _) => {
                complete(&mut state.claimed, entry, bytes, path, now);
            }
            // A cancelled run that was not asked to stop, or an outcome added to the
            // engine later: keep it paused, so nothing is lost.
            (_, Some(Stop::Pause | Stop::Remove)) | (Ended::Stopped, None) => {
                entry.row.status = DownloadStatus::Paused;
            }
            (_, Some(Stop::Requeue)) => entry.row.status = DownloadStatus::Queued,
            // Its server answered with a web page: the download gives way to reading the page.
            (_, Some(Stop::ReadAsVideo)) => answered_page = AnsweredPage::of(entry),
            // Its file is already in the folder: handled below, with the whole state.
            (_, Some(Stop::FileExists)) => {}
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
                entry.row.error = Some(wirecatch_messages::stopped_unexpectedly());
            }
        }
        if let Some(found) = found {
            self.on_found(state, id, &found);
        }
        self.save(state, id);
        self.reconcile(state);
        drop(guard);
        if let Some(page) = answered_page {
            self.read_answered_page(id, page);
        }
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

/// Records a run that finished its file, `bytes` long, at `path` when that is not the planned one.
fn complete(claimed: &mut Claimed, entry: &mut Entry, bytes: u64, path: Option<PathBuf>, now: i64) {
    // The finished file now holds the name on disk.
    claimed.remove(entry.path());
    if let Some(path) = path {
        entry.row.file_name = crate::naming::file_name_of(&path);
        entry.row.path = path.to_string_lossy().into_owned();
        // Reserved by the media job while it put the file in place.
        claimed.remove(entry.path());
    }
    // Already marked as downloaded from the internet: the run did that before it reported the
    // file (see `download_marked` and the media job's `finish`).
    mark_completed(entry, bytes, now);
}

/// Puts a download in the completed state, its file `bytes` long.
pub(super) fn mark_completed(entry: &mut Entry, bytes: u64, now: i64) {
    entry.row.status = DownloadStatus::Completed;
    // The scheduler no longer visits it, so nothing would clear a reason left behind.
    entry.row.waiting = None;
    entry.row.total = Some(bytes);
    entry.row.done = bytes;
    entry.row.completed_at = Some(now);
    entry.row.error = None;
    // Cookies and headers were only needed to download it.
    entry.origin = Origin::default();
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
        if path.is_dir() {
            wirecatch_files::discard::dir(path);
        } else {
            wirecatch_files::discard::file(path);
        }
    }
}

/// Reports the end of a run exactly once, however its task ends. If the task
/// panics or is aborted, dropping the guard reports the run as vanished, so the
/// download never stays "running" with nothing behind it. Either way only once the
/// run's file work has finished: until then a new run or a deletion could meet it.
struct RunGuard {
    manager: Weak<Inner>,
    runtime: Handle,
    id: DownloadId,
    run_id: u64,
    exited: CancellationToken,
    file_work: FileWork,
    reported: bool,
}

impl RunGuard {
    /// Records the engine's result and wakes everyone waiting for the run to end.
    async fn report(mut self, end: RunEnd) {
        self.file_work.finished().await;
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
        let (exited, file_work) = (self.exited.clone(), self.file_work.clone());
        self.runtime.spawn(async move {
            file_work.finished().await;
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
        let marking = tokio::task::spawn_blocking(move || {
            wirecatch_files::zone::mark(&path, &mark.url, mark.referrer.as_deref());
        });
        if let Err(e) = marking.await {
            warn!(error = %e, "could not mark the finished file as downloaded from the internet");
        }
    }
    result
}

/// The engine request for a download, run as `policy` says. The manager always decides the
/// path, either up front or through its resolver.
fn engine_request(
    entry: &Entry,
    policy: RunPolicy,
    target: Target,
) -> Result<DownloadRequest, UserError> {
    let url =
        Url::parse(&entry.row.url).map_err(|e| wirecatch_messages::invalid_link(&e.to_string()))?;
    if local_network::refuses_address(entry.chosen_by, &url) {
        return Err(wirecatch_messages::private_network(entry.chosen_by));
    }
    let path = PathBuf::from(&entry.row.path);
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut request = DownloadRequest::new(url, dir);
    match target {
        Target::Exact => request.exact_path = Some(path),
        Target::Resolve(resolver) => request.resolve_path = Some(resolver),
    }
    // Never over a file the person did not choose to replace (D88), which ENG-11's file saved
    // under its name meanwhile would otherwise be.
    request.if_exists = if entry.on_existing == Some(OnExisting::Replace) {
        ExistingFile::Replace
    } else {
        ExistingFile::Refuse
    };
    request.options = headers::download_options(entry.chosen_by.private_network(), policy.proxy);
    let options = &mut request.options;
    options.connections = policy.connections;
    options.rate_limiters = policy.limiters;
    options.headers = headers::to_map(&entry.origin.headers);
    if let Some(agent) = &entry.origin.user_agent {
        options.user_agent.clone_from(agent);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KnownFolders, ManagerConfig};
    use wirecatch_engine::{PrivateNetwork, ProxyChoice};
    use wirecatch_protocol::app::AddRequest;
    use wirecatch_sites::local_network::ChosenBy;

    /// A website's link written as a local address is refused before it
    /// starts: the engine's `Block` lets a starting address through (D34.4),
    /// for its caller to classify (D39). Every other link runs under its rule.
    #[test]
    fn a_download_runs_under_the_rule_of_whoever_chose_its_link() {
        let policy = || RunPolicy {
            connections: 1,
            limiters: Vec::new(),
            proxy: ProxyChoice::System,
        };
        let run = |url: &str, chosen_by| {
            engine_request(
                &super::super::state::idle_entry(url, chosen_by),
                policy(),
                Target::Exact,
            )
        };
        let refused = run("http://192.168.1.1/f", ChosenBy::Website).unwrap_err();
        assert_eq!(
            refused,
            wirecatch_messages::private_network(ChosenBy::Website)
        );
        for (chosen_by, rule) in [
            (ChosenBy::Website, PrivateNetwork::Block),
            (ChosenBy::LocalPage, PrivateNetwork::Auto),
            (ChosenBy::Person, PrivateNetwork::Auto),
        ] {
            let request = run("http://192.168.1.1/f", chosen_by);
            if chosen_by != ChosenBy::Website {
                assert_eq!(request.unwrap().options.private_network, rule);
            }
            let request = run("https://example.com/f", chosen_by).unwrap();
            assert_eq!(request.options.private_network, rule, "{chosen_by:?}");
        }
    }

    /// A download so small it finishes before the ticker could mark its part
    /// file is still marked before the run reports it finished.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_download_is_marked_before_it_is_reported_finished() {
        let server = wirecatch_test_support::files::FileServer::start().await;
        server.put("file.bin", b"tiny".as_slice());
        let url = server.url("file.bin");
        let dir = tempfile::tempdir().unwrap();
        let mut request = DownloadRequest::new(url.clone(), dir.path());
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

    /// A run that panics once it is polled, as a bug in the engine would.
    fn panicking_engine() -> impl std::future::Future<Output = RunEnd> {
        std::future::poll_fn(|_| panic!("a bug in the engine"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_that_panics_is_recorded_as_failed() {
        let (manager, id, _dir) = paused_download().await;
        let exited = {
            let mut state = manager.lock();
            let run_id = state.ids.run();
            let answered = CancellationToken::new();
            manager.launch(&mut state, id, run_id, answered, |_, _| {
                (Gauge::Engine(Arc::default()), panicking_engine())
            });
            state.downloads[&id].run.as_ref().unwrap().exited.clone()
        };
        tokio::time::timeout(wirecatch_test_support::PATIENCE, exited.cancelled())
            .await
            .expect("waiters were never told the run ended");
        let state = manager.lock();
        let entry = &state.downloads[&id];
        assert!(entry.run.is_none(), "the slot is still taken");
        state.assert_consistent();
        assert_eq!(entry.row.status, DownloadStatus::Failed);
        assert_eq!(
            entry.row.error,
            Some(wirecatch_messages::stopped_unexpectedly())
        );
    }

    /// A run that ends abruptly while its file work still runs is not over until that work is: a new run or a deletion would meet it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_ends_only_once_its_file_work_has() {
        let (manager, id, _dir) = paused_download().await;
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (started_tx, started) = tokio::sync::oneshot::channel();
        let (exited, task) = {
            let mut state = manager.lock();
            let run_id = state.ids.run();
            let answered = CancellationToken::new();
            manager.launch(&mut state, id, run_id, answered, |_, file_work| {
                drop(file_work.spawn_blocking(move || {
                    started_tx.send(()).unwrap();
                    held.recv().unwrap();
                }));
                (Gauge::Engine(Arc::default()), panicking_engine())
            });
            let run = state.downloads[&id].run.as_ref().unwrap();
            (run.exited.clone(), run.task.clone())
        };
        started.await.unwrap();
        tokio::time::timeout(wirecatch_test_support::PATIENCE, async {
            while !task.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the run's task never ended");
        assert!(
            tokio::time::timeout(wirecatch_test_support::QUIET, exited.cancelled())
                .await
                .is_err(),
            "the run was reported over while its file work still ran"
        );
        release.send(()).unwrap();
        tokio::time::timeout(wirecatch_test_support::PATIENCE, exited.cancelled())
            .await
            .expect("waiters were never told the run ended");
        assert!(manager.lock().downloads[&id].run.is_none());
    }
}
