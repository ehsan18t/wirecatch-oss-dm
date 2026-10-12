//! Runs one download from first request to final rename.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

use crate::checkpoint::{Checkpoints, ResumeSaver};
use crate::error::{Error, Result};
use crate::fetch::{PrivateGuard, PrivateNetwork, private_guard};
use crate::http::Clients;
use crate::limiter::RateLimiter;
use crate::numeric::saturating_cast;
use crate::progress::Progress;
use crate::segment::{self, SharedTable};
use crate::target::{self, Described, Destination, Prepared, Target};
use crate::transport::{self, Opened, Remote};
use crate::units::{MIB, MIB_USIZE};
use crate::url::without_password;
use crate::{storage, worker};

/// Most connections one download may open ([`DownloadOptions::connections`]). More only adds load on the server and on this machine.
pub const MAX_CONNECTIONS: usize = 64;
/// Largest buffer one connection may fill before writing ([`DownloadOptions::buffer_size`]): 64 MiB. Each connection holds two, so this bounds memory, and a larger one would gain nothing.
pub const MAX_BUFFER_SIZE: usize = 64 * MIB_USIZE;

/// Tuning for a download. The defaults suit most connections.
///
/// Start from [`DownloadOptions::default`] and change what you need:
///
/// ```
/// # use wirecatch_engine::DownloadOptions;
/// let mut options = DownloadOptions::default();
/// options.connections = 16;
/// ```
///
/// New options may be added in minor releases, which is why the struct cannot
/// be built field by field outside this crate.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    /// How long to wait for the server, either for the answer to a request or
    /// for more data while receiving, before the attempt counts as failed and
    /// is retried (default 30 s).
    pub stall_timeout: Duration,
    /// How long opening a connection may take, TLS handshake included
    /// (default 15 s).
    pub connect_timeout: Duration,
    /// The `User-Agent` sent with every request (default `Wirecatch/<version>`).
    /// A `User-Agent` in [`headers`](Self::headers) takes precedence.
    pub user_agent: String,
    /// Extra request headers, such as the cookies, referer and user agent a
    /// browser hands over. Only used for `http` and `https` links; FTP has no
    /// headers (its login comes from the URL).
    ///
    /// They are sent in full only to the origin (scheme, host and port) of
    /// [`DownloadRequest::url`]. If that URL redirects to another origin, only
    /// headers that carry no credentials follow: `User-Agent`, `Accept`,
    /// `Accept-Language`, `Cache-Control`, `DNT`, `Pragma`, and `Referer` cut
    /// down to its origin (`https://site.example/`), as browsers do. Cookies,
    /// `Authorization` and any other header stay with their origin.
    ///
    /// A `user:password@` in the URL becomes an `Authorization: Basic` header
    /// bound the same way, unless this map already has an `Authorization`.
    ///
    /// Proxy credentials do not belong here: a `Proxy-Authorization` header is
    /// sent to the server, not to a proxy. Put them in the proxy URL instead,
    /// for example `HTTPS_PROXY=http://user:password@proxy.example:8080`.
    pub headers: HeaderMap,
    /// How often the resume file is rewritten while downloading (default 1 s).
    /// A killed process loses at most this much progress.
    pub checkpoint_interval: Duration,
    /// Speed limits this download obeys, for example a global one and its
    /// queue's. Every limiter in the list applies (default none).
    pub rate_limiters: Vec<Arc<RateLimiter>>,
    /// Follow a redirect from a secure `https` address to an insecure `http`
    /// one (default `false`: the download stops with
    /// [`Error::InsecureRedirect`] instead, because anyone on the network could
    /// read or change the file).
    pub allow_insecure_redirects: bool,
    /// Use HTTP/3 (QUIC) when the server offers it. **Experimental**, default
    /// `false`.
    ///
    /// Has an effect only when this crate is built with its `http3` feature;
    /// without it the option is accepted and ignored. When on, the first
    /// request still goes over HTTP/2 or HTTP/1.1. If the server's answer
    /// advertises HTTP/3 on the same host (an `Alt-Svc: h3=":port"` header),
    /// the other connections use HTTP/3, each over a QUIC connection of its
    /// own. Any QUIC failure (blocked UDP, a failed handshake, a broken
    /// stream) quietly switches the download back to HTTP/2 or HTTP/1.1 and
    /// it carries on from where it was. Not used when a proxy is set in the
    /// environment (`HTTPS_PROXY` or `ALL_PROXY`) and `NO_PROXY` does not name
    /// the server, since QUIC would go around the proxy.
    ///
    /// Off by default because on fast links several TCP connections are
    /// already as fast, and QUIC costs more processor time per byte.
    pub http3: bool,
    /// Whether an `http` or `https` download may reach the local network
    /// (loopback, private, link-local, shared CGNAT and the other addresses
    /// [`PrivateNetwork`] lists). Default
    /// [`PrivateNetwork::Allow`]: a link the user gave may point anywhere.
    ///
    /// With [`PrivateNetwork::Block`], or [`PrivateNetwork::Auto`] when the
    /// link's host is on the internet, the download follows the same rules as a
    /// [`Fetcher`](crate::Fetcher): every request and redirect hop to another
    /// origin is checked first, and every connection resolves names through a
    /// filter that never hands out a local address, so a name that changes its
    /// address later (DNS rebinding) cannot reach one either. The link's own host
    /// keeps local addresses only if all of its addresses were local when the
    /// download started. HTTP/3 is not used then, as QUIC connections do not go
    /// through that filter. `ftp` links are not covered.
    pub private_network: PrivateNetwork,
    /// The most bytes one HTTP request may ask for (default `None`: no limit).
    ///
    /// Some servers slow down any request for more than a certain amount; video
    /// sites do this to make players fetch a little at a time (yt-dlp reports
    /// it as a format's `http_chunk_size`). With `Some(n)`, no request over
    /// HTTP/1.1, HTTP/2 or HTTP/3 asks for more than `n` bytes: the first
    /// request asks for `bytes=0-(n-1)` and learns the size from its
    /// `Content-Range`, and each connection fetches a larger range as a series
    /// of requests of at most `n` bytes, one after the other on the same
    /// connection. Splitting, resuming, retries, speed limits and the stall
    /// timeout work exactly as without it.
    ///
    /// A server that does not accept byte ranges is downloaded in one request
    /// as before, and so is a file whose size the server does not say. FTP
    /// ignores this option. `Some(0)` is the same as `None`. Only
    /// [`download`] uses it: a [`Fetcher`](crate::Fetcher) sends the requests
    /// its caller asks for.
    pub max_request_bytes: Option<u64>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connections: 8,
            min_split: MIB,
            buffer_size: MIB_USIZE,
            max_retries: 6,
            stall_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(15),
            user_agent: concat!("Wirecatch/", env!("CARGO_PKG_VERSION")).to_string(),
            headers: HeaderMap::new(),
            checkpoint_interval: Duration::from_secs(1),
            rate_limiters: Vec::new(),
            allow_insecure_redirects: false,
            http3: false,
            private_network: PrivateNetwork::Allow,
            max_request_bytes: None,
        }
    }
}

impl DownloadOptions {
    /// Checks that every option can work. [`download`] and [`Fetcher`](crate::Fetcher) call this before sending anything, so a zero connection count, split size, buffer size, timeout or checkpoint interval is an error instead of a hang or a panic, and so are more than [`MAX_CONNECTIONS`] connections or a buffer over [`MAX_BUFFER_SIZE`], which would exhaust memory.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidOption`] naming the first option that cannot work.
    pub fn validate(&self) -> Result<()> {
        let in_range = [
            (
                "connections",
                (1..=MAX_CONNECTIONS).contains(&self.connections),
            ),
            ("min_split", self.min_split > 0),
            (
                "buffer_size",
                (1..=MAX_BUFFER_SIZE).contains(&self.buffer_size),
            ),
            ("stall_timeout", !self.stall_timeout.is_zero()),
            ("connect_timeout", !self.connect_timeout.is_zero()),
            ("checkpoint_interval", !self.checkpoint_interval.is_zero()),
        ];
        match in_range.into_iter().find(|&(_, ok)| !ok) {
            Some((option, _)) => Err(Error::InvalidOption(option)),
            None => Ok(()),
        }
    }

    /// [`max_request_bytes`](Self::max_request_bytes) as a limit, `None` when
    /// there is none.
    pub(crate) fn request_limit(&self) -> Option<u64> {
        self.max_request_bytes.filter(|&n| n > 0)
    }
}

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
pub type PathResolver = Arc<dyn Fn(&RemoteFile) -> Result<PathBuf, String> + Send + Sync>;

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
pub(crate) struct Shared {
    /// Where requests go: the URL after redirects, without credentials.
    pub url: Url,
    /// The protocol and what its connections share.
    pub remote: Remote,
    pub ranged: bool,
    pub total: Option<u64>,
    pub table: SharedTable,
    pub part_path: PathBuf,
    pub progress: Arc<Progress>,
    pub cancel: CancellationToken,
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
            Remote::Http { .. } if self.ranged => self.opts.request_limit(),
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

    let Prepared {
        target,
        table,
        resumed,
        resumed_from,
    } = {
        let described = Described {
            ranged,
            total,
            validators: validators.clone(),
        };
        let (saved_url, min_split) = (saved_url.clone(), opts.min_split);
        storage::blocking(move || destination.prepare(&saved_url, &described, min_split)).await?
    };
    let file_name = target
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    progress.start(&target.file, &file_name, total, resumed_from);

    let shared = Arc::new(Shared {
        url: final_url,
        remote,
        ranged,
        total,
        table: SharedTable::new(table),
        part_path: target.part.clone(),
        progress,
        cancel,
        opts,
        receiving: AtomicUsize::new(0),
    });
    let saver = Arc::new(ResumeSaver::new(
        shared.clone(),
        saved_url,
        target.state.clone(),
        validators,
    ));
    {
        let saver = saver.clone();
        storage::blocking(move || saver.save()).await?;
    }

    // The probe response already streams from byte 0; hand it to worker 0 along with
    // the matching range before any other worker can split it.
    let acquired = if resumed {
        None
    } else {
        shared.table.lock().acquire(0)
    };
    let first = if let Some(assignment) = acquired {
        Some((assignment, body))
    } else {
        // Resuming (or nothing to download): the connection is kept, the
        // transfer from byte 0 is not.
        connection.finish(body, &shared.cancel).await;
        None
    };

    let workers = spawn_workers(&shared, &clients, connection, first).await?;
    let checkpoints = Checkpoints::spawn(saver.clone());
    let first_error = join_workers(workers).await;
    checkpoints.stop();
    storage::blocking(move || finish(&shared, &saver, target, resumed_from, first_error)).await
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
    let worker_count = if shared.ranged {
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
            .map(|_| storage::open_for_write(&part).map_err(Error::from))
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
            warn!(error = %e, "connection gave up");
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
    let (complete, remaining, file_len) = {
        let table = shared.table.lock();
        // A known size is the file's; a table resumed with every range already
        // on disk holds no range to take the size from.
        let file_len = shared.total.unwrap_or_else(|| table.file_len());
        (table.is_complete(), table.remaining_bytes(), file_len)
    };

    if complete {
        saver.close();
        storage::finalize(&target.part, &target.file, file_len)?;
        target::discard(&target.state);
        info!(path = %target.file.display(), bytes = file_len, "completed");
        return Ok(Outcome::Completed {
            path: target.file,
            bytes: file_len,
            resumed_from,
        });
    }

    saver.save()?;
    saver.close();
    if shared.cancel.is_cancelled() {
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
