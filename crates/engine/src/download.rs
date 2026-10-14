//! Runs one download from first request to final rename.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

use crate::abandon::{Abandoned, Abandonment};
use crate::checkpoint::{Checkpoints, ResumeSaver};
use crate::error::{Chain, Error, IoAction, IoContext, Result};
use crate::http::Clients;
use crate::net_guard::{PrivateGuard, private_guard};
use crate::numeric::saturating_cast;
use crate::options::DownloadOptions;
use crate::progress::Progress;
use crate::request::RequestContext;
use crate::segment::{self, SharedTable};
use crate::storage::Finish;
use crate::target::{self, Described, Destination, Prepared, Target};
use crate::transport::{self, Opened, Remote};
use crate::url::without_password;
use crate::{storage, worker};

/// What the server said about the file, handed to a [`PathResolver`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemoteFile {
    /// The name the server suggests, already made safe for the file system:
    /// from `Content-Disposition`, else from the URL, else `download`.
    pub suggested_name: String,
    /// Size of the file, if the server said.
    pub total: Option<u64>,
    /// The `Content-Type` the server sent, if any, for example `application/pdf`.
    pub content_type: Option<String>,
    /// The address the file actually comes from, after redirects. Never
    /// carries a password.
    pub final_url: Url,
    /// Whether the server accepts byte ranges, so the download can be paused
    /// and resumed.
    pub resumable: bool,
}

/// Picks where a download goes once the server has answered; see
/// [`DownloadRequest::resolve_path`]. Return `Err` with a reason to refuse the
/// file. It runs on the download's task, so it must return quickly and must not
/// block.
pub type PathResolver = Arc<dyn Fn(&RemoteFile) -> Result<PathBuf, PathRejected> + Send + Sync>;

/// Why a [`PathResolver`] refused a file. Its message is the caller's own,
/// written for whoever reads [`Error::TargetRejected`]; the engine only
/// carries it.
///
/// ```
/// use wirecatch_engine::PathRejected;
///
/// let refused = PathRejected::new("The server sent a web page instead of the file.");
/// assert_eq!(refused.message(), "The server sent a web page instead of the file.");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PathRejected {
    message: String,
}

impl PathRejected {
    /// A refusal for the reason `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The reason given.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// What to download and where to put it.
///
/// Create one with [`DownloadRequest::new`] and set the fields you need. New
/// fields may be added in minor releases.
#[derive(Clone)]
#[non_exhaustive]
pub struct DownloadRequest {
    /// The address to download. Its scheme picks the protocol: `http` and
    /// `https` (redirects are followed), or `ftp`, `ftps` and `ftpes` (see the
    /// [crate documentation](crate#ftp-and-ftps) for how each is encrypted).
    pub url: Url,
    /// The folder to save into; created if missing. Ignored when
    /// [`exact_path`](Self::exact_path) or [`resolve_path`](Self::resolve_path)
    /// is set.
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
    /// Decides the path from the server's answer, for callers that pick the
    /// folder or name from what the server says (its suggested name, size or
    /// type) without an extra request.
    ///
    /// Used only when [`exact_path`](Self::exact_path) is `None`. Called once,
    /// right after the server answered the first request successfully and
    /// before anything is written. The path it returns is then treated exactly
    /// like `exact_path`: an earlier partial download there is resumed, and no
    /// `(1)` suffix is added. Returning `Err` ends the download with
    /// [`Error::TargetRejected`] and nothing written. To resume later, pass the
    /// same path as `exact_path` (it is also in [`Progress::path`]).
    pub resolve_path: Option<PathResolver>,
    /// Connection count, retries, headers, speed limits and other tuning.
    pub options: DownloadOptions,
    /// What happens when a file is already at [`exact_path`](Self::exact_path),
    /// or the path [`resolve_path`](Self::resolve_path) chose, as the download
    /// finishes: [`ExistingFile::Replace`] (the default) or
    /// [`ExistingFile::Refuse`]. A download into [`dest_dir`](Self::dest_dir)
    /// never replaces anything, whatever this says.
    pub if_exists: ExistingFile,
}

/// What a download that saves to a path of the caller's does with a file that
/// is already there when it finishes. See [`DownloadRequest::if_exists`].
///
/// New variants may be added in minor releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ExistingFile {
    /// Replace it with the finished file.
    #[default]
    Replace,
    /// Keep it, and end with [`Error::TargetTaken`]. The finished data stays
    /// in the part file next to a resume file that says it is complete.
    /// Running the request again once the path is free, or after moving both
    /// files to another path and asking for that one, then only finishes it
    /// when the server gave the size and takes ranges, as for any resume;
    /// otherwise that run downloads it again.
    /// Taking the path and keeping it are one step on Windows and on Linux file
    /// systems that support it (see
    /// [`naming::move_to_free_name`](crate::naming::move_to_free_name)).
    Refuse,
}

impl std::fmt::Debug for DownloadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadRequest")
            .field("url", &self.url)
            .field("dest_dir", &self.dest_dir)
            .field("file_name", &self.file_name)
            .field("exact_path", &self.exact_path)
            .field(
                "resolve_path",
                &self.resolve_path.as_ref().map(|_| "<resolver>"),
            )
            .field("options", &self.options)
            .field("if_exists", &self.if_exists)
            .finish()
    }
}

impl DownloadRequest {
    /// A request for `url` into the folder `dest_dir`, with default options.
    pub fn new(url: Url, dest_dir: impl Into<PathBuf>) -> Self {
        Self {
            url,
            dest_dir: dest_dir.into(),
            file_name: None,
            exact_path: None,
            resolve_path: None,
            options: DownloadOptions::default(),
            if_exists: ExistingFile::Replace,
        }
    }
}

/// How a download that did not fail ended.
///
/// New variants and fields may be added in minor releases, so a `match` needs
/// a `_` arm and patterns need `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The whole file is on disk under its final name.
    #[non_exhaustive]
    Completed {
        /// Where the file was saved.
        path: PathBuf,
        /// Size of the file.
        bytes: u64,
        /// Bytes kept from an earlier run instead of downloaded again.
        resumed_from: u64,
    },
    /// Cancelled; running the same request again continues where it stopped.
    #[non_exhaustive]
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
///
/// It stays with the download and its connections: the protocol layers only
/// get [`RequestContext`] and [`Remote`], never the segment table or progress.
pub(crate) struct Shared {
    /// What every request of the download needs.
    pub request: RequestContext,
    /// The protocol and what its connections share.
    pub remote: Remote,
    pub table: SharedTable,
    pub part_path: PathBuf,
    pub progress: Arc<Progress>,
    pub opts: DownloadOptions,
    /// Connections currently streaming a response body.
    pub receiving: AtomicUsize,
}

impl Shared {
    /// Where the next request for `[from, to)` ends: cut to
    /// [`DownloadOptions::max_request_bytes`] for HTTP with byte ranges, the
    /// whole range otherwise (FTP, or a server without ranges).
    pub fn request_end(&self, from: u64, to: u64) -> u64 {
        let limit = match self.remote {
            Remote::Http { .. } if self.request.ranged => self.opts.request_limit(),
            _ => None,
        };
        segment::request_end(from, to, limit)
    }
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
    // The first download reads the system certificate store, which blocks, so
    // that one happens off the async threads.
    let clients = if Clients::shared_ready() {
        Clients::new(&request.options)?
    } else {
        let opts = request.options.clone();
        tokio::task::spawn_blocking(move || Clients::new(&opts))
            .await
            .map_err(|e| Error::task(&e))??
    };
    download_with(request, progress, cancel, clients).await
}

/// [`download`] with the given clients (tests pass ones that trust a test
/// certificate).
pub(crate) async fn download_with(
    request: DownloadRequest,
    progress: Arc<Progress>,
    cancel: CancellationToken,
    clients: Clients,
) -> Result<Outcome> {
    request.options.validate()?;
    let guard = match request.url.scheme() {
        "http" | "https" => {
            private_guard(
                &request.url,
                request.options.private_network,
                request.options.stall_timeout,
                clients.environment(),
            )
            .await
        }
        _ => None,
    };
    download_guarded(request, progress, cancel, clients, guard).await
}

/// [`download_with`] with the local network protection already decided (tests
/// pass one made for a host whose address looked public at the start).
pub(crate) async fn download_guarded(
    request: DownloadRequest,
    progress: Arc<Progress>,
    cancel: CancellationToken,
    mut clients: Clients,
    guard: Option<PrivateGuard>,
) -> Result<Outcome> {
    // Dropped with this future, which then marks queued file work abandoned.
    let abandonment = Abandonment::new();
    let abandoned = abandonment.flag();
    let mut opts = request.options.clone();
    let session_guard = guard.map(|g| {
        clients.set_resolver(g.resolver);
        // QUIC connections would not go through the filtering resolver.
        opts.http3 = false;
        g.guard
    });
    let opened = transport::open(&request.url, &opts, &clients, &cancel, session_guard).await?;
    debug!(ranged = opened.ranged, total = ?opened.total, url = %opened.final_url, "probed");
    // What the resume file records and compares: never a password.
    let saved_url = without_password(&request.url).to_string();
    let destination = Destination::choose(&request, &opened)?;

    let Opened {
        body,
        mut connection,
        remote,
        final_url,
        ranged,
        total,
        validators,
        ..
    } = opened;

    let described = Described {
        ranged,
        total,
        validators: validators.clone(),
    };
    let Prepared {
        target,
        table,
        resumed,
        resumed_from,
    } = prepare(
        destination,
        &saved_url,
        described,
        opts.min_split,
        &abandoned,
    )
    .await?;
    progress.start(&target.file, &target.name(), total, resumed_from);

    let shared = Arc::new(Shared {
        request: RequestContext {
            url: final_url,
            ranged,
            total,
            validators: validators.clone(),
            cancel,
            stall_timeout: opts.stall_timeout,
        },
        remote,
        table: SharedTable::new(table),
        part_path: target.part.clone(),
        progress,
        opts,
        receiving: AtomicUsize::new(0),
    });
    let saver = Arc::new(ResumeSaver::new(
        shared.clone(),
        saved_url,
        target.state.clone(),
        validators,
        abandoned.clone(),
    ));
    {
        let saver = saver.clone();
        storage::blocking(move || saver.save()).await?;
    }

    let first = first_work(&shared, resumed, &mut connection, body).await;
    let workers = spawn_workers(&shared, &clients, connection, first).await?;
    let checkpoints = Checkpoints::spawn(saver.clone());
    let first_error = join_workers(workers).await;
    drop(checkpoints);
    storage::blocking(move || finish(&shared, &saver, target, resumed_from, first_error)).await
}

/// Makes `destination` ready on the blocking pool, unless the download is
/// abandoned first.
async fn prepare(
    destination: Destination,
    saved_url: &str,
    described: Described,
    min_split: u64,
    abandoned: &Abandoned,
) -> Result<Prepared> {
    let (saved_url, abandoned) = (saved_url.to_string(), abandoned.clone());
    storage::blocking(move || {
        abandoned.check()?;
        destination.prepare(&saved_url, &described, min_split, &abandoned)
    })
    .await
}

/// The work the first connection starts with. The first response already
/// streams from byte 0; on a new download it goes to that connection with the
/// range from byte 0, before any other connection can split it. When resuming
/// (or with nothing to download) the connection is kept and the transfer from
/// byte 0 is ended.
async fn first_work(
    shared: &Shared,
    resumed: bool,
    connection: &mut transport::Connection,
    body: transport::Body,
) -> Option<(segment::Assignment, transport::Body)> {
    let acquired = if resumed {
        None
    } else {
        shared.table.lock().acquire(0)
    };
    if let Some(assignment) = acquired {
        Some((assignment, body))
    } else {
        connection.finish(body, &shared.request.cancel).await;
        None
    }
}

/// Starts the download's connections: one per [`DownloadOptions::min_split`]
/// still to fetch, up to [`DownloadOptions::connections`], or one for a server
/// without ranges. The first reuses the probe's connection and, when given,
/// its response. Each gets a handle of its own on the part file, all opened
/// before any starts, so they start together.
async fn spawn_workers(
    shared: &Arc<Shared>,
    clients: &Clients,
    connection: transport::Connection,
    mut first: Option<(segment::Assignment, transport::Body)>,
) -> Result<JoinSet<Result<()>>> {
    let worker_count = if shared.request.ranged {
        let remaining = shared.table.lock().remaining_bytes();
        let wanted: usize = saturating_cast(remaining.div_ceil(shared.opts.min_split));
        wanted.clamp(1, shared.opts.connections.max(1))
    } else {
        1
    };
    let mut workers = JoinSet::new();
    if shared.table.lock().is_complete() {
        return Ok(workers);
    }
    let part = shared.part_path.clone();
    let files = storage::blocking(move || {
        (0..worker_count)
            .map(|_| storage::open_for_write(&part).context(IoAction::OpenFile, &part))
            .collect::<Result<Vec<_>>>()
    })
    .await?;
    let mut first_connection = Some(connection);
    for (id, file) in files.into_iter().enumerate() {
        let connection = match first_connection.take() {
            Some(connection) => connection,
            None => shared.remote.connection(clients)?,
        };
        workers.spawn(worker::run(
            id,
            shared.clone(),
            file,
            connection,
            if id == 0 { first.take() } else { None },
        ));
    }
    Ok(workers)
}

/// Waits for every connection to end; returns the first error one gave up with.
async fn join_workers(mut workers: JoinSet<Result<()>>) -> Option<Error> {
    let mut first_error = None;
    while let Some(joined) = workers.join_next().await {
        let result = joined.unwrap_or_else(|e| Err(Error::task(&e)));
        if let Err(e) = result {
            warn!(error = %Chain(&e), "connection gave up");
            first_error.get_or_insert(e);
        }
    }
    first_error
}

/// Ends a download whose connections have all stopped: renames a complete
/// file into place, or saves the resume file and reports a pause or an error.
/// Blocks.
fn finish(
    shared: &Shared,
    saver: &ResumeSaver,
    target: Target,
    resumed_from: u64,
    first_error: Option<Error>,
) -> Result<Outcome> {
    let abandoned = saver.abandoned();
    abandoned.check()?;
    let (complete, remaining, file_len) = {
        let table = shared.table.lock();
        // A known size is the file's; a table resumed with every range already
        // on disk holds no range to take the size from.
        let file_len = shared.request.total.unwrap_or_else(|| table.file_len());
        (table.is_complete(), table.remaining_bytes(), file_len)
    };

    if complete {
        if matches!(target.finish, Finish::Refuse) {
            // A finish refused because the name is taken leaves a resume file that says the data is all there, so a later run that can resume only finishes.
            saver.save()?;
        }
        // Closing waits for a checkpoint still flushing, during which the owner may drop the download and start another run on the same files.
        saver.close();
        #[cfg(test)]
        abandoned.slow_step_done();
        abandoned.check()?;
        let path = storage::finalize(
            &target.part,
            &target.file,
            &target.finish,
            file_len,
            abandoned,
        )?;
        abandoned.check()?;
        target::discard(&target.state);
        info!(path = %path.display(), bytes = file_len, "completed");
        return Ok(Outcome::Completed {
            path,
            bytes: file_len,
            resumed_from,
        });
    }

    saver.save()?;
    saver.close();
    if shared.request.cancel.is_cancelled() {
        return Ok(Outcome::Paused {
            path: target.file,
            remaining,
        });
    }
    Err(first_error.unwrap_or(Error::PrematureEof {
        at: file_len - remaining,
        end: file_len,
    }))
}
