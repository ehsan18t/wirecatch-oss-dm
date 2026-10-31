//! Installing a newer version of the app (D111) once the core has downloaded and checked it. On
//! Windows the setup runs without questions and starts the new version itself; an AppImage
//! takes the place of the one running. A `.deb` or `.rpm` install needs the system's package
//! manager, so the app offers the release page instead.

use std::path::Path;

pub use wirecatch_updates::FileKind;

/// The installer the app can install itself, as it was installed; `None` for a system package.
#[must_use]
pub fn install_kind() -> Option<FileKind> {
    if cfg!(windows) {
        Some(FileKind::Setup)
    } else if std::env::var_os(crate::appimage::APPIMAGE_VAR).is_some() {
        Some(FileKind::AppImage)
    } else {
        None
    }
}

/// Starts installing the checked installer at `installer`; the caller quits the app next, so
/// the installer can replace it.
///
/// # Errors
///
/// The installer could not be started, or the AppImage could not take the old one's place.
pub fn install(installer: &Path, kind: FileKind) -> Result<(), String> {
    match kind {
        FileKind::Setup => run_setup(installer),
        FileKind::AppImage => replace_appimage(installer),
        FileKind::Deb | FileKind::Rpm => {
            Err("A system package is installed by the system's package manager.".into())
        }
    }
}

/// Runs the setup through the shell, which asks for an administrator's approval as the setup
/// requires; a plain process start cannot. `/SILENT` shows progress and asks nothing, and the
/// setup starts the new version when it is done.
#[cfg(windows)]
fn run_setup(installer: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &std::ffi::OsStr| {
        text.encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let file = wide(installer.as_os_str());
    let arguments = wide("/SILENT /SUPPRESSMSGBOXES /NORESTART".as_ref());
    let verb = wide("open".as_ref());
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
        Err(format!(
            "The update could not be started (error {}).",
            started as usize
        ))
    }
}

#[cfg(not(windows))]
fn run_setup(_installer: &Path) -> Result<(), String> {
    Err("A Windows setup cannot run here.".into())
}

/// Moves the new AppImage over the running one, in one step on the same folder, and starts it.
#[cfg(unix)]
fn replace_appimage(installer: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    let current = std::env::var_os(crate::appimage::APPIMAGE_VAR)
        .map(std::path::PathBuf::from)
        .ok_or("The app is not running from an AppImage.")?;
    let folder = current.parent().ok_or("The AppImage has no folder.")?;
    let staged = folder.join(".wirecatch-update.AppImage");
    let failed = |e: std::io::Error| {
        format!(
            "The update could not be put in place of {} ({e}).",
            current.display()
        )
    };
    std::fs::copy(installer, &staged).map_err(failed)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(failed)?;
    std::fs::rename(&staged, &current).map_err(failed)?;
    std::process::Command::new(&current)
        .spawn()
        .map_err(|e| format!("The updated app could not be started ({e})."))?;
    Ok(())
}

#[cfg(not(unix))]
fn replace_appimage(_installer: &Path) -> Result<(), String> {
    Err("An AppImage cannot run here.".into())
}
