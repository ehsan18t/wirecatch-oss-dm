//! Running the add-on's programs (yt-dlp, ffmpeg): always with an argument
//! vector, never through a shell; without a console window on Windows; killed
//! when cancelled, timed out or dropped.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use wirecatch_engine::CancellationToken;

/// Largest standard output collected (yt-dlp's description of a page is a few MB).
const MAX_STDOUT: u64 = 64 * 1024 * 1024;
/// Lines of standard error kept for an error message.
const STDERR_TAIL: usize = 20;
/// Longest line passed on; longer ones are cut.
const MAX_LINE: usize = 16 * 1024;

/// How a program ended.
#[derive(Debug)]
pub(crate) struct Ran {
    pub success: bool,
    /// Standard output, when collected.
    pub stdout: Vec<u8>,
    /// The last lines it wrote to standard error.
    pub stderr: Vec<String>,
}

impl Ran {
    /// The most telling line of standard error: the last `ERROR:` line, else the
    /// last line.
    pub fn error_line(&self) -> Option<&str> {
        self.stderr
            .iter()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| self.stderr.last())
            .map(|l| l.trim())
    }
}

/// Why a program did not run to its end.
#[derive(Debug)]
pub(crate) enum RunError {
    /// It could not be started.
    Start(io::Error),
    Cancelled,
    TimedOut,
}

/// Runs `program` with `args`. With `collect_stdout`, standard output is
/// returned whole; otherwise each of its lines goes to `on_line`, as does every
/// line of standard error.
pub(crate) async fn run(
    program: &Path,
    args: &[OsString],
    collect_stdout: bool,
    timeout: Option<Duration>,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str),
) -> Result<Ran, RunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        /// Keeps a console window from flashing up for every run.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(RunError::Start)?;
    let (tx, mut rx) = mpsc::channel::<(bool, String)>(64);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let collected = tokio::spawn({
        let tx = tx.clone();
        async move {
            match stdout {
                Some(out) if collect_stdout => {
                    let mut bytes = Vec::new();
                    let _ = out.take(MAX_STDOUT).read_to_end(&mut bytes).await;
                    bytes
                }
                Some(out) => {
                    forward(out, false, &tx).await;
                    Vec::new()
                }
                None => Vec::new(),
            }
        }
    });
    if let Some(err) = stderr {
        tokio::spawn(async move { forward(err, true, &tx).await });
    } else {
        drop(tx);
    }

    let deadline = async {
        match timeout {
            Some(t) => tokio::time::sleep(t).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(deadline);
    let mut tail = Vec::new();
    let stopped = loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some((is_err, line)) => {
                    on_line(&line);
                    if is_err {
                        tail.push(line);
                        if tail.len() > STDERR_TAIL {
                            tail.remove(0);
                        }
                    }
                }
                None => break None,
            },
            _ = cancel.cancelled() => break Some(RunError::Cancelled),
            _ = &mut deadline => break Some(RunError::TimedOut),
        }
    };
    if let Some(why) = stopped {
        let _ = child.kill().await;
        return Err(why);
    }
    let status = tokio::select! {
        status = child.wait() => status,
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            return Err(RunError::Cancelled);
        }
    };
    let stdout = collected.await.unwrap_or_default();
    Ok(Ran {
        success: status.is_ok_and(|s| s.success()),
        stdout,
        stderr: tail,
    })
}

/// Sends every line of `reader` down `tx`.
async fn forward(reader: impl AsyncRead + Unpin, is_err: bool, tx: &mpsc::Sender<(bool, String)>) {
    // Bytes, not text: a program may write in the console's code page, and the
    // pipe must be drained to the end whatever it writes.
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let mut line = String::from_utf8_lossy(&buf)
            .trim_end_matches(['\r', '\n'])
            .to_string();
        if line.len() > MAX_LINE {
            let mut end = MAX_LINE;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        if tx.send((is_err, line)).await.is_err() {
            return;
        }
    }
}
