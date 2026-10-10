//! Running the engine on one download and recording how the run ended.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

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

impl Manager {
    /// Starts the engine on `id`. The caller has already checked every limit.
    pub(super) fn start_run(&self, state: &mut State, id: DownloadId) {
        let run_id = state.ids.run();
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
        let request = engine_request(entry, connections, limiters);

        let Some(entry) = state.downloads.get_mut(&id) else {
            return;
        };
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(error);
                self.save(state, id);
                return;
            }
        };
        let run = Run {
            id: run_id,
            cancel: CancellationToken::new(),
            exited: CancellationToken::new(),
            progress: Arc::new(Progress::default()),
            stop: None,
            meter: SpeedMeter::default(),
        };
        tokio::spawn(drive(
            Arc::downgrade(&self.inner),
            id,
            run_id,
            request,
            run.progress.clone(),
            run.cancel.clone(),
            run.exited.clone(),
        ));
        entry.run = Some(run);
        entry.resume_first = false;
        entry.row.status = DownloadStatus::Connecting;
        entry.row.error = None;
        debug!(id = id.0, "download started");
        self.save(state, id);
    }

    /// Records how a run ended and gives its slot to the next download.
    fn finish(&self, id: DownloadId, run_id: u64, result: Result<Outcome, EngineError>) {
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
        match (result, run.stop) {
            (Ok(Outcome::Completed { bytes, .. }), _) => {
                entry.row.status = DownloadStatus::Completed;
                entry.row.total = Some(bytes);
                entry.row.done = bytes;
                entry.row.completed_at = Some(now);
                entry.row.error = None;
            }
            (_, Some(Stop::Pause | Stop::Remove)) | (Ok(Outcome::Paused { .. }), None) => {
                entry.row.status = DownloadStatus::Paused;
            }
            (_, Some(Stop::Requeue)) => entry.row.status = DownloadStatus::Queued,
            // The status stays as it was, so the next start continues it.
            (_, Some(Stop::Shutdown)) => {}
            (_, Some(Stop::Cancel)) => {
                mark_cancelled(entry);
                discard_partial = true;
            }
            (Err(error), None) => {
                warn!(id = id.0, %error, "download failed");
                let folder = entry.path().parent().unwrap_or(Path::new("")).to_path_buf();
                entry.row.status = DownloadStatus::Failed;
                entry.row.error = Some(user_error::describe(&error, &folder, worked_before));
            }
        }
        let partial = discard_partial.then(|| partial_files(entry.path()));
        self.save(&mut state, id);
        self.reconcile(&mut state);
        drop(state);
        if let Some(files) = partial {
            tokio::task::spawn_blocking(move || delete_files(&files));
        }
    }
}

/// Puts a download in the cancelled state: failed, with nothing kept.
pub(super) fn mark_cancelled(entry: &mut Entry) {
    entry.row.status = DownloadStatus::Failed;
    entry.row.error = Some(user_error::cancelled());
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

async fn drive(
    weak: Weak<Inner>,
    id: DownloadId,
    run_id: u64,
    request: DownloadRequest,
    progress: Arc<Progress>,
    cancel: CancellationToken,
    exited: CancellationToken,
) {
    let result = wirecatch_engine::download(request, progress, cancel).await;
    if let Some(manager) = Manager::from_weak(&weak) {
        manager.finish(id, run_id, result);
    }
    exited.cancel();
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
    use super::*;
    use wirecatch_protocol::extension::Header;

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
