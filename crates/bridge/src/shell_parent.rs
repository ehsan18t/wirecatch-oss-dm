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
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
    PROCESS_CREATE_PROCESS, PROCESS_INFORMATION, STARTUPINFOEXW, UpdateProcThreadAttribute,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

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
    let shell = shell_process_id()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no desktop shell is running"))?;
    // SAFETY: the handle is checked and closed by the guard.
    let parent = Handle(unsafe { OpenProcess(PROCESS_CREATE_PROCESS, FALSE, shell) });
    if parent.0.is_null() {
        return Err(io::Error::last_os_error());
    }
    let attributes = AttributeList::with_parent(&parent)?;

    let application = wide(program.as_os_str());
    let mut command_line = wide(&command_line(program, args));
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
    drop(Handle(info.hThread));
    drop(Handle(info.hProcess));
    Ok(info.dwProcessId)
}

/// The size of `T` as the `u32` that Win32 structures record (`cb`, `dwSize`). A type of
/// 4 GiB or more does not compile, so the conversion below never falls back.
fn win32_size_of<T>() -> u32 {
    const { assert!(std::mem::size_of::<T>() <= u32::MAX as usize) };
    u32::try_from(std::mem::size_of::<T>()).unwrap_or(u32::MAX)
}

/// A process or thread handle, closed when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle this module opened and nothing else closes.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A one-entry attribute list naming the parent process, freed when dropped.
struct AttributeList<'a> {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    _buffer: Vec<usize>,
    /// The parent handle must stay open while the list refers to it.
    _parent: &'a Handle,
}

impl<'a> AttributeList<'a> {
    fn with_parent(parent: &'a Handle) -> io::Result<Self> {
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
            _parent: parent,
        };
        // SAFETY: the value is a pointer to the handle, which outlives the list (`_parent`).
        let updated = unsafe {
            UpdateProcThreadAttribute(
                this.list,
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                (&raw const parent.0).cast(),
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

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
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
        let line = command_line(
            Path::new(r"C:\Users\A B\AppData\Local\Wirecatch\wirecatch-app.exe"),
            &[OsString::from("--background")],
        );
        assert_eq!(
            line,
            OsString::from(
                r#""C:\Users\A B\AppData\Local\Wirecatch\wirecatch-app.exe" --background"#
            )
        );
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
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: a snapshot walked with a correctly sized entry, then closed.
        unsafe {
            let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = win32_size_of::<PROCESSENTRY32W>();
            let mut more = Process32FirstW(snapshot.0, &raw mut entry) != FALSE;
            while more {
                if entry.th32ProcessID == pid {
                    return Some(entry.th32ParentProcessID);
                }
                more = Process32NextW(snapshot.0, &raw mut entry) != FALSE;
            }
            None
        }
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
