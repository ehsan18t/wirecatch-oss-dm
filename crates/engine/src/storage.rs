//! File creation, space reservation and positional writes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

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
    if let Some(len) = total {
        reserve(&file, len)?;
    }
    Ok(())
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

/// Moves the finished part file to its final name and tidies file flags.
///
/// The data is flushed to the disk before the rename, so a file under its final
/// name is complete even after a power cut. The rename itself may be lost to
/// one; the part file and its resume file are then still there, and the next
/// run finishes them again.
///
/// Nothing is renamed once the download is `abandoned`, checked again after
/// the flush: a slow flush is when an owner gives up, and the part file may
/// then already be another run's.
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) naming the step that failed and its file:
/// opening, sizing or flushing the part file, or the rename to `final_path`;
/// [`Error::Cancelled`](crate::Error::Cancelled) once abandoned.
pub(crate) fn finalize(
    part: &Path,
    final_path: &Path,
    len: u64,
    abandoned: &Abandoned,
) -> crate::Result<()> {
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
            tracing::debug!(error = %e, "could not clear the sparse flag of a finished file");
        }
    }
    abandoned.flushed();
    abandoned.check()?;
    std::fs::rename(part, final_path).context(IoAction::Finish, final_path)
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
            tracing::debug!(error = %e, "the file system does not take sparse files");
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
    use std::os::windows::io::AsRawHandle;
    use std::ptr;

    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};

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
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;

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
    fn a_download_dropped_during_the_final_flush_is_not_renamed() {
        let dir = wirecatch_test_support::private_dir("finalize-dropped").unwrap();
        let part = dir.join("f.bin.wcpart");
        let done = dir.join("f.bin");
        create_part(&part, Some(10)).unwrap();
        let abandonment = Abandonment::new();
        let abandoned = abandonment.flag();
        abandoned.drop_at_flush();
        let error = finalize(&part, &done, 10, &abandoned).unwrap_err();
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
        finalize(&part, &done, 10, &live.flag()).unwrap();

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
        let error = finalize(&part, &done, 10, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::OpenFile, path, .. } if *path == part),
            "{error:?}"
        );

        create_part(&part, Some(10)).unwrap();
        // No file system holds a file this long.
        let error = finalize(&part, &done, u64::MAX, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::Resize, path, .. } if *path == part),
            "{error:?}"
        );
        let elsewhere = dir.join("missing-folder").join("gone.bin");
        let error = finalize(&part, &elsewhere, 10, &live.flag()).unwrap_err();
        assert!(
            matches!(&error, crate::Error::Io { action: IoAction::Finish, path, .. } if *path == elsewhere),
            "{error:?}"
        );
        dir.remove().unwrap();
    }
}
