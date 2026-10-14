//! File creation, space reservation and positional writes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::abandon::Abandoned;
use crate::error::{IoAction, IoContext};

/// Suffix of the file that receives data while a download runs.
pub const PART_EXT: &str = ".wcpart";
/// Suffix of the resume file next to it.
pub const STATE_EXT: &str = ".wcstate";

/// Runs file system work on the blocking pool, off the async threads, where a
/// slow disk, an antivirus scan or a network drive stalls nothing else.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> crate::Result<T> + Send + 'static,
) -> crate::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| crate::Error::task(&e))?
}

/// Creates (or truncates) the part file and reserves `total` bytes if known.
pub fn create_part(path: &Path, total: Option<u64>) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    size_part(&file, total)
}

/// Reserves `total` bytes for a new, empty part file if the size is known.
pub(crate) fn size_part(file: &File, total: Option<u64>) -> io::Result<()> {
    match total {
        Some(len) => reserve(file, len),
        None => Ok(()),
    }
}

/// Opens an existing part file for positional writes.
pub fn open_for_write(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(path)
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
        linux::fallocate(file, len).or_else(|_| file.set_len(len))
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

    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};

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

    pub fn set_sparse(file: &File, sparse: bool) -> io::Result<()> {
        let input = FILE_SET_SPARSE_BUFFER {
            SetSparse: u8::from(sparse),
        };
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abandon::Abandonment;

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
