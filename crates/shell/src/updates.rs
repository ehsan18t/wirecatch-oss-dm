//! Installing a newer version of the app (D111) once the core has downloaded and checked it. On
//! Windows the setup asks only for approval and starts the new version itself; an AppImage
//! takes the place of the one running. A `.deb` or `.rpm` install needs the system's package
//! manager, so the app offers the release page instead.

use std::path::{Path, PathBuf};

pub use wirecatch_updates::FileKind;

/// The new AppImage's name beside the running one until it takes its place.
#[cfg(unix)]
const STAGED_APPIMAGE: &str = ".wirecatch-update.AppImage";

/// Why an update could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("A system package is installed by the system's package manager.")]
    SystemPackage,
    #[error("A Windows setup cannot run here.")]
    SetupElsewhere,
    #[error("An AppImage cannot run here.")]
    AppImageElsewhere,
    #[error("The update could not be started (error {0}).")]
    Start(usize),
    #[error(
        "Setup ended without installing the update. When Windows asks for approval, choose Yes, then try again."
    )]
    SetupEnded,
    #[error("The update's setup could not be followed ({0}).")]
    Follow(#[source] std::io::Error),
    #[error("The app is not running from an AppImage.")]
    NotAppImage,
    #[error("The update could not be put in place of {} ({source}).", .current.display())]
    Replace {
        current: PathBuf,
        source: std::io::Error,
    },
}

/// The installer the app can install itself, as it was installed; `None` for a system package.
#[must_use]
pub fn install_kind() -> Option<FileKind> {
    if cfg!(windows) {
        Some(FileKind::Setup)
    } else if crate::appimage::running_appimage().is_some() {
        Some(FileKind::AppImage)
    } else {
        None
    }
}

/// Starts installing the checked installer at `installer`; the caller quits the app next, so
/// the installer can replace it. A setup starts the new version itself; an AppImage does not, and
/// the caller starts it again after it has exited. Blocks, on Windows until the setup has its
/// approval.
///
/// # Errors
///
/// The installer could not be started or ended without its approval, or the AppImage could not
/// take the old one's place.
pub fn install(installer: &Path, kind: FileKind) -> Result<(), InstallError> {
    match kind {
        FileKind::Setup => run_setup(installer),
        FileKind::AppImage => replace_appimage(installer),
        FileKind::Deb | FileKind::Rpm => Err(InstallError::SystemPackage),
    }
}

/// `/SILENT` shows progress and asks nothing, and the setup starts the new version when it is done.
#[cfg(windows)]
const SETUP_SWITCHES: &str = "/SILENT /SUPPRESSMSGBOXES /NORESTART";
/// Names this process to the setup, whose `{param:UPDATEFROM}` in `installer.iss` reads it.
#[cfg(windows)]
const UPDATE_FROM_SWITCH: &str = "/UPDATEFROM";
/// With this process's id, the event the setup sets from `installer.iss` once it is approved.
#[cfg(windows)]
const APPROVED_EVENT_PREFIX: &str = r"Local\AppUpdateApproved-";

/// Runs the setup and waits until it has its administrator's approval, or has ended without. The
/// setup asks for approval itself after it has started (it is `asInvoker`), so the shell's answer
/// says nothing about it, and the app has to stay to show a refusal.
#[cfg(windows)]
fn run_setup(installer: &Path) -> Result<(), InstallError> {
    let pid = std::process::id();
    let approved = approved_event(pid).map_err(InstallError::Follow)?;
    let arguments = format!("{SETUP_SWITCHES} {UPDATE_FROM_SWITCH}={pid}");
    let setup = start_through_shell(installer.as_os_str(), &arguments)?;
    wait_for_approval(&approved, &setup)
}

#[cfg(windows)]
fn approved_event(pid: u32) -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::FromRawHandle as _;
    use windows_sys::Win32::System::Threading::CreateEventW;
    use wirecatch_ipc::wide;

    let name = wide(format!("{APPROVED_EVENT_PREFIX}{pid}"));
    // SAFETY: the name is NUL-terminated UTF-16 that outlives the call; null security attributes
    // are allowed.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    if event.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `event` is a handle just opened here and owned by nothing else.
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(event) })
}

/// Starts `file` through the shell and returns its process.
#[cfg(windows)]
fn start_through_shell(
    file: &std::ffi::OsStr,
    arguments: &str,
) -> Result<std::os::windows::io::OwnedHandle, InstallError> {
    use std::os::windows::io::FromRawHandle as _;
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use wirecatch_ipc::wide;

    let file = wide(file);
    let arguments = wide(arguments);
    let verb = wide("open");
    // SAFETY: all zeros is a valid value of this plain struct: null pointers and no flags.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX);
    // No error box from the shell: the app shows the failure itself.
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = arguments.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: `info` has its size set and is valid for the call; every buffer it points to is
    // NUL-terminated UTF-16 that outlives the call.
    let started = unsafe { ShellExecuteExW(&raw mut info) };
    if started == 0 || info.hProcess.is_null() {
        return Err(InstallError::Start(info.hInstApp as usize));
    }
    // SAFETY: SEE_MASK_NOCLOSEPROCESS hands this process handle to the caller to close.
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(info.hProcess) })
}

#[cfg(windows)]
fn wait_for_approval(
    approved: &std::os::windows::io::OwnedHandle,
    setup: &std::os::windows::io::OwnedHandle,
) -> Result<(), InstallError> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForMultipleObjects};

    let handles = [approved.as_raw_handle(), setup.as_raw_handle()];
    // SAFETY: both handles are open for the whole call, and the count is the array's length.
    let woken = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
    if woken == WAIT_OBJECT_0 {
        Ok(())
    } else if woken == WAIT_OBJECT_0 + 1 {
        Err(InstallError::SetupEnded)
    } else {
        Err(InstallError::Follow(std::io::Error::last_os_error()))
    }
}

#[cfg(not(windows))]
fn run_setup(_installer: &Path) -> Result<(), InstallError> {
    Err(InstallError::SetupElsewhere)
}

/// Moves the new AppImage over the running one. Not started here: a copy started while this one
/// still holds the endpoint would hand its launch over and leave, and nothing would be running.
#[cfg(unix)]
fn replace_appimage(installer: &Path) -> Result<(), InstallError> {
    let current = crate::appimage::running_appimage().ok_or(InstallError::NotAppImage)?;
    put_in_place(installer, &current)
}

/// Copies `installer` beside `current`, flushed so a crash leaves one whole program or the other,
/// and renames it over `current`, in one step on the same folder. A failure leaves `current` as it
/// was, and no copy behind.
#[cfg(unix)]
fn put_in_place(installer: &Path, current: &Path) -> Result<(), InstallError> {
    use std::os::unix::fs::PermissionsExt as _;

    let folder = current.parent().ok_or(InstallError::NotAppImage)?;
    let staged = folder.join(STAGED_APPIMAGE);
    let placed = std::fs::copy(installer, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)))
        .and_then(|()| std::fs::File::open(&staged)?.sync_all())
        .and_then(|()| std::fs::rename(&staged, current));
    placed.map_err(|source| {
        wirecatch_files::discard::file(&staged);
        InstallError::Replace {
            current: current.to_owned(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn replace_appimage(_installer: &Path) -> Result<(), InstallError> {
    Err(InstallError::AppImageElsewhere)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const OTHER_PROGRAM: &[u8] = b"#!/bin/sh\n# another program\n";
    const UPDATE: &[u8] = b"#!/bin/sh\n# the update\n";

    #[test]
    fn the_new_appimage_takes_the_old_ones_place_and_can_run() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let (current, installer) = (dir.path().join("App.AppImage"), dir.path().join("new"));
        std::fs::write(&current, OTHER_PROGRAM).unwrap();
        std::fs::write(&installer, UPDATE).unwrap();
        put_in_place(&installer, &current).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), UPDATE);
        let mode = std::fs::metadata(&current).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!dir.path().join(STAGED_APPIMAGE).exists());
    }

    #[test]
    fn a_failed_replacement_leaves_no_copy_behind() {
        let dir = tempfile::tempdir().unwrap();
        // A folder cannot be renamed over by a file.
        let current = dir.path().join("App.AppImage");
        std::fs::create_dir(&current).unwrap();
        let installer = dir.path().join("new");
        std::fs::write(&installer, UPDATE).unwrap();
        assert!(matches!(
            put_in_place(&installer, &current),
            Err(InstallError::Replace { .. })
        ));
        assert!(!dir.path().join(STAGED_APPIMAGE).exists());
    }

    #[test]
    fn an_update_does_not_overwrite_the_program_named_by_an_inherited_variable() {
        const NAME: &str = "updates::tests::an_update_does_not_overwrite_the_program_named_by_an_inherited_variable";
        let installer_beside = |other: &Path| other.with_file_name("new.AppImage");
        match std::env::var_os(crate::appimage::APPIMAGE_VAR) {
            None => {
                let dir = tempfile::tempdir().unwrap();
                let other = dir.path().join("Other.AppImage");
                std::fs::write(&other, OTHER_PROGRAM).unwrap();
                std::fs::write(installer_beside(&other), UPDATE).unwrap();
                assert!(crate::appimage::passes_with_inherited_appimage(
                    NAME, &other
                ));
                assert_eq!(std::fs::read(&other).unwrap(), OTHER_PROGRAM);
            }
            Some(other) => {
                let installer = installer_beside(Path::new(&other));
                assert!(matches!(
                    replace_appimage(&installer),
                    Err(InstallError::NotAppImage)
                ));
                assert_eq!(install_kind(), None);
            }
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    const SETUP_SCRIPT: &str = include_str!("../../desktop/packaging/windows/installer.iss");

    fn finished_program() -> std::os::windows::io::OwnedHandle {
        start_through_shell(std::ffi::OsStr::new("cmd.exe"), "/c exit 1")
            .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_setup_that_ends_unapproved_is_a_failure() {
        // Event names are shared by every process, so each test has its own.
        let approved = approved_event(u32::MAX - 1).unwrap();
        assert!(matches!(
            wait_for_approval(&approved, &finished_program()),
            Err(InstallError::SetupEnded)
        ));
    }

    #[test]
    fn a_setup_that_sets_the_event_is_approved() {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::System::Threading::SetEvent;

        let approved = approved_event(u32::MAX - 2).unwrap();
        // SAFETY: the handle is open for the whole call.
        assert_ne!(unsafe { SetEvent(approved.as_raw_handle()) }, 0);
        assert!(wait_for_approval(&approved, &finished_program()).is_ok());
    }

    #[test]
    fn a_program_that_cannot_be_started_is_reported() {
        assert!(matches!(
            start_through_shell(std::ffi::OsStr::new(r"C:\no\such\setup.exe"), ""),
            Err(InstallError::Start(_))
        ));
    }

    #[test]
    fn the_setup_script_uses_the_names_the_app_gives_it() {
        let switch = UPDATE_FROM_SWITCH.trim_start_matches('/');
        assert!(SETUP_SCRIPT.contains(&format!("{{param:{switch}|")));
        assert!(SETUP_SCRIPT.contains(APPROVED_EVENT_PREFIX));
    }
}
