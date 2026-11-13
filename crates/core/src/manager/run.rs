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
use wirecatch_messages::Worked;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{DownloadStatus, UserError};

use super::added_pages::AnsweredPage;
use super::file_exists::give_up_the_name;
use super::policy::RunPolicy;
use super::settle::path_resolver;
use super::state::{Entry, Found, Gauge, NextRun, OnExisting, Run, State, Stop};
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
    /// in its extension, with its subtitle files beside it.
    Completed {
        path: PathBuf,
        bytes: u64,
        sidecars: Vec<PathBuf>,
    },
    /// Stopped; starting it again continues.
    Paused,
    /// Its name is taken in the folder and the person is asked (D96); the answer starts it
    /// again, which puts the finished file in place.
    FileExists(Found),
    /// The parts are downloaded but could not be converted; they stay in the work folder,
    /// so "Keep the original" can save them as they came (D24).
    NotConverted(UserError),
}

/// What a run that finished leaves.
struct Finished {
    bytes: u64,
    /// Where a media job put the file, when not at the download's path.
    path: Option<PathBuf>,
    sidecars: Vec<PathBuf>,
}

/// A run's end, in the terms that decide the download's new state.
enum Ended {
    Completed(Finished),
    /// Ended without finishing or failing (cancelled, or an outcome added later).
    Stopped,
    Failed(UserError),
    NotConverted(UserError),
    Vanished,
}

impl Ended {
    /// How run `end` of download `id`, saving into `folder`, ended. A failure
    /// is described for the person, in words that depend on whether anything
    /// had arrived before it, on who chose the link and on the proxy it went through.
    fn of(end: RunEnd, id: DownloadId, folder: &Path, worked: Worked, entry: &Entry) -> Self {
        match end {
            RunEnd::Engine(Ok(Outcome::Completed { bytes, .. })) => Ended::Completed(Finished {
                bytes,
                path: None,
                sidecars: Vec::new(),
            }),
            RunEnd::Engine(Ok(_))
            | RunEnd::Media(Ok(MediaDone::Paused | MediaDone::FileExists(_))) => Ended::Stopped,
            RunEnd::Engine(Err(error)) => {
                warn!(id = id.0, %error, "download failed");
                Ended::Failed(wirecatch_messages::describe_run(
                    &error,
                    folder,
                    worked,
                    entry.chosen_by,
                    entry.through_proxy.as_deref(),
                ))
            }
            RunEnd::Media(Ok(MediaDone::Completed {
                path,
                bytes,
                sidecars,
            })) => Ended::Completed(Finished {
                bytes,
                path: Some(path),
                sidecars,
            }),
            RunEnd::Media(Err(error)) => {
                warn!(id = id.0, error = %error.message, "media download failed");
                Ended::Failed(error)
            }
            RunEnd::Media(Ok(MediaDone::NotConverted(error))) => {
                warn!(id = id.0, error = %error.message, "media conversion failed");
                Ended::NotConverted(error)
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
        // Until the server has answered once, the path is only a guess: let the server's answer decide it. Until a run begins writing, its answer also records that it did.
        // A fresh capture also checks the server's answer before touching an existing part.
        let settle = (!entry.naming.writing || entry.expect.is_some())
            .then(|| self.settler(id, run_id, answered.clone()));
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
        let (gauge, work) = work(cancel.clone(), file_work.clone());
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
            marking_done: false,
            file_work,
            stop: None,
            found: None,
            server_name: None,
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
        let worked = take_reading(entry, &run.gauge);
        // A file saved under its name meanwhile holds the path: it moves to the next free name.
        if run.stop.is_none() && matches!(end, RunEnd::Engine(Err(EngineError::TargetTaken { .. })))
        {
            entry.held_for_another_name();
            self.save(state, id);
            drop(guard);
            self.take_next_free_name(id);
            return;
        }
        let found = run.found.take().or_else(|| match &end {
            RunEnd::Media(Ok(MediaDone::FileExists(found))) => Some(found.clone()),
            _ => None,
        });

        let folder = entry.path().parent().unwrap_or(Path::new("")).to_path_buf();
        let ended = Ended::of(end, id, &folder, worked, entry);
        if let Some(job) = &mut entry.media_job {
            job.set_conversion_failed(matches!((&ended, run.stop), (Ended::NotConverted(_), None)));
        }
        let mut answered_page = None;
        let finished = matches!(ended, Ended::Completed(_));
        match (ended, run.stop) {
            (Ended::Completed(finished), _) => {
                complete(&mut state.claimed, entry, finished, now);
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
            (_, Some(Stop::FileExists | Stop::FileTypeChanged)) => {}
            // The status stays as it was, so the next start continues it.
            (_, Some(Stop::Shutdown)) => {
                if entry.row.status == DownloadStatus::Processing {
                    entry.row.status = DownloadStatus::Downloading;
                }
            }
            (_, Some(Stop::Cancel)) => mark_cancelled(entry),
            (_, Some(Stop::Restart)) => {
                mark_cancelled(entry);
                entry.row.status = DownloadStatus::Queued;
            }
            (Ended::Failed(error) | Ended::NotConverted(error), None) => {
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(super::sign_in::explain(&entry.origin, error));
            }
            (Ended::Vanished, None) => {
                warn!(id = id.0, "download task ended without a result");
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(wirecatch_messages::stopped_unexpectedly(worked));
            }
        }
        if !finished {
            self.discard_after_stop(state, id, run.stop.as_ref());
        }
        self.act_on_the_answer(state, id, run.stop, found, run.server_name.take());
        self.save(state, id);
        // First: the plan must see the read, or the run of a queue with nothing else left ends here (D155.3).
        if let Some(page) = answered_page {
            self.read_answered_page(state, id, page);
        }
        self.reconcile(state);
    }

    /// What the server's answer found, or the name it gave, if the run stopped for that. Whatever stopped it since decides instead, and a later run meets the same answer again.
    fn act_on_the_answer(
        &self,
        state: &mut State,
        id: DownloadId,
        stop: Option<Stop>,
        found: Option<Found>,
        server_name: Option<String>,
    ) {
        if let Some(found) = found
            && matches!(stop, None | Some(Stop::FileExists))
        {
            self.on_found(state, id, &found);
        }
        if let Some(server_name) = server_name
            && matches!(stop, Some(Stop::FileTypeChanged))
        {
            self.ask_about_file_type(state, id, server_name);
        }
    }

    /// A run that did not finish was stopped by `stop`: what cancelling or starting over leaves to do.
    fn discard_after_stop(&self, state: &mut State, id: DownloadId, stop: Option<&Stop>) {
        if matches!(stop, Some(Stop::Cancel | Stop::Restart)) {
            self.discard_partial(state, id);
        }
        if matches!(stop, Some(Stop::Cancel)) {
            give_up_the_name(state, id);
        }
    }

    /// Deletes a download's partial files in the background, on record until they are gone (D134). A run started before the deletion has finished waits for it, so it never writes into a file that is being deleted.
    pub(super) fn discard_partial(&self, state: &mut State, id: DownloadId) {
        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let partial = entry.partial();
        let deleted = CancellationToken::new();
        entry.cleanup = Some(deleted.clone());
        let deletion = entry.pending_deletion();
        state.pending_deletions.insert(id, deletion);
        self.save_pending_deletions(state);
        let manager = self.clone();
        #[cfg(test)]
        let before_deleting = self.before_deleting();
        self.inner.runtime.spawn_blocking(move || {
            #[cfg(test)]
            if let Some(hook) = before_deleting {
                hook();
            }
            partial.delete();
            let mut state = manager.lock();
            // A removal that began meanwhile answers for the record itself.
            if state
                .downloads
                .get(&id)
                .is_some_and(|e| !e.hold.is_removing())
            {
                state.pending_deletions.remove(&id);
                manager.save_pending_deletions(&state);
            }
            drop(state);
            deleted.cancel();
        });
    }
}

/// Keeps the progress a run's `gauge` last read, once the server had answered, and clears the
/// live fields; whether anything had arrived before is what a failure is worded by.
fn take_reading(entry: &mut Entry, gauge: &Gauge) -> Worked {
    let reading = gauge.read();
    if reading.answered {
        entry.row.done = reading.done;
        entry.row.total = reading.total;
        entry.next_run = if gauge.can_continue() {
            NextRun::Continues
        } else {
            NextRun::StartsOver
        };
    }
    let worked = worked_before(reading.answered, entry.row.done, entry.next_run);
    entry.clear_live_fields();
    worked
}

/// Records a run that finished its file, `bytes` long, at `path` when that is not the planned one.
fn complete(claimed: &mut Claimed, entry: &mut Entry, finished: Finished, now: UnixMillis) {
    let Finished {
        bytes,
        path,
        sidecars,
    } = finished;
    let id = entry.row.id;
    entry.files.sidecars = sidecars;
    // The finished file now holds the name on disk.
    claimed.remove(entry.path(), id);
    if let Some(path) = path {
        entry.row.file_name = crate::naming::file_name_of(&path);
        entry.row.path = path.to_string_lossy().into_owned();
        // Reserved by the media job while it put the file in place.
        claimed.remove(entry.path(), id);
    }
    // Already marked as downloaded from the internet: the run did that before it reported the
    // file (see `download_marked` and the media job's `finish`).
    mark_completed(entry, bytes, now);
}

/// Puts a download in the completed state, its file `bytes` long.
pub(super) fn mark_completed(entry: &mut Entry, bytes: u64, now: UnixMillis) {
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
    entry.naming.writing = false;
    entry.row.error = None;
    entry.row.done = 0;
}

/// A run that got an answer, or a download that had data from an earlier run, had a link that worked. Only data a next run can continue from is worth promising.
fn worked_before(answered: bool, done: u64, next_run: NextRun) -> Worked {
    if done > 0 && next_run != NextRun::StartsOver {
        Worked::Before
    } else if answered || done > 0 {
        Worked::BeforeStartsOver
    } else {
        Worked::Never
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
        file_work_ended(&self.manager, self.id, self.run_id, &self.file_work).await;
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
            file_work_ended(&manager, id, run_id, &file_work).await;
            if let Some(manager) = Manager::from_weak(&manager) {
                manager.finish(id, run_id, RunEnd::Vanished);
            }
            exited.cancel();
        });
    }
}

/// Waits for the file work of a run that has ended. The ticker adds to it until told not to, and work added after the wait would outlast the run.
async fn file_work_ended(manager: &Weak<Inner>, id: DownloadId, run_id: u64, file_work: &FileWork) {
    if let Some(manager) = Manager::from_weak(manager) {
        let mut state = manager.lock();
        let run = state.downloads.get_mut(&id).and_then(|e| e.run.as_mut());
        if let Some(run) = run.filter(|run| run.id == run_id) {
            run.marking_done = true;
        }
    }
    file_work.finished().await;
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
    let url = Url::parse(&entry.row.url).map_err(|_| wirecatch_messages::invalid_link())?;
    if local_network::refuses_address(entry.chosen_by, &url) {
        return Err(wirecatch_messages::private_network(entry.chosen_by));
    }
    let path = PathBuf::from(&entry.row.path);
    let dir = path.parent().map_or_default(Path::to_path_buf);
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
    request.data_complete = entry.next_run == NextRun::Finishes;
    request.options = headers::download_options(entry.chosen_by.private_network(), policy.proxy);
    let options = &mut request.options;
    options.connections = policy.connections;
    options.rate_limiters = policy.limiters;
    options.retry_backoff = policy.retry_backoff;
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
            retry_backoff: wirecatch_engine::Backoff::default(),
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
        let config = ManagerConfig::for_tests(dir.path().join("data"), folders);
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
            Some(wirecatch_messages::stopped_unexpectedly(Worked::Never))
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
