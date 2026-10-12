//! Registering the bridge when the app runs from an AppImage.
//!
//! An AppImage runs from a temporary mount (`/tmp/.mount_XXXX`) that disappears when the app
//! exits. Registering the bridge inside it would point every browser at a path that, once
//! unmounted, another local user could create and fill with a program of their own. So under an
//! AppImage the app copies the bridge into a private folder of the user's data folder (mode
//! 0700, owned by the user, never through a symbolic link) and registers that copy. Next to it,
//! a two-line `wirecatch-app` script starts the AppImage itself, because the bridge starts
//! `wirecatch-app --background` from its own folder. A temporary path is never registered.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

/// Set by the AppImage runtime to the path of the `.AppImage` file.
pub const APPIMAGE_VAR: &str = "APPIMAGE";

/// File name of the launcher script next to the private bridge; the bridge starts this name.
pub const LAUNCHER_FILE_NAME: &str = "wirecatch-app";

/// Whether `path` lies in a temporary location, where a registered program could disappear and
/// be replaced by someone else.
#[must_use]
pub fn is_temporary(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.contains("/.mount_") {
        return true;
    }
    let temp = std::env::temp_dir();
    [
        Path::new("/tmp"),
        Path::new("/var/tmp"),
        Path::new("/dev/shm"),
        &temp,
    ]
    .iter()
    .any(|dir| path.starts_with(dir))
}

/// `text` as one single-quoted shell word.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The launcher script that starts the AppImage at `appimage` with the bridge's arguments.
///
/// # Errors
///
/// The path is not valid text, is not a plain absolute path, or is in a temporary folder that
/// will be gone after a restart. Each message says what to do.
pub fn launcher_script(appimage: &Path) -> Result<String, String> {
    let text = appimage.to_str().ok_or_else(|| {
        format!(
            "The AppImage path {} is not valid text, so the browser bridge cannot start it. Rename the file or its folder.",
            appimage.display()
        )
    })?;
    // AppImages are Linux only, where an absolute path starts with a slash.
    if !text.starts_with('/') || text.chars().any(char::is_control) {
        return Err(format!(
            "The AppImage path {text} is not a plain absolute path, so the browser bridge cannot start it."
        ));
    }
    if is_temporary(appimage) {
        return Err(format!(
            "The AppImage is in a temporary folder ({text}). Move it somewhere permanent, for example ~/Applications, so your browser can start Wirecatch."
        ));
    }
    Ok(format!(
        "#!/bin/sh\n# Starts Wirecatch from its AppImage for the browser bridge. Written by Wirecatch on each start.\nexec {} \"$@\"\n",
        shell_quote(text)
    ))
}

/// Copies the bundled bridge and writes the launcher into `bin_dir`, privately, and returns the
/// copy to register.
///
/// # Errors
///
/// The AppImage's path cannot go in a launcher (see [`launcher_script`]); `bin_dir` is a link,
/// not a plain folder, or belongs to another user; or a folder or file cannot be made, read or
/// written. Each message says what to do.
#[cfg(unix)]
pub fn install_private_bridge(
    bundled_bridge: &Path,
    appimage: &Path,
    bin_dir: &Path,
) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let script = launcher_script(appimage)?;
    let fail = |what: &str, e: std::io::Error| {
        format!(
            "Could not {what} in {} ({e}), so browsers cannot reach Wirecatch from this AppImage.",
            bin_dir.display()
        )
    };
    if let Some(parent) = bin_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| fail("make the data folder", e))?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(bin_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(fail("make the bridge folder", e)),
    }
    let meta =
        std::fs::symlink_metadata(bin_dir).map_err(|e| fail("check the bridge folder", e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{} is not a plain folder (it may be a link to somewhere else), so Wirecatch will not put the browser bridge there. Delete it and start Wirecatch again.",
            bin_dir.display()
        ));
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    if meta.uid() != me {
        return Err(format!(
            "{} belongs to another user account, so Wirecatch will not put the browser bridge there. Delete it and start Wirecatch again.",
            bin_dir.display()
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(bin_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| fail("make the bridge folder private", e))?;
    }
    let bridge = std::fs::read(bundled_bridge).map_err(|e| fail("read the bundled bridge", e))?;
    let target = bin_dir.join(crate::bridge::BRIDGE_FILE_NAME);
    write_private(&target, &bridge).map_err(|e| fail("copy the bridge", e))?;
    write_private(&bin_dir.join(LAUNCHER_FILE_NAME), script.as_bytes())
        .map_err(|e| fail("write the launcher", e))?;
    Ok(target)
}

/// Writes `bytes` to `dest` as a file only the user can read, write or run: into a new file
/// (never through an existing file or link), then renamed over `dest`.
#[cfg(unix)]
fn write_private(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;

    let mut name = dest.as_os_str().to_owned();
    name.push(".new");
    let tmp = PathBuf::from(name);
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_and_temporary_folders_are_never_registered() {
        assert!(is_temporary(Path::new(
            "/tmp/.mount_WirecaXYZ/usr/bin/wirecatch-bridge"
        )));
        assert!(is_temporary(Path::new("/var/tmp/wirecatch-bridge")));
        assert!(is_temporary(&std::env::temp_dir().join("wirecatch-bridge")));
        assert!(!is_temporary(Path::new("/usr/bin/wirecatch-bridge")));
        assert!(!is_temporary(Path::new(
            "/home/ana/.local/share/wirecatch/bin/wirecatch-bridge"
        )));
    }

    #[test]
    fn the_launcher_starts_the_appimage_with_its_path_safely_quoted() {
        let script = launcher_script(Path::new("/home/ana/Apps/Wire 'catch' $(x).AppImage"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(
            script.contains(r#"exec '/home/ana/Apps/Wire '\''catch'\'' $(x).AppImage' "$@""#),
            "{script}"
        );
    }

    #[test]
    fn odd_appimage_paths_are_refused() {
        assert!(launcher_script(Path::new("relative/Wirecatch.AppImage")).is_err());
        assert!(launcher_script(Path::new("/home/ana/a\nb.AppImage")).is_err());
        assert!(launcher_script(Path::new("/tmp/Wirecatch.AppImage")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_private_copy_is_owner_only_and_refuses_links() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        // The test's own temporary folder counts as temporary, so the AppImage lives elsewhere.
        let appimage = Path::new("/opt/Wirecatch.AppImage");
        let bundled = dir.path().join("bundled-bridge");
        std::fs::write(&bundled, b"bridge").unwrap();

        let bin = dir.path().join("data").join("bin");
        let copy = install_private_bridge(&bundled, appimage, &bin).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"bridge");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&bin), 0o700);
        assert_eq!(mode(&copy), 0o700);
        let script = std::fs::read_to_string(bin.join(LAUNCHER_FILE_NAME)).unwrap();
        assert!(script.contains("'/opt/Wirecatch.AppImage'"));

        // A bridge folder that is a link to somewhere else is refused.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        assert!(install_private_bridge(&bundled, appimage, &linked).is_err());
        assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none());
    }
}
