//! Making the data folder and database readable only by the current user (D17):
//! the database holds cookies. On Windows the folder gets a protected access list
//! that grants the user alone full control, inherited by every file created in it;
//! on Linux the folder is mode 0700 and files 0600.

use std::io;
use std::path::Path;

/// Restricts `path` (a folder or a file) to the current user.
pub fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(any(test, feature = "testing"))]
    if REFUSED.get() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    imp::restrict(path)
}

#[cfg(any(test, feature = "testing"))]
thread_local! {
    static REFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes every later [`restrict`] on this thread fail, as on a drive that keeps no
/// permissions or a folder another account owns.
#[cfg(any(test, feature = "testing"))]
pub fn refuse_on_this_thread() {
    REFUSED.set(true);
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub fn restrict(path: &Path) -> io::Result<()> {
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::core::PWSTR;

    /// Frees memory Windows allocated with `LocalAlloc` when dropped.
    struct LocalMemory(*mut core::ffi::c_void);

    impl Drop for LocalMemory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from a Windows API that documents LocalFree.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    /// Closes a handle when dropped.
    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by us and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn restrict(path: &Path) -> io::Result<()> {
        let sid = current_user_sid()?;
        // D: the access list. P: protected, so nothing is inherited from the
        // parent. One entry: Allow, inherited by subfolders and files (OICI), full
        // access (FA), for the current user.
        let sddl = wide(format!("D:P(A;OICI;FA;;;{sid})").encode_utf16());
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is freed by the guard.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalMemory(descriptor);

        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = null_mut();
        // SAFETY: `descriptor` is a valid descriptor; `dacl` points into it and is
        // only used while the descriptor is alive.
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &raw mut present,
                &raw mut dacl,
                &raw mut defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let name = wide(path.as_os_str().encode_wide());
        // SAFETY: `name` is NUL-terminated and `dacl` is valid for this call.
        let status = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status.cast_signed()));
        }
        Ok(())
    }

    /// The current user's security id, as text (`S-1-5-21-...`).
    pub fn current_user_sid() -> io::Result<String> {
        let mut token: HANDLE = null_mut();
        // SAFETY: plain out-parameter call; the handle is closed by the guard.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _token = OwnedHandle(token);

        let mut len = 0u32;
        // SAFETY: asks only for the size; failing with "buffer too small" is expected.
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &raw mut len) };
        // u64 elements keep the buffer aligned for TOKEN_USER.
        let mut buffer = vec![0u64; (len as usize).div_ceil(8).max(1)];
        // SAFETY: the buffer holds at least `len` bytes.
        let filled = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &raw mut len,
            )
        };
        if filled == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: on success the buffer starts with an aligned TOKEN_USER whose SID
        // points into the same buffer, which outlives this use.
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text: PWSTR = null_mut();
        // SAFETY: `sid` is valid; the string is freed by the guard.
        if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _text = LocalMemory(text.cast());
        // SAFETY: Windows returned a NUL-terminated string.
        Ok(unsafe { from_wide(text) })
    }

    fn wide(units: impl Iterator<Item = u16>) -> Vec<u16> {
        units.chain(std::iter::once(0)).collect()
    }

    /// Reads a NUL-terminated UTF-16 string.
    ///
    /// # Safety
    /// `text` must point to a NUL-terminated UTF-16 string.
    pub unsafe fn from_wide(text: PWSTR) -> String {
        let mut len = 0;
        // SAFETY: the caller promises a terminating NUL.
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` units were just read.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
    }

    #[cfg(test)]
    pub mod inspect {
        use super::*;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
        };
        use windows_sys::Win32::Security::{
            OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        };

        /// How SDDL writes the account `sid` (`S-1-...`): its alias when it has
        /// one, such as `LA` for the built-in Administrator that CI runners use,
        /// otherwise `sid` itself.
        pub fn sddl_account(sid: &str) -> io::Result<String> {
            let owner = wide(format!("O:{sid}").encode_utf16());
            let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
            // SAFETY: `owner` is NUL-terminated; the descriptor is freed by the guard.
            let converted = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    owner.as_ptr(),
                    SDDL_REVISION_1,
                    &raw mut descriptor,
                    null_mut(),
                )
            };
            if converted == 0 {
                return Err(io::Error::last_os_error());
            }
            let _descriptor = LocalMemory(descriptor);
            let text = descriptor_text(descriptor, OWNER_SECURITY_INFORMATION)?;
            text.strip_prefix("O:")
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other(format!("not an owner: {text}")))
        }

        /// The access list of `path` in SDDL form.
        pub fn dacl_text(path: &Path) -> io::Result<String> {
            let name = wide(path.as_os_str().encode_wide());
            let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
            // SAFETY: out-parameter call; the descriptor is freed by the guard.
            let status = unsafe {
                GetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &raw mut descriptor,
                )
            };
            if status != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(status.cast_signed()));
            }
            let _descriptor = LocalMemory(descriptor);
            descriptor_text(descriptor, DACL_SECURITY_INFORMATION)
        }

        /// The parts of `descriptor` that `information` names, in SDDL form.
        fn descriptor_text(
            descriptor: PSECURITY_DESCRIPTOR,
            information: OBJECT_SECURITY_INFORMATION,
        ) -> io::Result<String> {
            let mut text: PWSTR = null_mut();
            // SAFETY: the caller passes a valid descriptor; the string is freed by the guard.
            let converted = unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    information,
                    &raw mut text,
                    null_mut(),
                )
            };
            if converted == 0 {
                return Err(io::Error::last_os_error());
            }
            let _text = LocalMemory(text.cast());
            // SAFETY: Windows returned a NUL-terminated string.
            Ok(unsafe { from_wide(text) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn only_the_current_user_can_open_the_folder_and_what_is_created_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        restrict(&data).unwrap();
        let file = data.join("downloads.db");
        std::fs::write(&file, b"x").unwrap();

        let user = imp::current_user_sid().unwrap();
        let allowed = format!(";;;{})", imp::inspect::sddl_account(&user).unwrap());
        let folder = imp::inspect::dacl_text(&data).unwrap();
        assert!(
            folder.starts_with("D:P"),
            "inherits from its parent: {folder}"
        );
        assert_eq!(folder.matches("(A;").count(), 1, "{folder}");
        assert!(folder.ends_with(&allowed), "{folder}");
        let created = imp::inspect::dacl_text(&file).unwrap();
        assert_eq!(created.matches("(A;").count(), 1, "{created}");
        assert!(created.ends_with(&allowed), "{created}");
    }

    #[cfg(windows)]
    #[test]
    fn an_account_with_an_alias_is_written_by_its_alias() {
        // LocalSystem; the built-in Administrator CI runs as is `LA` the same way.
        assert_eq!(imp::inspect::sddl_account("S-1-5-18").unwrap(), "SY");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_current_user_can_open_the_folder_and_the_database() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("downloads.db");
        std::fs::write(&file, b"x").unwrap();
        restrict(dir.path()).unwrap();
        restrict(&file).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.path()), 0o700);
        assert_eq!(mode(&file), 0o600);
    }
}
