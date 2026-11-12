//! File creation, space reservation and positional writes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::abandon::Abandoned;
use crate::error::{IoAction, IoContext};
use crate::file_work::FileWork;

/// Suffix of the file that receives data while a download runs.
pub const PART_EXT: &str = ".wcpart";
/// Suffix of the resume file next to it.
pub const STATE_EXT: &str = ".wcstate";

/// Runs file system work on the blocking pool, off the async threads, where a
/// slow disk, an antivirus scan or a network drive stalls nothing else.
pub(crate) async fn blocking<T: Send + 'static>(
    file_work: &FileWork,
    work: impl FnOnce() -> crate::Result<T> + Send + 'static,
) -> crate::Result<T> {
    file_work
        .spawn_blocking(work)
        .await
        .map_err(|e| crate::Error::task(&e))?
}

/// Creates (or truncates) the part file, hidden, and reserves `total` bytes if known.
pub fn create_part(path: &Path, total: Option<u64>) -> io::Result<()> {
    let file =
        created_hidden(OpenOptions::new().create(true).write(true).truncate(true)).open(path)?;
    size_part(path, &file, total)
}

/// A file this opens is created hidden, so it is never on show for a moment. Windows also refuses a truncating open of a hidden file that does not ask for the attribute.
pub fn created_hidden(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::attributes(
        options,
        windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN,
    );
    options
}

/// Hides the file or folder at `path` from a file manager, or shows it again: the hidden attribute on Windows. Elsewhere nothing. A failure is logged: it costs only the look of the folder, and a network share may refuse the attribute.
pub fn set_hidden(path: &Path, hidden: bool) {
    if let Err(e) = try_set_hidden(path, hidden) {
        let error = crate::error::Chain(&e);
        tracing::debug!(path = %path.display(), hidden, %error, "could not change whether a download's file is hidden");
    }
}

fn try_set_hidden(path: &Path, hidden: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::set_hidden(path, hidden)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, hidden);
        Ok(())
    }
}

/// Reserves `total` bytes for a new, empty part file at `path` if the size is known, or fails with [`io::ErrorKind::StorageFull`] when the disk does not have them: a sparse file reserves nothing, so the download would only find out when the disk is full.
pub(crate) fn size_part(path: &Path, file: &File, total: Option<u64>) -> io::Result<()> {
    let Some(len) = total else {
        return Ok(());
    };
    if let Some(free) = free_bytes(path, file)
        && free < len
    {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            format!("the file needs {len} bytes and the disk has {free} free"),
        ));
    }
    reserve(file, len)
}

/// What this user may still write on the disk of the part file, when the system says.
#[cfg(windows)]
fn free_bytes(path: &Path, _file: &File) -> Option<u64> {
    let dir = path.parent()?;
    windows::free_bytes(dir)
        .inspect_err(
            |e| tracing::debug!(error = %crate::error::Chain(e), "could not read the free space"),
        )
        .ok()
}

/// What this user may still write on the disk of the part file, when the system says.
#[cfg(target_os = "linux")]
fn free_bytes(_path: &Path, file: &File) -> Option<u64> {
    linux::free_bytes(file)
        .inspect_err(
            |e| tracing::debug!(error = %crate::error::Chain(e), "could not read the free space"),
        )
        .ok()
        .flatten()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn free_bytes(_path: &Path, _file: &File) -> Option<u64> {
    None
}

/// Opens an existing part file for positional writes.
pub fn open_for_write(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(path)
}

/// A run's hold on its part file, for as long as this lives: another run of this process or another sees it and leaves the file alone, and the system lets go of it when the process dies, however it dies. Where the file system has no such locks (some network drives) it holds nothing.
#[derive(Debug, Default)]
pub(crate) struct PartLock(Option<File>);

impl PartLock {
    /// Takes the lock on `file`. `None` when another run holds it.
    pub fn hold(file: File) -> Option<Self> {
        match try_lock(&file) {
            Ok(true) => Some(Self(Some(file))),
            Ok(false) => None,
            Err(e) => {
                let error = crate::error::Chain(&e);
                tracing::debug!(%error, "the file system offers no lock on a part file");
                Some(Self(None))
            }
        }
    }

    /// Takes the lock on the part file at `path`, if it can be opened. `None` when another run holds it.
    pub fn open(path: &Path) -> Option<Self> {
        match open_for_write(path) {
            Ok(file) => Self::hold(file),
            Err(_) => Some(Self::default()),
        }
    }

    pub fn is_held(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(windows)]
fn try_lock(file: &File) -> io::Result<bool> {
    windows::try_lock_far_byte(file)
}

#[cfg(target_os = "linux")]
fn try_lock(file: &File) -> io::Result<bool> {
    linux::try_flock(file)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn try_lock(_file: &File) -> io::Result<bool> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Writes all of `buf` at `offset` without touching any shared cursor.
pub fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = write_at(file, buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, buf, offset)
}

/// What the finish does when a file is already at the final name.
#[derive(Debug, Clone)]
pub(crate) enum Finish {
    /// Replace it: the path is the caller's ([`ExistingFile::Replace`](crate::ExistingFile::Replace)).
    Replace,
    /// Leave it and end with [`Error::TargetTaken`](crate::Error::TargetTaken) ([`ExistingFile::Refuse`](crate::ExistingFile::Refuse)).
    Refuse,
    /// Leave it and take the first free name of `name` in `dir` instead (a download into a folder).
    FreeName { dir: PathBuf, name: String },
}

/// Moves the finished part file to its final name and tidies file flags, and returns where the file went: `final_path`, or with [`Finish::FreeName`] another name when another program saved a file at `final_path` while the download ran.
///
/// The data is flushed to the disk before the rename, so a file under its final
/// name is complete even after a power cut. The rename itself may be lost to
/// one; the part file and its resume file are then still there, and the next
/// run finishes them again.
///
/// Nothing is renamed once the download is `abandoned`, checked again after the flush: a slow flush is when an owner gives up, and the part file may then already be another run's.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) naming the step that failed and its file: opening, sizing or flushing the part file, or the rename to `final_path`; [`Error::TargetTaken`](crate::Error::TargetTaken) with [`Finish::Refuse`] when a file is at `final_path`; [`Error::Cancelled`](crate::Error::Cancelled) once abandoned.
pub(crate) fn finalize(
    part: &Path,
    final_path: &Path,
    finish: &Finish,
    len: u64,
    abandoned: &Abandoned,
) -> crate::Result<PathBuf> {
    {
        let file = open_for_write(part).context(IoAction::OpenFile, part)?;
        // Unknown-length downloads may have reserved nothing; known ones already match.
        let size = file.metadata().context(IoAction::Resize, part)?.len();
        if size != len {
            file.set_len(len).context(IoAction::Resize, part)?;
        }
        file.sync_data().context(IoAction::Flush, part)?;
        // Fully written now, so the sparse flag can go. A file that keeps it
        // reads the same, only less efficiently.
        #[cfg(windows)]
        if let Err(e) = windows::set_sparse(&file, false) {
            let error = crate::error::Chain(&e);
            tracing::debug!(%error, "could not clear the sparse flag of a finished file");
        }
    }
    #[cfg(test)]
    abandoned.slow_step_done();
    abandoned.check()?;
    // Before the rename, which keeps the attribute: a kill between leaves a part file on show, never a hidden finished file.
    set_hidden(part, false);
    let placed = place(part, final_path, finish);
    if placed.is_err() {
        set_hidden(part, true);
    }
    placed
}

/// Renames the part file to `final_path`, or as `finish` says when a file is there.
fn place(part: &Path, final_path: &Path, finish: &Finish) -> crate::Result<PathBuf> {
    match finish {
        Finish::Replace => std::fs::rename(part, final_path)
            .context(IoAction::Finish, final_path)
            .map(|()| final_path.to_path_buf()),
        Finish::Refuse => match rename_no_replace(part, final_path) {
            Ok(()) => Ok(final_path.to_path_buf()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(crate::Error::TargetTaken {
                path: final_path.to_path_buf(),
            }),
            Err(e) => Err(e).context(IoAction::Finish, final_path),
        },
        Finish::FreeName { dir, name } => match rename_no_replace(part, final_path) {
            Ok(()) => Ok(final_path.to_path_buf()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let moved = crate::naming::move_to_free_name(part, dir, name)
                    .context(IoAction::Finish, final_path)?;
                tracing::info!(taken = %final_path.display(), path = %moved.display(), "another file took the name meanwhile; saved under a free one");
                Ok(moved)
            }
            Err(e) => Err(e).context(IoAction::Finish, final_path),
        },
    }
}

/// Renames `from` to `to` unless something is already at `to`, in one step
/// where the system has one, failing with [`io::ErrorKind::AlreadyExists`]
/// then. See [`naming::move_to_free_name`](crate::naming::move_to_free_name)
/// for how each system does it.
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::move_no_replace(from, to)
    }
    #[cfg(target_os = "linux")]
    {
        match linux::rename_no_replace(from, to) {
            // `EPERM` too: a sandbox can answer a call it blocks that way. A
            // real `EPERM` comes back from the hard link and the rename.
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EINVAL | libc::ENOSYS | libc::EPERM)
                ) =>
            {
                link_then_unlink(from, to)
            }
            done => done,
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        link_then_unlink(from, to)
    }
}

/// A hard link at `to`, which fails when `to` exists, then `from` removed. On
/// a file system without hard links, a check that `to` is free and a rename,
/// the one way left, which a file saved at `to` in between is lost to. All or
/// nothing: when `from` cannot be removed, `to` is removed again, so a file is
/// never left under two names, where writing to one would change the other.
#[cfg(unix)]
fn link_then_unlink(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from).inspect_err(|_| {
            if let Err(e) = std::fs::remove_file(to) {
                let error = crate::error::Chain(&e);
                tracing::warn!(path = %to.display(), %error, "could not take back the new name of a file that did not move");
            }
        }),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) && std::fs::symlink_metadata(from).is_ok() =>
        {
            if std::fs::symlink_metadata(to).is_ok() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            std::fs::rename(from, to)
        }
        Err(e) => Err(e),
    }
}

/// Reserves space for `len` bytes without writing them.
///
/// On NTFS a plain `set_len` makes the first write far into the file zero-fill
/// everything before it, which doubles disk traffic for segmented downloads.
/// Marking the file sparse first avoids that. On Linux `fallocate` reserves
/// real, contiguous extents without writing zeros.
fn reserve(file: &File, len: u64) -> io::Result<()> {
    #[cfg(windows)]
    {
        // Not every file system supports sparse files (FAT32, exFAT); without
        // the flag, space is reserved the slower way.
        if let Err(e) = windows::set_sparse(file, true) {
            let error = crate::error::Chain(&e);
            tracing::debug!(%error, "the file system does not take sparse files");
        }
        file.set_len(len)
    }
    #[cfg(target_os = "linux")]
    {
        // Only where the file system has no `fallocate`: a full disk stays an error.
        linux::fallocate(file, len).or_else(|e| match e.raw_os_error() {
            Some(libc::ENOSPC | libc::EDQUOT) => Err(e),
            _ => file.set_len(len),
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        file.set_len(len)
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, GetDiskFreeSpaceExW, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, MOVEFILE_WRITE_THROUGH,
        MoveFileExW, SetFileAttributesW,
    };

    use windows_sys::Win32::System::IO::{DeviceIoControl, OVERLAPPED};
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};

    /// Past any real file's end. Not the file's own bytes: a Windows lock stops other handles' writes, and each connection writes through a handle of its own. The top bit stays clear, since the system reads the offset as signed.
    const LOCK_OFFSET_LOW: u32 = u32::MAX - 1;
    const LOCK_OFFSET_HIGH: u32 = u32::MAX >> 1;
    const LOCK_BYTES: u32 = 1;

    /// Locks one byte far past the end of `file`: `false` when another handle holds it.
    pub fn try_lock_far_byte(file: &File) -> io::Result<bool> {
        // SAFETY: OVERLAPPED is plain data, for which all zeros is a valid value.
        let mut at: OVERLAPPED = unsafe { std::mem::zeroed() };
        at.Anonymous.Anonymous.Offset = LOCK_OFFSET_LOW;
        at.Anonymous.Anonymous.OffsetHigh = LOCK_OFFSET_HIGH;
        // SAFETY: the handle is valid for the lifetime of `file`, and `at` lives until the call returns, which it does at once on a file not opened for overlapped use.
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle().cast(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                LOCK_BYTES,
                0,
                &raw mut at,
            )
        };
        if ok != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        let busy = [ERROR_LOCK_VIOLATION, ERROR_IO_PENDING]
            .iter()
            .any(|&code| error.raw_os_error() == i32::try_from(code).ok());
        if busy { Ok(false) } else { Err(error) }
    }

    /// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: fails with
    /// `ERROR_ALREADY_EXISTS` when `to` exists, in one step on NTFS, `ReFS` and
    /// SMB shares. `MOVEFILE_WRITE_THROUGH` returns once the move is on the disk.
    pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (verbatim(from)?, verbatim(to)?);
        // SAFETY: both are NUL-terminated UTF-16 strings that live until the call returns.
        let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// What the caller may still write on the disk holding folder `dir`, quotas counted.
    pub fn free_bytes(dir: &Path) -> io::Result<u64> {
        let dir = verbatim(&dir.join(""))?;
        let mut free = 0u64;
        // SAFETY: `dir` is a NUL-terminated UTF-16 string and `free` a u64, both alive until the call returns; the other two outputs are optional.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                dir.as_ptr(),
                &raw mut free,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(free)
        }
    }

    /// `path` as a NUL-terminated wide string in the `\\?\` form, which the
    /// system takes as it is, without the 260-character limit. That form skips
    /// the system's own clean-up of the path, so the path is made absolute and
    /// cleaned first (`std::path::absolute`).
    fn verbatim(path: &Path) -> io::Result<Vec<u16>> {
        let absolute = std::path::absolute(path)?;
        let units: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        let mut out = verbatim_form(&units);
        out.push(0);
        Ok(out)
    }

    /// An absolute, cleaned `path` in the `\\?\` form: `C:\...` becomes
    /// `\\?\C:\...` and `\\server\share\...` becomes `\\?\UNC\server\share\...`.
    /// A device path (`\\?\` or `\\.\`) is kept as it is.
    fn verbatim_form(path: &[u16]) -> Vec<u16> {
        let starts = |prefix: &str| path.starts_with(&prefix.encode_utf16().collect::<Vec<_>>());
        let (prefix, rest) = if starts(r"\\?\") || starts(r"\\.\") {
            ("", path)
        } else if starts(r"\\") {
            (r"\\?\UNC", &path[1..])
        } else {
            (r"\\?\", path)
        };
        prefix.encode_utf16().chain(rest.iter().copied()).collect()
    }

    /// Sets or clears only the hidden attribute, keeping the others.
    pub fn set_hidden(path: &Path, hidden: bool) -> io::Result<()> {
        let wide = verbatim(path)?;
        // SAFETY: `wide` is a NUL-terminated wide string that lives until the call returns.
        let now = unsafe { GetFileAttributesW(wide.as_ptr()) };
        if now == INVALID_FILE_ATTRIBUTES {
            return Err(io::Error::last_os_error());
        }
        let wanted = if hidden {
            now | FILE_ATTRIBUTE_HIDDEN
        } else {
            now & !FILE_ATTRIBUTE_HIDDEN
        };
        if wanted == now {
            return Ok(());
        }
        // SAFETY: as above. Attributes the call cannot set, such as sparse, are ignored by it.
        let ok = unsafe { SetFileAttributesW(wide.as_ptr(), wanted) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn set_sparse(file: &File, sparse: bool) -> io::Result<()> {
        let input = FILE_SET_SPARSE_BUFFER { SetSparse: sparse };
        let input_len =
            u32::try_from(size_of::<FILE_SET_SPARSE_BUFFER>()).map_err(io::Error::other)?;
        let mut returned = 0u32;
        // SAFETY: the handle is valid for the lifetime of `file`, the input buffer
        // is a properly sized FILE_SET_SPARSE_BUFFER, and no output buffer is used.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle().cast(),
                FSCTL_SET_SPARSE,
                (&raw const input).cast::<c_void>(),
                input_len,
                ptr::null_mut(),
                0,
                &raw mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::verbatim_form;

        fn form(path: &str) -> String {
            String::from_utf16(&verbatim_form(&path.encode_utf16().collect::<Vec<_>>())).unwrap()
        }

        #[test]
        fn every_kind_of_absolute_path_takes_the_verbatim_form() {
            assert_eq!(form(r"C:\d\f"), r"\\?\C:\d\f");
            assert_eq!(form(r"\\server\share\f"), r"\\?\UNC\server\share\f");
            assert_eq!(form(r"\\?\C:\d\f"), r"\\?\C:\d\f");
            assert_eq!(form(r"\\?\UNC\server\share\f"), r"\\?\UNC\server\share\f");
            // A device path is one already.
            assert_eq!(form(r"\\.\C:\d\f"), r"\\.\C:\d\f");
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// `renameat2` with `RENAME_NOREPLACE`, called through `syscall` so no C
    /// library version is needed: fails with `EEXIST` when `to` exists, with
    /// `EINVAL` or `ENOSYS` where the file system or kernel lacks the flag, and
    /// with `EPERM` (or `ENOSYS`) where a sandbox blocks the call.
    pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: both are NUL-terminated strings that live until the call
        // returns, and `renameat2` takes exactly these five arguments.
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// `flock`, not `fcntl`: an `fcntl` lock belongs to the process, so a second run in this process would not see it, and closing any handle of the file would drop it. `false` when another open file holds it.
    pub fn try_flock(file: &File) -> io::Result<bool> {
        // SAFETY: plain syscall on a valid descriptor.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    /// What an unprivileged user may still write on the file system of `file`, when it says.
    pub fn free_bytes(file: &File) -> io::Result<Option<u64>> {
        // SAFETY: `statvfs` is plain data, for which all zeros is a valid value.
        let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: a valid descriptor and a pointer to a `statvfs` that outlives the call.
        let rc = unsafe { libc::fstatvfs(file.as_raw_fd(), &raw mut stats) };
        if rc == 0 {
            Ok(said_free(stats.f_blocks, stats.f_bavail, stats.f_frsize))
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// A file system of no blocks reports nothing: a FUSE mount that leaves `statfs` to libfuse's default says all zeros, whatever it holds.
    fn said_free(blocks: u64, available: u64, block_size: u64) -> Option<u64> {
        (blocks > 0).then(|| available.saturating_mul(block_size))
    }

    pub fn fallocate(file: &File, len: u64) -> io::Result<()> {
        let len =
            libc::off_t::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: plain syscall on a valid descriptor.
        let rc = unsafe { libc::fallocate(file.as_raw_fd(), 0, 0, len) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::said_free;

        #[test]
        fn a_file_system_that_counts_nothing_says_nothing_about_free_space() {
            assert_eq!(said_free(0, 0, 0), None);
            assert_eq!(said_free(0, 0, 4096), None);
            assert_eq!(said_free(10, 0, 4096), Some(0));
            assert_eq!(said_free(10, 2, 4096), Some(8192));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abandon::Abandonment;

    #[test]
    fn a_held_part_file_refuses_another_run_and_still_takes_writes() {
        let dir = wirecatch_test_support::private_dir("part-lock").unwrap();
        let path = dir.join("f.bin.wcpart");
        create_part(&path, Some(8)).unwrap();
        let held = PartLock::open(&path).expect("a free part file");
        assert!(PartLock::open(&path).is_none(), "taken twice");
        // Each connection writes through a handle of its own.
        write_all_at(&open_for_write(&path).unwrap(), b"data", 0).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[..4], b"data");
        drop(held);
        assert!(PartLock::open(&path).is_some(), "free once the run ended");
        dir.remove().unwrap();
    }

    #[test]
    fn a_file_larger_than_the_free_space_is_refused_before_anything_is_reserved() {
        const MARGIN: u64 = 1 << 30;
        let dir = wirecatch_test_support::private_dir("no-room").unwrap();
        let path = dir.join("f.bin.wcpart");
        let file = File::create(&path).unwrap();
        let Some(free) = free_bytes(&path, &file) else {
            panic!("this system tells the free space");
        };
        let error = size_part(&path, &file, Some(free.saturating_add(MARGIN))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert_eq!(file.metadata().unwrap().len(), 0);
        size_part(&path, &file, Some(1)).unwrap();
        drop(file);
        dir.remove().unwrap();
    }

    #[test]
    fn a_rename_that_must_not_replace_leaves_a_taken_name_alone() {
        let dir = wirecatch_test_support::private_dir("no-replace").unwrap();
        let (from, to) = (dir.join("from"), dir.join("to"));
        std::fs::write(&from, b"mine").unwrap();
        std::fs::write(&to, b"theirs").unwrap();
        let error = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error:?}");
        assert_eq!(std::fs::read(&from).unwrap(), b"mine");
        assert_eq!(std::fs::read(&to).unwrap(), b"theirs");

        std::fs::remove_file(&to).unwrap();
        rename_no_replace(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"mine");
        assert!(!from.exists());
        dir.remove().unwrap();
    }

    /// Paths longer than the 260 characters Windows takes by default, and written with `..`, still move.
    #[test]
    fn a_rename_that_must_not_replace_takes_long_and_relative_paths() {
        let dir = wirecatch_test_support::private_dir("no-replace-long").unwrap();
        let deep = (0..6).fold(dir.to_path_buf(), |path, i| {
            path.join(format!("{i}-{}", "d".repeat(60)))
        });
        std::fs::create_dir_all(&deep).unwrap();
        let from = deep.join("from");
        std::fs::write(&from, b"mine").unwrap();
        // Linux follows `x/..` through `x` itself, so it has to exist there.
        std::fs::create_dir(deep.join("x")).unwrap();
        let to = deep.join("x").join("..").join("to");
        assert!(to.as_os_str().len() > 300);
        rename_no_replace(&from, &to).unwrap();
        assert_eq!(std::fs::read(deep.join("to")).unwrap(), b"mine");
        dir.remove().unwrap();
    }

    /// When the old name cannot be removed, the new one is taken back: the move did not happen, and the file is not left under two names.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_link_whose_old_name_stays_is_undone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = wirecatch_test_support::private_dir("link-undo").unwrap();
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        let (from, to) = (locked.join("from"), dir.join("to"));
        std::fs::write(&from, b"mine").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // An account that writes into a read-only folder anyway (root) cannot see this.
        let privileged = std::fs::write(locked.join("probe"), b"").is_ok();
        let result = (!privileged).then(|| link_then_unlink(&from, &to));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if let Some(result) = result {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error:?}");
            assert_eq!(std::fs::read(&from).unwrap(), b"mine");
            assert!(
                std::fs::symlink_metadata(&to).is_err(),
                "the new name stayed"
            );
        }
        dir.remove().unwrap();
    }

    #[test]
    fn a_finish_that_refuses_keeps_the_file_there_and_the_data() {
        let dir = wirecatch_test_support::private_dir("finalize-refuse").unwrap();
        let (part, done) = (dir.join("f.bin.wcpart"), dir.join("f.bin"));
        create_part(&part, Some(10)).unwrap();
        std::fs::write(&done, b"theirs").unwrap();
        let live = Abandonment::new();
        let error = finalize(&part, &done, &Finish::Refuse, 10, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::TargetTaken { path, .. } if *path == done),
            "{error:?}"
        );
        assert_eq!(std::fs::read(&done).unwrap(), b"theirs");
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 10);
        dir.remove().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_part_file_is_hidden_until_it_is_the_finished_file() {
        use wirecatch_test_support::is_hidden;
        let dir = wirecatch_test_support::private_dir("finalize-hidden").unwrap();
        let live = Abandonment::new();
        let (part, done) = (dir.join("f.bin.wcpart"), dir.join("f.bin"));
        create_part(&part, Some(10)).unwrap();
        assert!(is_hidden(&part));
        create_part(&part, Some(10)).unwrap();
        assert!(is_hidden(&part), "emptied again, still hidden");
        finalize(&part, &done, &Finish::Replace, 10, &live.flag()).unwrap();
        assert!(!is_hidden(&done));
        dir.remove().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_part_file_a_taken_name_keeps_stays_hidden() {
        let dir = wirecatch_test_support::private_dir("finalize-taken-hidden").unwrap();
        let live = Abandonment::new();
        let (part, done) = (dir.join("f.bin.wcpart"), dir.join("f.bin"));
        create_part(&part, Some(10)).unwrap();
        std::fs::write(&done, b"theirs").unwrap();
        finalize(&part, &done, &Finish::Refuse, 10, &live.flag()).unwrap_err();
        assert!(wirecatch_test_support::is_hidden(&part));
        assert!(!wirecatch_test_support::is_hidden(&done));
        dir.remove().unwrap();
    }

    #[test]
    fn a_download_dropped_during_the_final_flush_is_not_renamed() {
        let dir = wirecatch_test_support::private_dir("finalize-dropped").unwrap();
        let part = dir.join("f.bin.wcpart");
        let done = dir.join("f.bin");
        create_part(&part, Some(10)).unwrap();
        let abandonment = Abandonment::new();
        let abandoned = abandonment.flag();
        abandoned.drop_after_slow_step();
        let error = finalize(&part, &done, &Finish::Replace, 10, &abandoned).unwrap_err();
        assert!(matches!(error, crate::Error::Cancelled), "{error:?}");
        assert!(part.exists() && !done.exists(), "the part file was renamed");
        drop(abandonment);
        dir.remove().unwrap();
    }

    #[test]
    fn out_of_order_positional_writes_assemble_the_file() {
        let dir = wirecatch_test_support::private_dir("storage").unwrap();
        let live = Abandonment::new();
        let part = dir.join("f.bin.wcpart");
        let done = dir.join("f.bin");

        create_part(&part, Some(10)).unwrap();
        let f = open_for_write(&part).unwrap();
        write_all_at(&f, b"56789", 5).unwrap();
        write_all_at(&f, b"01234", 0).unwrap();
        drop(f);
        finalize(&part, &done, &Finish::Replace, 10, &live.flag()).unwrap();

        assert_eq!(std::fs::read(&done).unwrap(), b"0123456789");
        dir.remove().unwrap();
    }

    /// Each step that can fail says which: a missing part file is not a
    /// failed rename to the final name.
    #[test]
    fn a_failed_finish_names_its_step_and_file() {
        let dir = wirecatch_test_support::private_dir("finalize").unwrap();
        let live = Abandonment::new();
        let part = dir.join("gone.bin.wcpart");
        let done = dir.join("gone.bin");
        let error = finalize(&part, &done, &Finish::Replace, 10, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::OpenFile, path, .. } if *path == part),
            "{error:?}"
        );

        create_part(&part, Some(10)).unwrap();
        // A length past the largest signed file offset, which setting the size refuses (`InvalidInput`).
        let error = finalize(&part, &done, &Finish::Replace, u64::MAX, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::Resize, path, .. } if *path == part),
            "{error:?}"
        );
        let elsewhere = dir.join("missing-folder").join("gone.bin");
        let error = finalize(&part, &elsewhere, &Finish::Replace, 10, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::Finish, path, .. } if *path == elsewhere),
            "{error:?}"
        );
        dir.remove().unwrap();
    }
}
