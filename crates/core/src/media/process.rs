//! Running the add-on's programs (yt-dlp, ffmpeg): always with an argument
//! vector, never through a shell; in a folder of Wirecatch's; without a
//! console window on Windows; killed, with everything they started, when
//! cancelled, timed out or dropped.

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

/// Runs `program` with `args` in the folder `dir`. With `collect_stdout`,
/// standard output is returned whole; otherwise each of its lines goes to
/// `on_line`, as does every line of standard error.
///
/// The program and everything it starts form one group (a process group on
/// Unix, a job object on Windows) that is killed as a whole when it is
/// cancelled, times out or is dropped, and once the program itself has ended,
/// so nothing it started keeps running. Python variables are removed from its
/// environment, so a Python setup on the machine cannot change how yt-dlp runs.
pub(crate) async fn run(
    program: &Path,
    args: &[OsString],
    dir: &Path,
    collect_stdout: bool,
    timeout: Option<Duration>,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str),
) -> Result<Ran, RunError> {
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
    #[cfg(windows)]
    {
        /// Keeps a console window from flashing up for every run.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(RunError::Start)?;
    let tree = Tree::attach(&child);
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
    let mut status = None;
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
            ended = child.wait(), if status.is_none() => {
                // The program is done; whatever it started and left behind
                // goes too, which also closes the pipes it may have shared.
                status = Some(ended);
                tree.kill();
            }
            _ = cancel.cancelled() => break Some(RunError::Cancelled),
            _ = &mut deadline => break Some(RunError::TimedOut),
        }
    };
    if let Some(why) = stopped {
        tree.kill();
        let _ = child.kill().await;
        return Err(why);
    }
    let status = match status {
        Some(status) => status,
        None => tokio::select! {
            status = child.wait() => status,
            _ = cancel.cancelled() => {
                tree.kill();
                let _ = child.kill().await;
                return Err(RunError::Cancelled);
            }
        },
    };
    tree.kill();
    let stdout = collected.await.unwrap_or_default();
    Ok(Ran {
        success: status.is_ok_and(|s| s.success()),
        stdout,
        stderr: tail,
    })
}

/// The environment variables that set up Python (`PYTHONPATH`, `PYTHONHOME`,
/// `PYTHONSTARTUP` and the like), among `names`.
fn python_variables(names: impl Iterator<Item = OsString>) -> Vec<OsString> {
    names
        .filter(|name| {
            name.to_str()
                .is_some_and(|n| n.to_ascii_uppercase().starts_with("PYTHON"))
        })
        .collect()
}

/// A program and everything it starts, killed together.
struct Tree {
    #[cfg(unix)]
    group: Option<i32>,
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl Tree {
    /// Puts the just started `child` (and so what it starts later) in a group.
    fn attach(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            // `process_group(0)` made the child the leader of a group of its own.
            Tree {
                group: child.id().and_then(|id| i32::try_from(id).ok()),
            }
        }
        #[cfg(windows)]
        {
            let job = child.raw_handle().and_then(|handle| {
                job::Job::for_process(handle)
                    .inspect_err(
                        |e| tracing::warn!(error = %e, "could not group a started program"),
                    )
                    .ok()
            });
            Tree { job }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Tree {}
        }
    }

    /// Kills every process of the group that is still running.
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(group) = self.group {
            // SAFETY: plain system call; a group that is gone gives ESRCH.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
mod job {
    use std::io;
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// A job object that kills its processes when terminated or closed.
    pub struct Job(HANDLE);

    // SAFETY: a job handle may be used from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// A new job holding the process `process`.
        pub fn for_process(process: std::os::windows::io::RawHandle) -> io::Result<Self> {
            // SAFETY: no name, default security; the handle is owned by `Job`.
            let handle = unsafe { CreateJobObjectW(null(), null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(handle);
            // SAFETY: an all-zero limit structure is valid; only the flag is set.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` lives across the call and its size is passed.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: both handles are valid for the duration of the call.
            if unsafe { AssignProcessToJobObject(job.0, process as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        /// Kills every process in the job.
        pub fn terminate(&self) {
            // SAFETY: the handle is valid until dropped.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: owned handle, closed once; closing kills what is left.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
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
}
