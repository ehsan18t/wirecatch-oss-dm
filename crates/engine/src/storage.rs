//! File creation, space reservation and positional writes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Suffix of the file that receives data while a download runs.
pub const PART_EXT: &str = ".wcpart";
/// Suffix of the resume file next to it.
pub const STATE_EXT: &str = ".wcstate";

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
pub fn finalize(part: &Path, final_path: &Path, len: u64) -> io::Result<()> {
    {
        let file = open_for_write(part)?;
        // Unknown-length downloads may have reserved nothing; known ones already match.
        if file.metadata()?.len() != len {
            file.set_len(len)?;
        }
        #[cfg(windows)]
        {
            // Fully written now, so the sparse flag can go. Failure is harmless.
            let _ = windows::set_sparse(&file, false);
        }
    }
    std::fs::rename(part, final_path)
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
        // Not every file system supports sparse files (FAT32, exFAT). Fall through.
        let _ = windows::set_sparse(file, true);
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
            SetSparse: sparse as u8,
        };
        let mut returned = 0u32;
        // SAFETY: the handle is valid for the lifetime of `file`, the input buffer
        // is a properly sized FILE_SET_SPARSE_BUFFER, and no output buffer is used.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle() as _,
                FSCTL_SET_SPARSE,
                &input as *const FILE_SET_SPARSE_BUFFER as *const c_void,
                size_of::<FILE_SET_SPARSE_BUFFER>() as u32,
                ptr::null_mut(),
                0,
                &mut returned,
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
        // SAFETY: plain syscall on a valid descriptor.
        let rc = unsafe { libc::fallocate(file.as_raw_fd(), 0, 0, len as libc::off_t) };
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

    #[test]
    fn out_of_order_positional_writes_assemble_the_file() {
        let dir = std::env::temp_dir().join(format!("wc-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("f.bin.wcpart");
        let done = dir.join("f.bin");

        create_part(&part, Some(10)).unwrap();
        let f = open_for_write(&part).unwrap();
        write_all_at(&f, b"56789", 5).unwrap();
        write_all_at(&f, b"01234", 0).unwrap();
        drop(f);
        finalize(&part, &done, 10).unwrap();

        assert_eq!(std::fs::read(&done).unwrap(), b"0123456789");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
