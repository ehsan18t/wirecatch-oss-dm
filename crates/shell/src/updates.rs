//! Installing a newer version of the app (D111) once the core has downloaded and checked it. On
//! Windows the setup runs without questions and starts the new version itself; an AppImage
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
/// the caller starts it again after it has exited.
///
/// # Errors
///
/// The installer could not be started, or the AppImage could not take the old one's place.
pub fn install(installer: &Path, kind: FileKind) -> Result<(), InstallError> {
    match kind {
        FileKind::Setup => run_setup(installer),
        FileKind::AppImage => replace_appimage(installer),
        FileKind::Deb | FileKind::Rpm => Err(InstallError::SystemPackage),
    }
}

/// Runs the setup through the shell, which asks for an administrator's approval as the setup
/// requires; a plain process start cannot. `/SILENT` shows progress and asks nothing, and the
/// setup starts the new version when it is done.
#[cfg(windows)]
fn run_setup(installer: &Path) -> Result<(), InstallError> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use wirecatch_ipc::wide;

    let file = wide(installer);
    let arguments = wide("/SILENT /SUPPRESSMSGBOXES /NORESTART");
    let verb = wide("open");
    // SAFETY: every buffer is NUL-terminated UTF-16 that outlives the call; a null window and
    // folder are allowed.
    let started = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            arguments.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW reports success as a value above 32.
    if started as usize > 32 {
        Ok(())
    } else {
        Err(InstallError::Start(started as usize))
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
