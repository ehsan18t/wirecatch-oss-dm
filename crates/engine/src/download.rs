//! Runs one download from first request to final rename.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

use crate::error::{Error, Result};
use crate::http::{self, Clients, Probe, RequestHeaders};
use crate::limiter::RateLimiter;
use crate::progress::Progress;
use crate::segment::SegmentTable;
use crate::state::ResumeState;
use crate::{naming, storage, worker};

const MIB: u64 = 1024 * 1024;

/// Tuning for a download. The defaults suit most connections.
///
/// Start from [`DownloadOptions::default`] and change what you need:
///
/// ```
/// # use wirecatch_engine::DownloadOptions;
/// let options = DownloadOptions {
///     connections: 16,
///     ..DownloadOptions::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Most parallel connections to open (default 8). Fewer are used when the
    /// file is too small to split that often, when the server does not accept
    /// byte ranges, or when the server refuses extra connections.
    pub connections: usize,
    /// A range is only split if both halves would be at least this big, in
    /// bytes (default 1 MiB).
    pub min_split: u64,
    /// Bytes collected per connection before each disk write (default 1 MiB).
    pub buffer_size: usize,
    /// Consecutive failed attempts without progress before a connection gives
    /// up (default 6).
    pub max_retries: u32,
    /// A connection that receives nothing for this long is retried (default 30 s).
    pub stall_timeout: Duration,
    /// How long opening a connection may take, TLS handshake included
    /// (default 15 s).
    pub connect_timeout: Duration,
    /// The `User-Agent` sent with every request (default `Wirecatch/<version>`).
    /// A `User-Agent` in [`headers`](Self::headers) takes precedence.
    pub user_agent: String,
    /// Extra request headers, such as the cookies, referer and user agent a
    /// browser hands over.
    ///
    /// They are sent in full only to the origin (scheme, host and port) of
    /// [`DownloadRequest::url`]. If that URL redirects to another origin, only
    /// headers that carry no credentials follow: `Referer`, `User-Agent`,
    /// `Accept`, `Accept-Language`, `Cache-Control`, `DNT` and `Pragma`.
    /// Cookies, `Authorization` and any other header stay with their origin.
    pub headers: HeaderMap,
    /// How often the resume file is rewritten while downloading (default 1 s).
    /// A killed process loses at most this much progress.
    pub checkpoint_interval: Duration,
    /// Speed limits this download obeys, for example a global one and its
    /// queue's. Every limiter in the list applies (default none).
    pub rate_limiters: Vec<Arc<RateLimiter>>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connections: 8,
            min_split: MIB,
            buffer_size: MIB as usize,
            max_retries: 6,
            stall_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(15),
            user_agent: concat!("Wirecatch/", env!("CARGO_PKG_VERSION")).to_string(),
            headers: HeaderMap::new(),
            checkpoint_interval: Duration::from_secs(1),
            rate_limiters: Vec::new(),
        }
    }
}

/// What to download and where to put it.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// The address to download. Redirects are followed.
    pub url: Url,
    /// The folder to save into; created if missing. Ignored when
    /// [`exact_path`](Self::exact_path) is set.
    pub dest_dir: PathBuf,
    /// Overrides the name the server suggests. It is still made safe for the
    /// file system, and a free name is picked if a file with it already exists.
    pub file_name: Option<String>,
    /// Save to exactly this path, ignoring `dest_dir` and `file_name`. The caller
    /// owns naming (no `(1)` suffix is added, and an existing file is replaced on
    /// completion). A partial download already at this path is continued even if
    /// the URL differs, for example after a link refresh, as long as the server
    /// still reports the same size and validators.
    pub exact_path: Option<PathBuf>,
    /// Connection count, retries, headers, speed limits and other tuning.
    pub options: DownloadOptions,
}

impl DownloadRequest {
    /// A request for `url` into the folder `dest_dir`, with default options.
    pub fn new(url: Url, dest_dir: impl Into<PathBuf>) -> Self {
        Self {
            url,
            dest_dir: dest_dir.into(),
            file_name: None,
            exact_path: None,
            options: DownloadOptions::default(),
        }
    }
}

/// How a download that did not fail ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The whole file is on disk under its final name.
    Completed {
        /// Where the file was saved.
        path: PathBuf,
        /// Size of the file.
        bytes: u64,
        /// Bytes kept from an earlier run instead of downloaded again.
        resumed_from: u64,
    },
    /// Cancelled; running the same request again continues where it stopped.
    Paused {
        /// Where the file will be saved once complete. The data so far is in
        /// this path plus [`PART_EXT`](crate::PART_EXT), next to the resume
        /// file (plus [`STATE_EXT`](crate::STATE_EXT)).
        path: PathBuf,
        /// Bytes still to download.
        remaining: u64,
    },
}

/// State every worker of one download shares.
pub(crate) struct Shared {
    pub url: Url,
    /// The caller's headers, limited per request to what `url`'s origin may see.
    pub headers: RequestHeaders,
    pub ranged: bool,
    pub total: Option<u64>,
    pub table: Mutex<SegmentTable>,
    pub part_path: PathBuf,
    pub progress: Arc<Progress>,
    pub cancel: CancellationToken,
    pub opts: DownloadOptions,
    /// Connections currently streaming a response body.
    pub receiving: AtomicUsize,
}

struct Target {
    final_path: PathBuf,
    part_path: PathBuf,
    state_path: PathBuf,
    resume: Option<ResumeState>,
}

/// Downloads `request.url` into `request.dest_dir`.
///
/// `progress` is updated live and can be read from any task or thread.
/// Cancelling `cancel` stops all connections, writes what has arrived, saves the
/// resume file and returns [`Outcome::Paused`]. Running the same request again
/// later resumes it. See the [crate documentation](crate) for an example.
///
/// # Errors
///
/// Returns an [`Error`] when the download cannot finish: the server refuses it,
/// the network stays down beyond the retries, the disk fails, or the file
/// changes on the server in a way that cannot be reconciled. Whatever was
/// downloaded before the error is kept for a later run whenever possible.
pub async fn download(
    request: DownloadRequest,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> Result<Outcome> {
    let opts = request.options.clone();
    // Reading the system certificate store blocks, so keep it off the async threads.
    let clients = {
        let opts = opts.clone();
        tokio::task::spawn_blocking(move || Clients::new(&opts))
            .await
            .map_err(std::io::Error::other)?
    };
    let headers = RequestHeaders::new(&request.url, &opts.headers);
    let first_client = clients.build()?;
    let probe = probe_with_retries(&first_client, &request.url, &headers, &opts, &cancel).await?;
    debug!(ranged = probe.ranged, total = ?probe.total, url = %probe.final_url, "probed");

    let target = match &request.exact_path {
        Some(path) => plan_exact(path, &probe)?,
        None => {
            let name = naming::sanitize(
                &request
                    .file_name
                    .clone()
                    .or_else(|| probe.file_name.clone())
                    .or_else(|| naming::from_url(&probe.final_url))
                    .or_else(|| naming::from_url(&request.url))
                    .unwrap_or_default(),
            );
            std::fs::create_dir_all(&request.dest_dir)?;
            plan_target(&request.dest_dir, &name, request.url.as_str(), &probe)
        }
    };

    let Probe {
        response,
        final_url,
        ranged,
        total,
        validators,
        ..
    } = probe;

    let (table, resumed_from) = match &target.resume {
        Some(saved) => {
            let table = SegmentTable::from_remaining(&saved.remaining, opts.min_split);
            let kept = total.unwrap_or(0) - table.remaining_bytes();
            info!(kept, "resuming");
            (table, kept)
        }
        None => {
            storage::create_part(&target.part_path, total)?;
            let table = SegmentTable::new(total, opts.min_split);
            (
                if ranged {
                    table
                } else {
                    table.without_splitting()
                },
                0,
            )
        }
    };
    let file_name = target
        .final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    progress.start(&target.final_path, &file_name, total, resumed_from);

    let shared = Arc::new(Shared {
        url: final_url,
        headers,
        ranged,
        total,
        table: Mutex::new(table),
        part_path: target.part_path.clone(),
        progress,
        cancel,
        opts,
        receiving: AtomicUsize::new(0),
    });
    let save_state = {
        let shared = shared.clone();
        let (url, path) = (request.url.to_string(), target.state_path.clone());
        move || {
            let remaining = shared.table.lock().unwrap().remaining();
            ResumeState::new(&url, total, ranged, &validators, remaining).save(&path)
        }
    };
    save_state()?;

    // The probe response already streams from byte 0; hand it to worker 0 along with
    // the matching range before any other worker can split it.
    let mut first = if target.resume.is_none() {
        shared
            .table
            .lock()
            .unwrap()
            .acquire(0)
            .map(|a| (a, response))
    } else {
        drop(response);
        None
    };

    let worker_count = if ranged {
        let remaining = shared.table.lock().unwrap().remaining_bytes();
        remaining
            .div_ceil(shared.opts.min_split)
            .clamp(1, shared.opts.connections.max(1) as u64) as usize
    } else {
        1
    };

    let mut workers = JoinSet::new();
    if !shared.table.lock().unwrap().is_complete() {
        for id in 0..worker_count {
            let client = if id == 0 {
                first_client.clone()
            } else {
                clients.build()?
            };
            workers.spawn(worker::run(
                id,
                shared.clone(),
                client,
                if id == 0 { first.take() } else { None },
            ));
        }
    }

    let checkpoints = {
        let save_state = save_state.clone();
        let every = shared.opts.checkpoint_interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            tick.tick().await;
            loop {
                tick.tick().await;
                let save = save_state.clone();
                if let Ok(Err(e)) = tokio::task::spawn_blocking(save).await {
                    warn!(error = %e, "could not save resume state");
                }
            }
        })
    };

    let mut first_error = None;
    while let Some(joined) = workers.join_next().await {
        let result = joined.unwrap_or_else(|e| Err(std::io::Error::other(e).into()));
        if let Err(e) = result {
            warn!(error = %e, "connection gave up");
            first_error.get_or_insert(e);
        }
    }
    checkpoints.abort();

    let (complete, remaining, file_len) = {
        let table = shared.table.lock().unwrap();
        (
            table.is_complete(),
            table.remaining_bytes(),
            table.file_len(),
        )
    };

    if complete {
        storage::finalize(&target.part_path, &target.final_path, file_len)?;
        let _ = std::fs::remove_file(&target.state_path);
        info!(path = %target.final_path.display(), bytes = file_len, "completed");
        return Ok(Outcome::Completed {
            path: target.final_path,
            bytes: file_len,
            resumed_from,
        });
    }

    save_state()?;
    if shared.cancel.is_cancelled() {
        return Ok(Outcome::Paused {
            path: target.final_path,
            remaining,
        });
    }
    Err(first_error.unwrap_or(Error::PrematureEof {
        at: file_len - remaining,
        end: file_len,
    }))
}

/// The first request gets the same retry treatment as every later one: a server
/// can drop a connection before sending any headers.
///
/// A 429 or 503 with `Retry-After` waits as long as the server asked (up to
/// five minutes) instead of the usual backoff.
async fn probe_with_retries(
    client: &reqwest::Client,
    url: &Url,
    headers: &RequestHeaders,
    opts: &DownloadOptions,
    cancel: &CancellationToken,
) -> Result<Probe> {
    let mut failures = 0;
    loop {
        match http::probe(client, url, headers).await {
            Err(f) if f.error.is_retryable() && failures < opts.max_retries => {
                failures += 1;
                let delay = worker::retry_delay(failures, f.retry_after);
                warn!(error = %f.error, retry_in = ?delay, "first request failed, retrying");
                tokio::select! {
                    _ = cancel.cancelled() => return Err(Error::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            result => return result.map_err(|f| f.error),
        }
    }
}

/// Target for [`DownloadRequest::exact_path`]: the URL is not compared, only what
/// the server says about the file.
fn plan_exact(path: &Path, probe: &Probe) -> Result<Target> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let (part_path, state_path) = naming::part_paths(path);
    let part_len = std::fs::metadata(&part_path).map(|m| m.len()).ok();
    let resume = ResumeState::load(&state_path)
        .ok()
        .filter(|saved| saved.can_resume(probe.ranged, probe.total, &probe.validators))
        .filter(|_| part_len == probe.total);
    if resume.is_none() {
        let _ = std::fs::remove_file(&part_path);
        let _ = std::fs::remove_file(&state_path);
    }
    Ok(Target {
        final_path: path.to_path_buf(),
        part_path,
        state_path,
        resume,
    })
}

/// Decides where the file goes and whether an earlier run can be continued.
fn plan_target(dir: &Path, name: &str, url: &str, probe: &Probe) -> Target {
    let preferred = dir.join(name);
    let (part_path, state_path) = naming::part_paths(&preferred);

    if let Ok(saved) = ResumeState::load(&state_path)
        && saved.url == url
        && !preferred.exists()
    {
        let part_len = std::fs::metadata(&part_path).map(|m| m.len()).ok();
        if saved.can_resume(probe.ranged, probe.total, &probe.validators) && part_len == probe.total
        {
            return Target {
                final_path: preferred,
                part_path,
                state_path,
                resume: Some(saved),
            };
        }
        // Same download, but it cannot continue (file changed, or no range support):
        // start over under the same name.
        info!("previous partial download cannot be resumed; starting over");
        let _ = std::fs::remove_file(&part_path);
        let _ = std::fs::remove_file(&state_path);
        return Target {
            final_path: preferred,
            part_path,
            state_path,
            resume: None,
        };
    }

    let final_path = naming::unique_path(dir, name);
    let (part_path, state_path) = naming::part_paths(&final_path);
    Target {
        final_path,
        part_path,
        state_path,
        resume: None,
    }
}
