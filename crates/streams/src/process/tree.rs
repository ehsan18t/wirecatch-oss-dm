//! A started program and everything it starts, killed together: a process group on Unix, a job object on Windows. The system calls that needs live here and nowhere else.

use std::io;

use tokio::process::{Child, Command};

/// How often a start that failed with `ETXTBSY` is tried again, and the pause before each.
#[cfg(unix)]
const BUSY_TRIES: u32 = 20;
#[cfg(unix)]
const BUSY_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// Runs `start` again while it fails with `ETXTBSY`. A program written just before it starts (an add-on
/// unpacked, then asked its version) is "busy" while a child forked meanwhile by another thread still holds
/// the writer's handle, until that child execs; the race is the kernel's, so it is waited out, as Cargo does.
#[cfg(unix)]
fn while_text_file_busy<T>(mut start: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut tries = 1;
    loop {
        match start() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_PAUSE);
            }
            result => return result,
        }
    }
}

/// A program and everything it starts.
///
/// On Unix the group is the leader's process id. A group id is free for reuse once its last member is reaped, and the leader is reaped only by `Child::wait`, so the group is killed at most once ([`Tree::kill`] takes it) and always before the leader is reaped: [`Tree::exited`] tells that the leader exited without reaping it, and [`Tree::disarm`] is called once it has been reaped.
pub(super) struct Tree {
    #[cfg(unix)]
    group: Option<libc::pid_t>,
    #[cfg(unix)]
    exit: Option<tokio::task::JoinHandle<io::Result<()>>>,
    #[cfg(windows)]
    job: Option<job::Job>,
}

/// Starts `command` in a group of its own.
///
/// On Windows the program starts suspended, is put in its job, and only then resumed, so nothing it starts can escape the job; when it cannot be put in a job it is killed and this fails, rather than run ungrouped.
pub(super) fn spawn(mut command: Command) -> io::Result<(Child, Tree)> {
    #[cfg(unix)]
    {
        // Leader of a new process group whose id is its own.
        command.process_group(0);
        let child = while_text_file_busy(|| command.spawn())?;
        let group = child
            .id()
            .and_then(|id| libc::pid_t::try_from(id).ok())
            .ok_or_else(|| io::Error::other("the started program has no process id"))?;
        let exit = tokio::task::spawn_blocking(move || exit_without_reaping(group));
        Ok((
            child,
            Tree {
                group: Some(group),
                exit: Some(exit),
            },
        ))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        // No console window flashing up from a desktop app.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        let grouped = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the started program has no handle"))
            .and_then(job::Job::for_process)
            .and_then(|job| {
                let id = child
                    .id()
                    .ok_or_else(|| io::Error::other("the started program has no process id"))?;
                job::resume(id)?;
                Ok(job)
            });
        match grouped {
            Ok(job) => Ok((child, Tree { job: Some(job) })),
            Err(e) => {
                if let Err(kill) = child.start_kill() {
                    tracing::warn!(error = %kill, "could not stop a program that could not be grouped");
                }
                Err(e)
            }
        }
    }
}

impl Tree {
    /// Waits until the program itself has exited, without reaping it on Unix.
    pub(super) async fn exited(&mut self, child: &mut Child) {
        #[cfg(unix)]
        {
            let Some(exit) = self.exit.as_mut() else {
                return std::future::pending().await;
            };
            let result = exit.await;
            self.exit = None;
            let pid = child.id();
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, ?pid, "could not watch a program for its end");
                }
                Err(e) => {
                    tracing::warn!(error = %e, ?pid, "the task watching a program for its end failed");
                }
            }
        }
        #[cfg(windows)]
        {
            if let Err(e) = child.wait().await {
                tracing::warn!(error = %e, "could not wait for a program to end");
            }
        }
    }

    /// Kills every process of the group that is still running. Only the first call does anything.
    pub(super) fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            // SAFETY: `killpg` takes plain integers and has no memory effects. `group` is the leader's id, and the leader is not reaped yet (see `Tree`), so the id still names this group and no other: ESRCH at worst, never another group.
            if unsafe { libc::killpg(group, libc::SIGKILL) } != 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() != Some(libc::ESRCH) {
                    tracing::warn!(error = %e, "could not stop the programs a program started");
                }
            }
        }
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            job.terminate();
        }
    }

    /// Forgets the group once the leader is reaped, so nothing is ever sent to its id again.
    pub(super) fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.group = None;
        }
        #[cfg(windows)]
        {
            // The job stays valid however long it is kept: closing it kills what is left.
            self.kill();
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Blocks until the process `pid` (a child of this process) has exited, leaving it unreaped.
///
/// Each running program holds one thread of tokio's blocking pool here for as long as it runs. The app runs a handful of programs at a time, far below the pool's limit (512 threads by default); a run that queued behind a full pool would only learn of its program's end late, never miss it. A pidfd would wait without a thread but exists only on Linux 5.3 and later, and Unix as a whole needs this path anyway.
#[cfg(unix)]
fn exit_without_reaping(pid: libc::pid_t) -> io::Result<()> {
    let id = libc::id_t::try_from(pid).map_err(io::Error::other)?;
    loop {
        // SAFETY: an all-zero `siginfo_t` is a valid value of that plain C struct.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a valid, writable `siginfo_t` for the duration of the call. `WNOWAIT` leaves the child waitable, so `Child::wait` still reaps it and gets its status.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                id,
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

#[cfg(windows)]
mod job {
    use std::io;
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// The exit code the processes of a terminated job get.
    const TERMINATED: u32 = 1;

    /// A job object that kills its processes when terminated or closed.
    pub(super) struct Job(HANDLE);

    // SAFETY: the handle is a kernel object handle, which Win32 lets any thread use, and `Job` owns it alone, so moving it to another thread is moving the only owner.
    unsafe impl Send for Job {}

    impl Job {
        /// A new job holding the process `process`.
        pub(super) fn for_process(process: std::os::windows::io::RawHandle) -> io::Result<Self> {
            // SAFETY: no name and default security (both null are allowed); the handle returned is owned by `Job` from here on.
            let handle = unsafe { CreateJobObjectW(null(), null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(handle);
            // SAFETY: an all-zero limit structure is a valid value of that plain C struct; only the flag is set.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = u32::try_from(std::mem::size_of_val(&limits)).map_err(io::Error::other)?;
            // SAFETY: `job.0` is a valid job handle; `limits` lives across the call and `size` is its size.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `job.0` is a valid job handle and `process` the handle of a process the caller owns, both valid for the call.
            if unsafe { AssignProcessToJobObject(job.0, process as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        /// Kills every process in the job.
        pub(super) fn terminate(&self) {
            // SAFETY: `self.0` is a valid job handle until `Drop`.
            if unsafe { TerminateJobObject(self.0, TERMINATED) } == 0 {
                tracing::warn!(error = %io::Error::last_os_error(), "could not stop the programs a program started");
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is owned by this `Job` and closed once; closing kills what is left in the job.
            if unsafe { CloseHandle(self.0) } == 0 {
                tracing::warn!(error = %io::Error::last_os_error(), "could not close a job object");
            }
        }
    }

    /// An owned handle closed when dropped.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle was returned open by Win32 and is owned here alone; closed once.
            if unsafe { CloseHandle(self.0) } == 0 {
                tracing::warn!(error = %io::Error::last_os_error(), "could not close a handle");
            }
        }
    }

    /// Resumes the threads of the process `id`, started suspended: its primary thread, the only one a suspended process has.
    pub(super) fn resume(id: u32) -> io::Result<()> {
        // SAFETY: taking a snapshot of every thread has no preconditions; the handle is owned by `Owned`.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Owned(snapshot);
        // SAFETY: an all-zero `THREADENTRY32` is a valid value of that plain C struct; its size is set as the API requires.
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize =
            u32::try_from(std::mem::size_of::<THREADENTRY32>()).map_err(io::Error::other)?;
        let mut resumed = 0usize;
        // SAFETY: `snapshot.0` is a valid snapshot handle and `entry` a writable entry with its size set.
        let mut more = unsafe { Thread32First(snapshot.0, &raw mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == id {
                // SAFETY: opening a thread by id has no memory preconditions; a null result is checked.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = Owned(thread);
                // SAFETY: `thread.0` is a valid thread handle opened with suspend and resume access.
                if unsafe { ResumeThread(thread.0) } == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                resumed += 1;
            }
            // SAFETY: as for `Thread32First`.
            more = unsafe { Thread32Next(snapshot.0, &raw mut entry) } != 0;
        }
        if resumed == 0 {
            return Err(io::Error::other(
                "the started program has no thread to resume",
            ));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_start_refused_as_text_file_busy_is_tried_again() {
        let mut tries = 0;
        let started = while_text_file_busy(|| {
            tries += 1;
            if tries < 3 {
                Err(io::Error::from_raw_os_error(libc::ETXTBSY))
            } else {
                Ok(tries)
            }
        });
        assert_eq!(started.unwrap(), 3);
    }

    #[test]
    fn other_failures_and_a_start_that_stays_busy_are_given_up() {
        let mut tries = 0;
        let missing = while_text_file_busy::<()>(|| {
            tries += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(tries, 1);
        tries = 0;
        let busy = while_text_file_busy::<()>(|| {
            tries += 1;
            Err(io::Error::from_raw_os_error(libc::ETXTBSY))
        });
        assert_eq!(busy.unwrap_err().raw_os_error(), Some(libc::ETXTBSY));
        assert_eq!(tries, BUSY_TRIES);
    }
}
