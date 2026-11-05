//! What fault tests need beyond `http::TestServer`: a raw HTTP/1.1 server whose body is generated from the offset, so a file can be larger than memory, change mid-download, and its headers can lie, and a runner for a child process that can be killed.

use crate::resume::SavedRun;
use std::fs::File as FsFile;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};

use crate::PATIENCE;

const CHUNK: usize = 64 * 1024;
const HASH_BUFFER: usize = 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);
const MICROS_PER_SECOND: u64 = 1_000_000;

/// `[start, end)` from a `Range` header; `end` is `None` for an open range.
type AskedRange = Option<(u64, Option<u64>)>;

/// For work whose failure changes nothing: a client that hung up, a clean-up of something already gone.
fn best_effort<T>(result: &std::io::Result<T>) {
    let (Ok(_) | Err(_)) = result;
}

/// The byte at each position of generated file `seed`: every 8-byte word is a hash of its index, so a byte in the wrong place or from another version shows.
pub fn fill(seed: u64, offset: u64, buf: &mut [u8]) {
    let mut position = offset;
    let mut at = 0;
    while at < buf.len() {
        let word = mix(seed ^ (position / 8)).to_le_bytes();
        let skip = usize::try_from(position % 8).unwrap_or(0);
        let take = (8 - skip).min(buf.len() - at);
        buf[at..at + take].copy_from_slice(&word[skip..skip + take]);
        at += take;
        position += take as u64;
    }
}

fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A request as the server read it.
#[derive(Clone, Debug)]
pub struct Req {
    /// Its sequence number, 0 for the first.
    pub no: usize,
    /// The path asked for.
    pub path: String,
    /// What its `Range` header asks for.
    pub range: AskedRange,
    /// Body bytes the server had sent over all requests when this one arrived.
    pub sent_before: u64,
}

/// How to answer one request.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The status line's code.
    pub status: u16,
    /// Headers, sent as given; `Connection: close` is always added.
    pub headers: Vec<(String, String)>,
    /// Which generated file the body is.
    pub seed: u64,
    /// The offset of the body's first byte.
    pub from: u64,
    /// How many body bytes go out before the connection closes.
    pub send: u64,
    /// `Transfer-Encoding: chunked`.
    pub chunked: bool,
    /// Bytes per second, if limited.
    pub pace: Option<u64>,
    /// With `chunked`, leaves out the final chunk, so the answer is visibly cut off.
    pub cut_off: bool,
}

impl Plan {
    /// Sets or replaces a header.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// Removes a header.
    #[must_use]
    pub fn without(mut self, name: &str) -> Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self
    }

    /// Limits the speed.
    #[must_use]
    pub fn paced(mut self, bps: u64) -> Self {
        self.pace = Some(bps);
        self
    }
}

/// A generated file with its validators.
#[derive(Clone, Debug)]
pub struct Generated {
    /// Picks the bytes.
    pub seed: u64,
    /// Its size.
    pub len: u64,
    /// The `ETag` to send.
    pub etag: Option<String>,
    /// The `Last-Modified` to send.
    pub last_modified: Option<String>,
}

impl Generated {
    /// A file with an `ETag` that names its seed.
    #[must_use]
    pub fn new(seed: u64, len: u64) -> Self {
        Self {
            seed,
            len,
            etag: Some(format!("\"v{seed}\"")),
            last_modified: None,
        }
    }

    /// What an earlier run of this file at `url` saved, with only the `remaining` ranges missing.
    #[must_use]
    pub fn saved_run<'a>(&'a self, url: &'a str, remaining: &'a [(u64, u64)]) -> SavedRun<'a> {
        SavedRun {
            url,
            total: Some(self.len),
            ranged: true,
            etag: self.etag.as_deref(),
            last_modified: self.last_modified.as_deref(),
            remaining,
        }
    }

    /// The same file with no validators.
    #[must_use]
    pub fn without_validators(mut self) -> Self {
        self.etag = None;
        self
    }

    fn base(&self, status: u16, from: u64, send: u64) -> Plan {
        let mut plan = Plan {
            status,
            headers: Vec::new(),
            seed: self.seed,
            from,
            send,
            chunked: false,
            pace: None,
            cut_off: false,
        };
        if let Some(etag) = &self.etag {
            plan = plan.header("ETag", etag.as_str());
        }
        if let Some(modified) = &self.last_modified {
            plan = plan.header("Last-Modified", modified.as_str());
        }
        plan
    }

    /// An honest answer from a server with ranges: `206`, or `416` past the end, or `200` to a request without a range.
    #[must_use]
    pub fn ranged(&self, req: &Req) -> Plan {
        let Some((start, end)) = req.range else {
            return self
                .base(200, 0, self.len)
                .header("Content-Length", self.len.to_string())
                .header("Accept-Ranges", "bytes");
        };
        if start >= self.len {
            return self
                .base(416, 0, 0)
                .header("Content-Range", format!("bytes */{}", self.len))
                .header("Content-Length", "0");
        }
        let end = end.unwrap_or(self.len).min(self.len);
        self.base(206, start, end - start)
            .header(
                "Content-Range",
                format!("bytes {start}-{}/{}", end - 1, self.len),
            )
            .header("Content-Length", (end - start).to_string())
            .header("Accept-Ranges", "bytes")
    }

    /// An honest answer from a server without ranges: always `200` and the whole file.
    #[must_use]
    pub fn whole(&self) -> Plan {
        self.base(200, 0, self.len)
            .header("Content-Length", self.len.to_string())
    }

    /// The whole file as a chunked body of unknown size.
    #[must_use]
    pub fn chunked(&self) -> Plan {
        let mut plan = self.base(200, 0, self.len);
        plan.chunked = true;
        plan.header("Transfer-Encoding", "chunked")
    }
}

/// A request and how many body bytes went out for it.
#[derive(Clone, Debug)]
pub struct Seen {
    /// The request.
    pub req: Req,
    /// The status it was answered with.
    pub status: u16,
}

type Handler = Arc<dyn Fn(&Req) -> Plan + Send + Sync>;

#[derive(Default)]
struct State {
    sent: u64,
    requests: usize,
    hold_at: Option<u64>,
    hold_heads: bool,
    closing: bool,
    log: Vec<Seen>,
}

struct Shared {
    handler: Mutex<Handler>,
    state: Mutex<State>,
    changed: Condvar,
}

/// A local HTTP/1.1 server, one thread per connection and one request per connection, whose answers `handler` plans.
pub struct FaultServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
}

impl FaultServer {
    /// Starts answering with `handler`.
    ///
    /// # Panics
    ///
    /// When no local port can be bound.
    pub fn start(handler: impl Fn(&Req) -> Plan + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::new(Shared {
            handler: Mutex::new(Arc::new(handler)),
            state: Mutex::default(),
            changed: Condvar::new(),
        });
        let accepting = Arc::clone(&shared);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.state.lock().unwrap().closing {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let serving = Arc::clone(&accepting);
                std::thread::spawn(move || serve(&serving, stream));
            }
        });
        Self { addr, shared }
    }

    /// Replaces how requests are answered from now on.
    pub fn set_handler(&self, handler: impl Fn(&Req) -> Plan + Send + Sync + 'static) {
        *self.shared.handler.lock().unwrap() = Arc::new(handler);
    }

    /// The URL of `path`, given without its leading slash.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.addr)
    }

    /// Body bytes sent so far, over all requests.
    #[must_use]
    pub fn sent(&self) -> u64 {
        self.shared.state.lock().unwrap().sent
    }

    /// Every request so far.
    #[must_use]
    pub fn log(&self) -> Vec<Seen> {
        self.shared.state.lock().unwrap().log.clone()
    }

    /// Stops sending bodies once `at` bytes went out in total, until [`release`](Self::release). Headers still go out.
    pub fn hold_at(&self, at: u64) {
        self.shared.state.lock().unwrap().hold_at = Some(at);
        self.shared.changed.notify_all();
    }

    /// Accepts requests without answering them, until [`release`](Self::release).
    pub fn hold_heads(&self) {
        self.shared.state.lock().unwrap().hold_heads = true;
        self.shared.changed.notify_all();
    }

    /// Ends both holds.
    pub fn release(&self) {
        let mut state = self.shared.state.lock().unwrap();
        state.hold_at = None;
        state.hold_heads = false;
        self.shared.changed.notify_all();
    }

    /// Waits until `what` holds for the server's state, under [`PATIENCE`].
    ///
    /// # Panics
    ///
    /// When it does not in time.
    pub fn wait(&self, what: &str, check: impl Fn(u64, usize) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        let mut state = self.shared.state.lock().unwrap();
        while !check(state.sent, state.requests) {
            assert!(Instant::now() < deadline, "timed out waiting for: {what}");
            state = self.shared.changed.wait_timeout(state, POLL).unwrap().0;
        }
    }
}

impl Drop for FaultServer {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closing = true;
        self.shared.changed.notify_all();
        drop(TcpStream::connect(self.addr));
    }
}

fn read_request(stream: &TcpStream) -> Option<(String, AskedRange)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let path = line.split_whitespace().nth(1)?.to_string();
    let mut range = None;
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        let text = line.trim();
        if text.is_empty() {
            return Some((path, range));
        }
        let Some((name, value)) = text.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("range") {
            let (first, last) = value.trim().strip_prefix("bytes=")?.split_once('-')?;
            let end = if last.is_empty() {
                None
            } else {
                Some(last.parse::<u64>().ok()? + 1)
            };
            range = Some((first.parse().ok()?, end));
        }
    }
}

fn serve(shared: &Shared, mut stream: TcpStream) {
    let Some((path, range)) = read_request(&stream) else {
        return;
    };
    let req = {
        let mut state = shared.state.lock().unwrap();
        let req = Req {
            no: state.requests,
            path,
            range,
            sent_before: state.sent,
        };
        state.requests += 1;
        shared.changed.notify_all();
        req
    };
    {
        let mut state = shared.state.lock().unwrap();
        while state.hold_heads && !state.closing {
            state = shared.changed.wait_timeout(state, POLL).unwrap().0;
        }
    }
    let handler = Arc::clone(&*shared.handler.lock().unwrap());
    let plan = handler(&req);
    shared.state.lock().unwrap().log.push(Seen {
        req,
        status: plan.status,
    });
    best_effort(&answer(shared, &mut stream, &plan));
}

fn answer(shared: &Shared, stream: &mut TcpStream, plan: &Plan) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} Fault\r\n", plan.status);
    for (name, value) in &plan.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;

    let piece = plan
        .pace
        .map_or(CHUNK as u64, |bps| (bps / 20).clamp(1024, CHUNK as u64));
    let mut buf = vec![0; usize::try_from(piece).unwrap_or(CHUNK)];
    let started = Instant::now();
    let mut done = 0;
    while done < plan.send {
        {
            let mut state = shared.state.lock().unwrap();
            while state.hold_at.is_some_and(|at| state.sent >= at) && !state.closing {
                state = shared.changed.wait_timeout(state, POLL).unwrap().0;
            }
            if state.closing {
                return Ok(());
            }
        }
        let take = piece.min(plan.send - done);
        let slice = &mut buf[..usize::try_from(take).unwrap_or(CHUNK)];
        fill(plan.seed, plan.from + done, slice);
        if plan.chunked {
            stream.write_all(format!("{take:x}\r\n").as_bytes())?;
            stream.write_all(slice)?;
            stream.write_all(b"\r\n")?;
        } else {
            stream.write_all(slice)?;
        }
        done += take;
        {
            let mut state = shared.state.lock().unwrap();
            state.sent += take;
            shared.changed.notify_all();
        }
        if let Some(bps) = plan.pace {
            let due = started + Duration::from_micros(done.saturating_mul(MICROS_PER_SECOND) / bps);
            let mut state = shared.state.lock().unwrap();
            while Instant::now() < due && !state.closing {
                let left = due.saturating_duration_since(Instant::now());
                state = shared.changed.wait_timeout(state, left).unwrap().0;
            }
        }
    }
    if plan.chunked && !plan.cut_off {
        stream.write_all(b"0\r\n\r\n")?;
    }
    stream.flush()
}

/// Compares the file at `path` with generated file `seed` of `len` bytes by SHA-256, and says where they differ when they do. Returns the checksum.
///
/// # Errors
///
/// The file cannot be read, or differs.
pub fn check_file(path: &Path, seed: u64, len: u64) -> Result<String, String> {
    let shown = path.display();
    let mut file = FsFile::open(path).map_err(|e| format!("{shown}: {e}"))?;
    let on_disk = file.metadata().map_err(|e| e.to_string())?.len();
    let mut got = Sha256::new();
    let mut want = Sha256::new();
    let mut read = vec![0; HASH_BUFFER];
    let mut made = vec![0; HASH_BUFFER];
    let (mut at, mut first_diff, mut differing) = (0u64, None, 0u64);
    loop {
        let n = file.read(&mut read).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        got.update(&read[..n]);
        let inside = usize::try_from((len.saturating_sub(at)).min(n as u64)).unwrap_or(n);
        fill(seed, at, &mut made[..inside]);
        want.update(&made[..inside]);
        if read[..inside] != made[..inside] {
            for i in 0..inside {
                if read[i] != made[i] {
                    first_diff.get_or_insert(at + i as u64);
                    differing += 1;
                }
            }
        }
        at += n as u64;
    }
    while at < len {
        let take = usize::try_from((len - at).min(HASH_BUFFER as u64)).unwrap_or(HASH_BUFFER);
        fill(seed, at, &mut made[..take]);
        want.update(&made[..take]);
        at += take as u64;
    }
    let (got, want) = (
        format!("{:x}", got.finalize()),
        format!("{:x}", want.finalize()),
    );
    if on_disk == len && got == want {
        return Ok(got);
    }
    Err(format!(
        "{shown}: sha256 {got}, expected {want}; {on_disk} bytes, expected {len}; first differing byte {first_diff:?}, {differing} differing bytes"
    ))
}

/// Checks that the ranges `kept` of a part file really hold generated file `seed`.
///
/// # Errors
///
/// The file cannot be read, or a kept range holds other bytes.
pub fn check_kept(part: &Path, kept: &[(u64, u64)], seed: u64) -> Result<(), String> {
    use std::io::{Seek as _, SeekFrom};
    let mut file = FsFile::open(part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut read = vec![0; HASH_BUFFER];
    let mut made = vec![0; HASH_BUFFER];
    for &(start, end) in kept {
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut at = start;
        while at < end {
            let take = usize::try_from((end - at).min(HASH_BUFFER as u64)).unwrap_or(HASH_BUFFER);
            file.read_exact(&mut read[..take])
                .map_err(|e| format!("kept range {start}..{end} at {at}: {e}"))?;
            fill(seed, at, &mut made[..take]);
            if read[..take] != made[..take] {
                let off = read[..take]
                    .iter()
                    .zip(&made[..take])
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                return Err(format!(
                    "the resume file says {start}..{end} is on disk, but byte {} differs",
                    at + off as u64
                ));
            }
            at += take as u64;
        }
    }
    Ok(())
}

/// A folder for one test, removed when dropped, with its downloads and its logs kept apart.
pub struct Scratch {
    /// The folder.
    pub root: PathBuf,
    /// Where downloads go.
    pub downloads: PathBuf,
    /// Where child processes' output and logs go.
    pub logs: PathBuf,
}

impl Scratch {
    /// A new folder under the system's temporary folder.
    ///
    /// # Panics
    ///
    /// When it cannot be made.
    #[must_use]
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("wirecatch-fault-{name}-{}-{n}", std::process::id()));
        let (downloads, logs) = (root.join("downloads"), root.join("logs"));
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        Self {
            root,
            downloads,
            logs,
        }
    }

    /// The names in the downloads folder, sorted.
    ///
    /// # Panics
    ///
    /// When the folder cannot be read.
    #[must_use]
    pub fn files(&self) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(&self.downloads)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        best_effort(&crate::remove_test_folder(&self.root));
    }
}

/// How a child process ended.
#[derive(Debug)]
pub struct Ran {
    /// Its exit status.
    pub code: Option<i32>,
    /// What it wrote to standard output.
    pub stdout: String,
    /// What it wrote to standard error.
    pub stderr: String,
    /// How long it ran.
    pub took: Duration,
}

/// A child process; killed when dropped, so none outlives a failed test.
pub struct Running {
    child: Child,
    out: PathBuf,
    err: PathBuf,
    started: Instant,
}

impl Running {
    /// Starts `command` with its output going to files in `logs`.
    ///
    /// # Panics
    ///
    /// When it cannot be started.
    #[must_use]
    pub fn start(mut command: Command, logs: &Path, keep_stdin_open: bool) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let (out, err) = (
            logs.join(format!("out{n}.txt")),
            logs.join(format!("err{n}.txt")),
        );
        command
            .stdout(Stdio::from(FsFile::create(&out).unwrap()))
            .stderr(Stdio::from(FsFile::create(&err).unwrap()))
            .stdin(if keep_stdin_open {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        Self {
            child: command.spawn().unwrap(),
            out,
            err,
            started: Instant::now(),
        }
    }

    /// Kills the process at once, as a crash or a power cut of the process would end it, and waits for it to be gone.
    ///
    /// # Panics
    ///
    /// When it cannot be killed.
    pub fn kill(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    /// [`wait`](Self::wait), for a test.
    ///
    /// # Panics
    ///
    /// It did not end in time.
    #[must_use]
    pub fn ended(self, bound: Duration) -> Ran {
        match self.wait(bound) {
            Ok(ran) => ran,
            Err(e) => panic!("the run hung: {e}"),
        }
    }

    /// Waits for the process to end by itself, under `bound`.
    ///
    /// # Errors
    ///
    /// It did not end in time; it is killed then.
    pub fn wait(mut self, bound: Duration) -> Result<Ran, String> {
        let deadline = Instant::now() + bound;
        loop {
            match self.child.try_wait().map_err(|e| e.to_string())? {
                Some(status) => {
                    return Ok(Ran {
                        code: status.code(),
                        stdout: std::fs::read_to_string(&self.out).unwrap_or_default(),
                        stderr: std::fs::read_to_string(&self.err).unwrap_or_default(),
                        took: self.started.elapsed(),
                    });
                }
                None if Instant::now() >= deadline => {
                    best_effort(&self.child.kill());
                    best_effort(&self.child.wait());
                    return Err(format!(
                        "still running after {bound:?}; stderr: {}",
                        std::fs::read_to_string(&self.err).unwrap_or_default()
                    ));
                }
                None => std::thread::park_timeout(POLL),
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        best_effort(&self.child.kill());
        best_effort(&self.child.wait());
    }
}

/// The command line downloader at `exe`, saving into `scratch`, with its log file in `scratch`.
#[must_use]
pub fn cli(exe: &str, scratch: &Scratch, url: &str) -> Running {
    let mut command = Command::new(exe);
    command
        .arg("--quiet")
        .arg("--no-proxy")
        .arg("--output-dir")
        .arg(&scratch.downloads)
        .arg(url)
        .env("WIRECATCH_LOG_DIR", &scratch.logs);
    Running::start(command, &scratch.logs, false)
}

/// Polls `check` under [`PATIENCE`] until it gives a value.
///
/// # Panics
///
/// When it does not in time.
pub fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::park_timeout(POLL);
    }
}

/// The ranges the resume file at `state` says are on disk, once it can be read and says some are.
#[must_use]
pub fn kept_now(state: &Path, len: u64) -> Option<Vec<(u64, u64)>> {
    let kept = crate::resume::kept_ranges(state, len).ok()?;
    (kept_bytes(&kept) > 0).then_some(kept)
}

/// Creates a file of `len` bytes that takes no disk space for bytes never written: on NTFS only after `fsutil` marks it sparse, otherwise `set_len` allocates them all.
///
/// # Errors
///
/// The file cannot be made.
pub fn create_sparse(path: &Path, len: u64) -> std::io::Result<()> {
    let file = FsFile::create(path)?;
    #[cfg(windows)]
    {
        let marked = Command::new("fsutil")
            .args(["sparse", "setflag"])
            .arg(path)
            .stdout(Stdio::null())
            .status()?;
        if !marked.success() {
            return Err(std::io::Error::other(format!("fsutil ended with {marked}")));
        }
    }
    file.set_len(len)
}

/// How many bytes `kept` covers.
#[must_use]
pub fn kept_bytes(kept: &[(u64, u64)]) -> u64 {
    kept.iter().map(|(start, end)| end - start).sum()
}

/// How a run that faced a fault ended, when it ended safely.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// Exit status 0, and the file is exactly the expected one.
    Exact,
    /// Another exit status, and nothing at the file's final name.
    Refused,
}

/// The rule every fault test holds a run to: it says the file is saved only when it is byte for byte the original, and otherwise leaves nothing at the final name.
///
/// # Panics
///
/// When the run broke that rule.
#[must_use]
pub fn safe_end(ran: &Ran, scratch: &Scratch, name: &str, seed: u64, len: u64) -> Ended {
    let path = scratch.downloads.join(name);
    if ran.code == Some(0) {
        if let Err(e) = check_file(&path, seed, len) {
            panic!(
                "exit status 0 with a wrong file: {e}\nstdout: {}",
                ran.stdout
            );
        }
        Ended::Exact
    } else {
        assert!(
            !path.exists(),
            "exit status {:?} but a file is at the final name; stderr: {}",
            ran.code,
            ran.stderr
        );
        Ended::Refused
    }
}

/// The requests among `seen` that asked again for bytes in `kept`, other than each run's first probe, which asks from byte 0 and is broken off.
#[must_use]
pub fn fills_overlapping(seen: &[Seen], kept: &[(u64, u64)], len: u64) -> Vec<(u64, u64)> {
    seen.iter()
        .skip(1)
        .map(|s| {
            s.req
                .range
                .map_or((0, len), |(start, end)| (start, end.unwrap_or(len)))
        })
        .filter(|&(start, end)| kept.iter().any(|&(from, to)| start < to && from < end))
        .collect()
}
