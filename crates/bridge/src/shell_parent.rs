//! Starting the app as a child of the Windows shell (Explorer) rather than of the bridge.
//!
//! The browser starts the bridge, and a process records who started it. Task Manager groups a
//! process under its parent's app, so an app started by the bridge was listed inside the
//! browser ("Microsoft Edge"), not as Wirecatch. Starting it with the shell as its parent
//! (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`) makes it exactly like an app opened from the Start
//! menu: its own entry in Task Manager, and outside the browser's job object, since the job is
//! taken from the parent too.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{
    AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, HandleOrNull, OwnedHandle,
    RawHandle,
};
use std::path::Path;

use windows_sys::Win32::Foundation::{FALSE, HANDLE};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
    PROCESS_CREATE_PROCESS, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
use wirecatch_ipc::{IpcError, ProcessAccount, wide};

/// The process id of the desktop shell (Explorer) of this session, if one runs.
pub(crate) fn shell_process_id() -> Option<u32> {
    // SAFETY: plain calls with no pointers but an out parameter on the stack.
    unsafe {
        let shell = GetShellWindow();
        if shell.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(shell, &raw mut pid);
        (pid != 0).then_some(pid)
    }
}

/// Starts `program` with `args` in `dir`, detached, as a child of the shell. Returns the new
/// process id. Fails when there is no shell or it cannot be used as the parent; the caller then
/// starts the program the usual way.
pub(crate) fn spawn(program: &Path, args: &[OsString], dir: Option<&Path>) -> io::Result<u32> {
    let parent = open_shell(shell_process_id, ProcessAccount::of)?;
    let attributes = AttributeList::with_parent(&parent)?;

    let application = wide(program.as_os_str());
    let mut command_line = wide(command_line(program, args));
    let dir = dir.map(|d| wide(d.as_os_str()));
    // SAFETY: zeroed is a valid empty value for these plain C structures.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = win32_size_of::<STARTUPINFOEXW>();
    startup.lpAttributeList = attributes.list;
    // SAFETY: as above, an all-zero `PROCESS_INFORMATION` is a valid value to be filled in.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer refers to a live, null-terminated buffer or structure owned here;
    // no handles are inherited; the environment is this process's own (null).
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            FALSE,
            EXTENDED_STARTUPINFO_PRESENT
                | DETACHED_PROCESS
                | CREATE_NEW_PROCESS_GROUP
                | CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            dir.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            &raw const startup.StartupInfo,
            &raw mut info,
        )
    };
    if created == FALSE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateProcessW succeeded, so both are open handles this process owns and closes
    // nowhere else.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread as RawHandle) });
    // SAFETY: as above.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hProcess as RawHandle) });
    Ok(info.dwProcessId)
}

/// Opens the desktop shell that `find_shell` names, to be the parent of a new process, after
/// checking that it still is the shell and that it runs as the bridge's own account, which
/// `account_of` reads from the opened process.
fn open_shell(
    mut find_shell: impl FnMut() -> Option<u32>,
    account_of: impl FnOnce(BorrowedHandle<'_>) -> Result<ProcessAccount, IpcError>,
) -> io::Result<OwnedHandle> {
    let shell = find_shell()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no desktop shell is running"))?;
    // SAFETY: OpenProcess returns a handle this process owns, or null on failure.
    let opened = unsafe {
        HandleOrNull::from_raw_handle(OpenProcess(
            PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION,
            FALSE,
            shell,
        ) as RawHandle)
    };
    let parent = OwnedHandle::try_from(opened).map_err(|_| io::Error::last_os_error())?;
    // An open handle keeps its process's id from going to another process, so if the shell
    // still has this id, the handle is the shell, not a program that got the id after the
    // shell restarted.
    if find_shell() != Some(shell) {
        return Err(io::Error::other(
            "the desktop shell restarted while Wirecatch was being started",
        ));
    }
    check_parent(
        &ProcessAccount::current().map_err(io::Error::other)?,
        &account_of(parent.as_handle()).map_err(io::Error::other)?,
    )?;
    Ok(parent)
}

/// Whether a process running as `shell` may be the parent of the app that the bridge, running
/// as `bridge`, starts. The app takes its parent's account: a shell of another user (a browser
/// started with "Run as different user") would start it as that user, listening where this
/// user's bridge never looks, and a shell with more rights would start it elevated. A shell
/// with fewer rights is what an elevated bridge needs: the app then starts unelevated.
fn check_parent(bridge: &ProcessAccount, shell: &ProcessAccount) -> io::Result<()> {
    if shell.user != bridge.user {
        return Err(io::Error::other(format!(
            "the desktop shell runs as another user account ({})",
            shell.user
        )));
    }
    if shell.integrity > bridge.integrity {
        return Err(io::Error::other(
            "the desktop shell runs with more rights than the browser",
        ));
    }
    Ok(())
}

/// The size of `T` as the `u32` that Win32 structures record (`cb`, `dwSize`). A type of
/// 4 GiB or more does not compile, so the conversion below never falls back.
fn win32_size_of<T>() -> u32 {
    const { assert!(std::mem::size_of::<T>() <= u32::MAX as usize) };
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(u32::MAX)
}

/// A one-entry attribute list naming the parent process, freed when dropped.
struct AttributeList<'a> {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    _buffer: Vec<usize>,
    /// The handle value the list points at, at an address that does not move.
    value: Box<HANDLE>,
    /// The parent handle must stay open while the list refers to it.
    _parent: &'a OwnedHandle,
}

impl<'a> AttributeList<'a> {
    fn with_parent(parent: &'a OwnedHandle) -> io::Result<Self> {
        let mut size = 0usize;
        // SAFETY: the first call only reports the size needed.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &raw mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        // Pointer-aligned storage of at least `size` bytes.
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        // SAFETY: `list` points to `size` writable bytes owned by `buffer`.
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut size) } == FALSE {
            return Err(io::Error::last_os_error());
        }
        let this = Self {
            list,
            _buffer: buffer,
            value: Box::new(parent.as_raw_handle() as HANDLE),
            _parent: parent,
        };
        // SAFETY: the value points at `this.value`, which lives as long as the list, and names
        // a handle that stays open as long (`_parent`).
        let updated = unsafe {
            UpdateProcThreadAttribute(
                this.list,
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                (&raw const *this.value).cast(),
                std::mem::size_of::<HANDLE>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if updated == FALSE {
            return Err(io::Error::last_os_error());
        }
        Ok(this)
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: the list was initialized and is deleted once, before its buffer is freed.
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

/// A command line that splits back into `program` and `args` the way programs built with the
/// Microsoft C runtime (and Rust) read it.
fn command_line(program: &Path, args: &[OsString]) -> OsString {
    let mut line = OsString::new();
    quote(&mut line, program.as_os_str());
    for arg in args {
        line.push(" ");
        quote(&mut line, arg);
    }
    line
}

fn quote(line: &mut OsString, arg: &OsStr) {
    use std::os::windows::ffi::OsStringExt;

    let text: Vec<u16> = arg.encode_wide().collect();
    let plain = !text.is_empty()
        && !text
            .iter()
            .any(|&c| c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'"'));
    if plain {
        line.push(arg);
        return;
    }
    let mut out: Vec<u16> = vec![u16::from(b'"')];
    let mut backslashes = 0usize;
    for &c in &text {
        if c == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        let repeat = if c == u16::from(b'"') {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        out.extend(std::iter::repeat_n(u16::from(b'\\'), repeat));
        out.push(c);
        backslashes = 0;
    }
    out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    out.push(u16::from(b'"'));
    line.push(OsString::from_wide(&out));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_back_into_the_same_arguments() {
        let app = format!(
            r"C:\Users\A B\AppData\Local\Wirecatch\{}.exe",
            wirecatch_product::APP_EXECUTABLE
        );
        let line = command_line(Path::new(&app), &[OsString::from("--background")]);
        assert_eq!(line, OsString::from(format!(r#""{app}" --background"#)));
        let tricky = command_line(
            Path::new(r"C:\x.exe"),
            &[
                OsString::from(r#"say "hi""#),
                OsString::from(r"ends\"),
                OsString::new(),
            ],
        );
        assert_eq!(tricky, OsString::from(r#"C:\x.exe "say \"hi\"" ends\ """#));
        let spaced = command_line(Path::new(r"C:\x.exe"), &[OsString::from(r"a b\")]);
        assert_eq!(spaced, OsString::from(r#"C:\x.exe "a b\\""#));
    }

    /// The parent process id Windows recorded for `pid`.
    fn parent_of(pid: u32) -> Option<u32> {
        use std::os::windows::io::HandleOrInvalid;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        // SAFETY: the snapshot is a handle this process owns, or invalid on failure.
        let snapshot = OwnedHandle::try_from(unsafe {
            HandleOrInvalid::from_raw_handle(
                CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) as RawHandle
            )
        })
        .ok()?;
        let raw = snapshot.as_raw_handle() as HANDLE;
        // SAFETY: the snapshot is open, and the entry is zeroed with its size set as required.
        unsafe {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = win32_size_of::<PROCESSENTRY32W>();
            let mut more = Process32FirstW(raw, &raw mut entry) != FALSE;
            while more {
                if entry.th32ProcessID == pid {
                    return Some(entry.th32ParentProcessID);
                }
                more = Process32NextW(raw, &raw mut entry) != FALSE;
            }
            None
        }
    }

    /// A finder that names each of `ids` in turn, then nothing.
    fn finder(ids: &[u32]) -> impl FnMut() -> Option<u32> + '_ {
        let mut ids = ids.iter().copied();
        move || ids.next()
    }

    // Between reading the shell's id and opening it, Explorer can restart and its id go to
    // another program, which would become the app's parent. The shell is used only if it
    // still has the id once the handle holds the process.
    #[test]
    fn a_shell_that_moved_to_another_id_is_not_used() {
        let here = std::process::id();
        let err = open_shell(finder(&[here, here + 4]), ProcessAccount::of).expect_err("refused");
        assert_eq!(err.kind(), io::ErrorKind::Other, "{err}");
        assert!(
            open_shell(finder(&[here]), ProcessAccount::of).is_err(),
            "the shell went away"
        );
        assert!(
            open_shell(finder(&[]), ProcessAccount::of).is_err(),
            "no shell at all"
        );
        open_shell(finder(&[here, here]), ProcessAccount::of).expect("still the shell");
    }

    fn account(user: &str, integrity: u32) -> ProcessAccount {
        ProcessAccount {
            user: wirecatch_ipc::Sid::parse(user).unwrap(),
            integrity,
        }
    }

    // `check_parent` alone being right did not mean `open_shell` asked it (W4I review L1):
    // the shell found here is this very process, read back as another user's.
    #[test]
    fn a_shell_read_as_another_users_is_not_opened_as_the_parent() {
        let here = std::process::id();
        let mine = ProcessAccount::current().unwrap();
        let someone_else = |_: BorrowedHandle<'_>| {
            Ok(account(
                "S-1-5-21-1111111111-2222222222-3333333333-1001",
                mine.integrity,
            ))
        };
        assert!(open_shell(finder(&[here, here]), someone_else).is_err());
        let higher = |_: BorrowedHandle<'_>| {
            Ok(ProcessAccount {
                user: mine.user.clone(),
                integrity: mine.integrity + 4096,
            })
        };
        assert!(open_shell(finder(&[here, here]), higher).is_err());
    }

    // The app takes its parent's account: a shell of another user started it as that user,
    // listening where this user's bridge never looks; a shell with more rights started it
    // elevated (IPB-8).
    #[test]
    fn a_shell_of_another_user_or_with_more_rights_is_not_used() {
        const ME: &str = "S-1-5-21-1-2-3-1001";
        const MEDIUM: u32 = 8192;
        let bridge = account(ME, MEDIUM);
        assert!(check_parent(&bridge, &account(ME, MEDIUM)).is_ok());
        assert!(
            check_parent(&account(ME, 12288), &account(ME, MEDIUM)).is_ok(),
            "an elevated bridge starts the app unelevated, as it should"
        );
        assert!(check_parent(&bridge, &account("S-1-5-21-1-2-3-1002", MEDIUM)).is_err());
        assert!(check_parent(&bridge, &account(ME, 12288)).is_err());
        assert!(check_parent(&bridge, &account("S-1-5-18", 16384)).is_err());
    }

    #[test]
    fn the_account_of_a_program_is_read_from_its_handle() {
        use std::os::windows::io::AsHandle;

        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), Into::into);
        let mut ping = std::process::Command::new(system.join("System32").join("PING.EXE"))
            .args(["-n", "2", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let account = ProcessAccount::of(ping.as_handle());
        ping.kill().ok();
        ping.wait().ok();
        assert_eq!(account.unwrap(), ProcessAccount::current().unwrap());
    }

    #[test]
    fn a_program_started_this_way_is_a_child_of_the_desktop_shell() {
        let Some(shell) = shell_process_id() else {
            eprintln!(
                "no desktop shell in this session (a service or CI runner): nothing to check"
            );
            return;
        };
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), Into::into);
        // A program that runs for about two seconds and only talks to this computer.
        let ping = system.join("System32").join("PING.EXE");
        let args = ["-n", "3", "127.0.0.1"].map(OsString::from);
        let pid = spawn(&ping, &args, Some(&system)).expect("started under the shell");
        assert_eq!(
            parent_of(pid),
            Some(shell),
            "the shell is the recorded parent"
        );
    }
}
