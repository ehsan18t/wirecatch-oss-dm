//! Running a program and following its outputs.

#![forbid(unsafe_code)]

use super::{MAX_STDOUT, tree};

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_util::sync::CancellationToken;
use wirecatch_engine::units::KIB_USIZE;

/// Lines of standard error kept for an error message.
const STDERR_TAIL: usize = 20;
/// Longest line passed on; longer ones are cut at a character boundary, and the rest of the line is read past without being kept.
const MAX_LINE: usize = 16 * KIB_USIZE;
/// Lines that may wait for the caller before the readers wait in turn.
const LINE_QUEUE: usize = 64;
/// How long the outputs are still read once the program has ended and its group is killed. A program that started another outside its group (with `setsid`, say) may leave that one holding the outputs open; the run does not wait for it.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// What happens to a program's standard output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdoutMode {
    /// Collected whole (up to [`MAX_STDOUT`]) into [`Ran::stdout`].
    Collect,
    /// Passed on line by line, like standard error.
    Lines,
}

/// How a program is run. Made with [`RunOptions::lines`] or [`RunOptions::collect`] and the `with_` methods.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunOptions {
    /// What happens to standard output.
    pub stdout: StdoutMode,
    /// The longest the whole run may take.
    pub timeout: Option<Duration>,
    /// The longest the program may go without writing a line (to standard error, or to standard output in [`StdoutMode::Lines`]) before it counts as stuck, unless [`RunOptions::watch`] changed meanwhile. For runs that may take hours but report progress all along.
    pub idle_timeout: Option<Duration>,
    /// A file or folder the program writes its result into. While it changes (grows, or is written over) the program is working, even when it writes nothing to its outputs (ffmpeg moving an MP4 index to the front, yt-dlp's merge).
    pub watch: Option<PathBuf>,
}

impl RunOptions {
    /// Lines passed on, with no time limits.
    #[must_use]
    pub fn lines() -> Self {
        Self {
            stdout: StdoutMode::Lines,
            timeout: None,
            idle_timeout: None,
            watch: None,
        }
    }

    /// Standard output collected, with no time limits.
    #[must_use]
    pub fn collect() -> Self {
        Self {
            stdout: StdoutMode::Collect,
            ..Self::lines()
        }
    }

    /// The same, with the whole run limited to `timeout`.
    #[must_use]
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            ..self
        }
    }

    /// The same, stopped when the program writes no line for `quiet` and `watch` (when given) did not change.
    #[must_use]
    pub fn with_idle_timeout(self, quiet: Duration) -> Self {
        Self {
            idle_timeout: Some(quiet),
            ..self
        }
    }

    /// The same, watching `path` (a file, or a folder whose files count) for changes; see [`RunOptions::watch`].
    #[must_use]
    pub fn watching(self, path: PathBuf) -> Self {
        Self {
            watch: Some(path),
            ..self
        }
    }
}

/// How a program that ran to its end ended.
#[derive(Debug)]
#[non_exhaustive]
pub struct Ran {
    /// It exited with success.
    pub success: bool,
    /// Standard output, in [`StdoutMode::Collect`].
    pub stdout: Vec<u8>,
    /// Standard output was longer than the most collected and is cut short.
    pub stdout_truncated: bool,
    /// The last lines it wrote to standard error.
    pub stderr: Vec<String>,
}

impl Ran {
    /// The most telling line of standard error: the last one naming an error, else the last line.
    #[must_use]
    pub fn error_line(&self) -> Option<&str> {
        self.stderr
            .iter()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| self.stderr.last())
            .map(|l| l.trim())
    }
}

/// Which of a program's outputs a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Why a program did not run to its end.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunError {
    /// It could not be started, or not grouped with what it starts (and so was killed at once).
    #[error("{} could not be started", .program.display())]
    Start {
        /// The program.
        program: std::path::PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The run was cancelled.
    #[error("the program was stopped")]
    Cancelled,
    /// It ran longer than [`RunOptions::timeout`].
    #[error("the program took longer than {} seconds", .0.as_secs())]
    TimedOut(Duration),
    /// It wrote nothing, and its result did not change, for [`RunOptions::idle_timeout`].
    #[error("the program wrote nothing for {} seconds", .0.as_secs())]
    Stalled(Duration),
}

/// Runs `program` with `args` in the folder `dir`. Standard output is returned whole or passed to `on_line` line by line, as `options` says; every line of standard error goes to `on_line` too, each with the output it came from.
///
/// Cancel-safe: dropping the future kills the program and its group.
///
/// # Errors
///
/// [`RunError::Start`] when it cannot be started, and the reason it was stopped otherwise. A program that runs and fails is `Ok` with [`Ran::success`] false.
pub async fn run(
    program: &Path,
    args: &[OsString],
    dir: &Path,
    options: RunOptions,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(Output, &str),
) -> Result<Ran, RunError> {
    let start = |source| RunError::Start {
        program: program.to_path_buf(),
        source,
    };
    let (mut child, mut tree) = tree::spawn(command(program, args, dir)).map_err(start)?;
    let mut readers = read_outputs(&mut child, options.stdout);
    let deadline = sleep_for(options.timeout);
    tokio::pin!(deadline);
    let mut idle = Idle::new(&options).await;
    let mut grace: Option<Pin<Box<Sleep>>> = None;
    let mut tail = Vec::new();
    let mut leader_done = false;
    let mut reading = true;
    let stopped = loop {
        tokio::select! {
            line = readers.lines.recv(), if reading => match line {
                Some((from, line)) => {
                    idle.heard();
                    on_line(from, &line);
                    if from == Output::Stderr {
                        tail.push(line);
                        if tail.len() > STDERR_TAIL {
                            tail.remove(0);
                        }
                    }
                }
                None if leader_done => break None,
                None => reading = false,
            },
            () = tree.exited(&mut child), if !leader_done => {
                // The program is done; its group goes too, which also closes the outputs its members shared.
                leader_done = true;
                tree.kill();
                if !reading {
                    break None;
                }
                grace = Some(Box::pin(tokio::time::sleep(OUTPUT_GRACE)));
            }
            () = wait(&mut grace), if grace.is_some() => {
                tracing::warn!(program = %program.display(), "a program's outputs stayed open after it ended, held by a program it started outside its group; no longer reading them");
                readers.abort();
                break None;
            }
            () = cancel.cancelled() => break Some(RunError::Cancelled),
            () = &mut deadline => break options.timeout.map(RunError::TimedOut),
            () = idle.expired(), if idle.armed() => {
                if !idle.output_changed().await {
                    break options.idle_timeout.map(RunError::Stalled);
                }
            }
        }
    };
    if let Some(why) = stopped {
        tree.kill();
        kill(&mut child).await;
        readers.abort();
        return Err(why);
    }
    let status = child.wait().await;
    tree.disarm();
    let (stdout, stdout_truncated) = match readers.collected.await {
        Ok(collected) => collected,
        Err(e) => {
            tracing::warn!(error = %e, program = %program.display(), "a program's output could not be read to its end; what it wrote is lost");
            (Vec::new(), false)
        }
    };
    Ok(Ran {
        success: status.is_ok_and(|s| s.success()),
        stdout,
        stdout_truncated,
        stderr: tail,
    })
}

/// The idle limit of a run: a deadline moved on by every line, and at which the watched result is looked at before the run counts as stuck.
struct Idle {
    quiet: Option<Duration>,
    sleep: Option<Pin<Box<Sleep>>>,
    watch: Option<PathBuf>,
    /// The watched result as it was when last looked at.
    seen: Snapshot,
}

impl Idle {
    async fn new(options: &RunOptions) -> Self {
        let seen = match &options.watch {
            Some(path) => Snapshot::of(path).await,
            None => Snapshot::default(),
        };
        Self {
            quiet: options.idle_timeout,
            sleep: options
                .idle_timeout
                .map(|quiet| Box::pin(tokio::time::sleep(quiet))),
            watch: options.watch.clone(),
            seen,
        }
    }

    fn armed(&self) -> bool {
        self.sleep.is_some()
    }

    /// The program wrote a line: the deadline moves on.
    fn heard(&mut self) {
        if let (Some(sleep), Some(quiet)) = (self.sleep.as_mut(), self.quiet) {
            sleep.as_mut().reset(Instant::now() + quiet);
        }
    }

    async fn expired(&mut self) {
        wait(&mut self.sleep).await;
    }

    /// Whether the watched result changed since it was last looked at; then the deadline moves on.
    async fn output_changed(&mut self) -> bool {
        let Some(path) = &self.watch else {
            return false;
        };
        let now = Snapshot::of(path).await;
        if now == self.seen {
            return false;
        }
        self.seen = now;
        self.heard();
        true
    }
}

/// What the watched result looks like: the size of the file, or of the files straight inside the folder, and the latest time one of them was written. Any change is work: ffmpeg's `+faststart` pass moves the data within the finished file for a long time without making it larger, and only the time it was written shows it (measured with ffmpeg 9.0.2 on Windows; Linux updates it on every write too).
#[derive(Debug, Default, PartialEq, Eq)]
struct Snapshot {
    size: u64,
    written: Option<SystemTime>,
}

impl Snapshot {
    /// `path` as it is now; empty for what cannot be read.
    async fn of(path: &Path) -> Self {
        let Ok(meta) = tokio::fs::metadata(path).await else {
            return Self::default();
        };
        let mut snapshot = Self::default();
        if meta.is_file() {
            snapshot.add(&meta);
            return snapshot;
        }
        let Ok(mut entries) = tokio::fs::read_dir(path).await else {
            return snapshot;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            // Through the path, not the listing: on Windows a listing shows a file as it was when last closed, so a file being written looks unchanged until it is done (measured with ffmpeg 9.0.2).
            if let Ok(meta) = tokio::fs::metadata(entry.path()).await
                && meta.is_file()
            {
                snapshot.add(&meta);
            }
        }
        snapshot
    }

    fn add(&mut self, meta: &std::fs::Metadata) {
        self.size = self.size.saturating_add(meta.len());
        self.written = self.written.max(meta.modified().ok());
    }
}

/// Sleeps for `duration`, or forever.
async fn sleep_for(duration: Option<Duration>) {
    match duration {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}

/// Waits for a timer, if there is one.
async fn wait(timer: &mut Option<Pin<Box<Sleep>>>) {
    match timer {
        Some(sleep) => sleep.as_mut().await,
        None => std::future::pending().await,
    }
}

/// Kills `child` itself (its group is the tree's) and reaps it; a failure is logged.
async fn kill(child: &mut Child) {
    if let Err(e) = child.kill().await {
        tracing::warn!(error = %e, "could not stop a program");
    }
}

/// The command that runs `program` with `args` in `dir`, its outputs piped, no input, without Python's environment variables.
fn command(program: &Path, args: &[OsString], dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in python_variables(std::env::vars_os().map(|(name, _)| name)) {
        command.env_remove(name);
    }
    command
}

/// The tasks reading a program's outputs.
struct Readers {
    /// Standard output collected whole, and whether it was cut short (in [`StdoutMode::Collect`]).
    collected: JoinHandle<(Vec<u8>, bool)>,
    /// Standard error's reader.
    stderr: Option<JoinHandle<()>>,
    /// The lines read; closed once both outputs have ended.
    lines: mpsc::Receiver<(Output, String)>,
}

impl Readers {
    /// Stops reading, for outputs held open by something outside the program's group.
    fn abort(&self) {
        self.collected.abort();
        if let Some(stderr) = &self.stderr {
            stderr.abort();
        }
    }
}

/// Reads the outputs of `child` on tasks of their own: in [`StdoutMode::Collect`] standard output is collected whole (up to [`MAX_STDOUT`]); otherwise its lines, and always those of standard error, arrive on the channel.
fn read_outputs(child: &mut Child, mode: StdoutMode) -> Readers {
    let (tx, lines) = mpsc::channel::<(Output, String)>(LINE_QUEUE);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let collected = tokio::spawn({
        let tx = tx.clone();
        async move {
            match stdout {
                Some(out) if mode == StdoutMode::Collect => collect(out, MAX_STDOUT).await,
                Some(out) => {
                    forward(BufReader::new(out), Output::Stdout, &tx).await;
                    (Vec::new(), false)
                }
                None => (Vec::new(), false),
            }
        }
    });
    let stderr = stderr.map(|err| {
        tokio::spawn(async move { forward(BufReader::new(err), Output::Stderr, &tx).await })
    });
    Readers {
        collected,
        stderr,
        lines,
    }
}

/// Reads `out` to its end, keeping at most `limit` bytes and telling whether there was more. Everything is read, so the program never blocks on a full pipe.
async fn collect(out: impl AsyncRead + Unpin, limit: u64) -> (Vec<u8>, bool) {
    let mut out = out;
    let mut bytes = Vec::new();
    // What arrived before a failed read is kept; the failure is logged, so a cut-off answer is not taken for a malformed one.
    if let Err(e) = (&mut out).take(limit).read_to_end(&mut bytes).await {
        tracing::warn!(error = %e, "could not read a program's output");
        return (bytes, false);
    }
    match tokio::io::copy(&mut out, &mut tokio::io::sink()).await {
        Ok(0) => (bytes, false),
        Ok(_) => (bytes, true),
        Err(e) => {
            tracing::warn!(error = %e, "could not read past the end of a program's output that was kept");
            (bytes, true)
        }
    }
}

/// The environment variables that set up Python (`PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP` and the like), among `names`.
fn python_variables(names: impl Iterator<Item = OsString>) -> Vec<OsString> {
    names
        .filter(|name| {
            name.to_str()
                .is_some_and(|n| n.to_ascii_uppercase().starts_with("PYTHON"))
        })
        .collect()
}

/// Sends every line of `reader` down `tx`, each cut to [`MAX_LINE`]; at most [`MAX_LINE`] bytes of a line are held, however long it is.
async fn forward(
    mut reader: impl AsyncBufRead + Unpin,
    from: Output,
    tx: &mpsc::Sender<(Output, String)>,
) {
    // Bytes, not text: a program may write in the console's code page, and the pipe must be drained to the end whatever it writes.
    let mut line = Vec::with_capacity(MAX_LINE);
    loop {
        line.clear();
        match read_line(&mut reader, &mut line).await {
            Ok(false) => return,
            Ok(true) => {}
            Err(e) => {
                tracing::debug!(error = %e, ?from, "a program's output could not be read further");
                return;
            }
        }
        let text = cut(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']));
        if tx.send((from, text)).await.is_err() {
            return;
        }
    }
}

/// Reads one line into `line`, keeping at most [`MAX_LINE`] bytes of it and reading past the rest. `false` at the end of the output.
async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    line: &mut Vec<u8>,
) -> io::Result<bool> {
    let mut read_any = false;
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return Ok(read_any);
        }
        read_any = true;
        let (taken, ends) = match buf.iter().position(|b| *b == b'\n') {
            Some(at) => (at + 1, true),
            None => (buf.len(), false),
        };
        let room = MAX_LINE.saturating_sub(line.len());
        line.extend_from_slice(&buf[..taken.min(room)]);
        reader.consume(taken);
        if ends {
            return Ok(true);
        }
    }
}

/// `line`, cut to at most [`MAX_LINE`] bytes at a character boundary.
fn cut(line: &str) -> String {
    let mut end = line.len().min(MAX_LINE);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_settings_are_left_out() {
        let names = [
            "PATH",
            "PYTHONPATH",
            "pythonhome",
            "PythonStartup",
            "MYPYTHON",
            "HOME",
        ]
        .into_iter()
        .map(OsString::from);
        assert_eq!(
            python_variables(names),
            ["PYTHONPATH", "pythonhome", "PythonStartup"].map(OsString::from)
        );
    }

    #[test]
    fn long_lines_are_cut_at_a_character_boundary() {
        // A three-byte character straddles the limit.
        let line = format!("{}€tail", "a".repeat(MAX_LINE - 1));
        let cut = cut(&line);
        assert_eq!(cut.len(), MAX_LINE - 1);
        assert!(cut.bytes().all(|b| b == b'a'));
        assert_eq!(super::cut("short"), "short");
    }

    #[tokio::test]
    async fn a_line_without_an_end_holds_at_most_the_line_limit() {
        let huge = vec![b'a'; MAX_LINE * 4];
        let mut reader = &huge[..];
        let mut line = Vec::new();
        assert!(read_line(&mut reader, &mut line).await.unwrap());
        assert_eq!(line.len(), MAX_LINE);
        assert!(line.capacity() < MAX_LINE * 2);
        assert!(!read_line(&mut reader, &mut line).await.unwrap());
    }

    #[tokio::test]
    async fn output_past_the_limit_is_reported_and_read_to_its_end() {
        let (bytes, truncated) = collect(&b"0123456789"[..], 4).await;
        assert_eq!((bytes.as_slice(), truncated), (&b"0123"[..], true));
        let (bytes, truncated) = collect(&b"0123"[..], 4).await;
        assert_eq!((bytes.as_slice(), truncated), (&b"0123"[..], false));
    }
}
