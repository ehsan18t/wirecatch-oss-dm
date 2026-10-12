//! Opens the download manager once and hands it to everything that needs it (D13).
//!
//! The manager opens in the background while the window appears, so the window never waits on
//! the database. Commands that arrive first simply wait for it.

use std::path::{Path, PathBuf};

use tokio::sync::OnceCell;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};
use wirecatch_ipc::{Endpoint, IpcError};

/// Overrides the data folder, for tests and for trying things on a throwaway download list.
/// Debug builds already keep their own folder, identity and endpoint (see [`DEV_BUILD`]).
pub const DATA_DIR_VAR: &str = "WIRECATCH_DATA_DIR";

/// Debug builds run as a separate app, "Wirecatch Dev", with their own identifier (so the
/// single-instance guard does not hand launches to an installed Wirecatch), data folder and
/// browser endpoint. A development copy can then run next to an installed one without either
/// touching the other's downloads.
pub const DEV_BUILD: bool = cfg!(debug_assertions);

/// The identifier of debug builds; release builds use the one in `tauri.conf.json`.
pub const DEV_IDENTIFIER: &str = "app.wirecatch.desktop.dev";

/// The name debug builds show and use for their folder and start-at-sign-in entry.
pub const DEV_NAME: &str = "Wirecatch Dev";

/// The download manager this process hosts, opened on first use.
pub struct Core {
    config: ManagerConfig,
    cell: OnceCell<Result<Manager, String>>,
}

impl Core {
    /// A manager that will open with `config`.
    #[must_use]
    pub fn new(config: ManagerConfig) -> Self {
        Self {
            config,
            cell: OnceCell::new(),
        }
    }

    /// The manager, opening it if nobody has yet.
    ///
    /// # Errors
    ///
    /// The manager could not open (a database from a newer Wirecatch, say, or a data folder
    /// that cannot be written). The text is the plain-language reason, ready to show to a
    /// person, and every later call answers with it again.
    pub async fn manager(&self) -> Result<&Manager, String> {
        self.cell
            .get_or_init(|| async {
                Manager::open(self.config.clone()).await.map_err(|e| {
                    tracing::error!("could not open the download manager: {e}");
                    e.to_string()
                })
            })
            .await
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The manager if it has opened successfully, without waiting.
    pub fn get(&self) -> Option<&Manager> {
        self.cell.get().and_then(|r| r.as_ref().ok())
    }
}

/// The per-user folder for the database, cookies and add-ons: `%LOCALAPPDATA%\Wirecatch` on
/// Windows (local, not roaming, so none of it follows a roaming profile to other computers, D21.5),
/// `~/.local/share/wirecatch` on Linux (`Wirecatch Dev` and `wirecatch-dev` for debug builds).
///
/// `local_base` is the platform's local user data folder and `roaming_base` its roaming one
/// (the same folder on Linux). A data folder an earlier version left in the roaming folder is
/// moved to the local one first, once, when the local one does not exist yet; if that fails, the
/// old folder is used as it is, so nothing is lost. Blocks on the disk: call it before the
/// manager opens.
pub fn data_dir(local_base: &Path, roaming_base: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_VAR).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let new = data_dir_for(local_base, DEV_BUILD);
    let old = data_dir_for(roaming_base, DEV_BUILD);
    let (used, migration) = migrate(&old, &new);
    match &migration {
        Migration::Nothing => {}
        Migration::Moved | Migration::Copied => tracing::info!(
            from = %old.display(),
            to = %new.display(),
            ?migration,
            "moved the data folder out of the roaming profile"
        ),
        Migration::Failed(reason) => tracing::warn!(
            from = %old.display(),
            to = %new.display(),
            %reason,
            "could not move the data folder out of the roaming profile; using it where it is"
        ),
    }
    used
}

/// What happened to a data folder left in the roaming folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migration {
    /// There was nothing to move.
    Nothing,
    /// Renamed in one step (same drive).
    Moved,
    /// Copied, then the old folder deleted (another drive).
    Copied,
    /// Not moved; the old folder stays in use. The reason, for the log.
    Failed(String),
}

/// Whether the data folder must move from `old` to `new`: only when they are different folders,
/// the old one exists and the new one does not, so it happens once and never overwrites anything.
#[must_use]
pub fn needs_move(old: &Path, new: &Path, old_exists: bool, new_exists: bool) -> bool {
    old != new && old_exists && !new_exists
}

/// Moves the data folder `old` to `new` when [`needs_move`] says so: a rename on the same drive,
/// otherwise a copy and then deleting the old folder. Returns the folder to use and what happened.
/// Blocks on the disk.
#[must_use]
pub fn migrate(old: &Path, new: &Path) -> (PathBuf, Migration) {
    if !needs_move(old, new, old.is_dir(), new.exists()) {
        return (new.to_path_buf(), Migration::Nothing);
    }
    if let Some(parent) = new.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return (old.to_path_buf(), Migration::Failed(e.to_string()));
    }
    if std::fs::rename(old, new).is_ok() {
        return (new.to_path_buf(), Migration::Moved);
    }
    match copy_then_delete(old, new) {
        Ok(()) => (new.to_path_buf(), Migration::Copied),
        Err(e) => (old.to_path_buf(), Migration::Failed(e.to_string())),
    }
}

/// Copies the folder `from` to `to` (which must not exist), then deletes `from`. A copy that fails
/// halfway is removed again, leaving `from` as it was. Blocks on the disk.
///
/// # Errors
///
/// `to` exists already, or a folder or file cannot be read, made or copied. A failure to
/// delete `from` afterwards is only logged: everything is in `to` by then.
pub fn copy_then_delete(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Err(e) = copy_dir(from, to) {
        let _ = std::fs::remove_dir_all(to);
        return Err(e);
    }
    if let Err(e) = std::fs::remove_dir_all(from) {
        // Everything is in the new folder already; the old one is only clutter now.
        tracing::warn!(folder = %from.display(), error = %e, "could not delete the old data folder");
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
        // Links are not followed: the data folder never holds any, and one put there should
        // not pull another folder's files along.
    }
    Ok(())
}

fn data_dir_for(base: &Path, dev: bool) -> PathBuf {
    base.join(match (cfg!(windows), dev) {
        (true, false) => "Wirecatch",
        (true, true) => DEV_NAME,
        (false, false) => "wirecatch",
        (false, true) => "wirecatch-dev",
    })
}

/// The browser endpoint this build serves: the per-user endpoint, or a sibling of it for debug
/// builds.
///
/// # Errors
///
/// The current user cannot be read (Windows), or the endpoint's folder cannot be created or
/// is not private to this user (Linux); see [`wirecatch_ipc::endpoint`].
pub fn endpoint() -> Result<Endpoint, IpcError> {
    let real = wirecatch_ipc::endpoint()?;
    Ok(if DEV_BUILD { dev_endpoint(&real) } else { real })
}

/// The debug build's endpoint next to `real`: `\\.\pipe\wirecatch-app-dev-<SID>` on Windows,
/// `app-dev.sock` in the same private folder on Linux.
#[must_use]
pub fn dev_endpoint(real: &Endpoint) -> Endpoint {
    let name = real.as_os_str().to_string_lossy();
    let dev = if let Some(rest) = name.strip_suffix("app.sock") {
        format!("{rest}app-dev.sock")
    } else {
        name.replacen("wirecatch-app-", "wirecatch-app-dev-", 1)
    };
    Endpoint::from_raw(dev)
}

/// The user's standard folders, falling back to folders under `home` when the system does not
/// say where they are.
#[must_use]
pub fn known_folders(home: &Path) -> KnownFolders {
    KnownFolders::detect().unwrap_or_else(|| KnownFolders::under_home(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_and_debug_builds_keep_separate_folders() {
        let name = |dev| {
            data_dir_for(Path::new("base"), dev)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_lowercase()
        };
        assert_eq!(name(false), "wirecatch");
        assert_ne!(name(true), name(false));
        assert!(name(true).ends_with("dev"));
        assert!(data_dir_for(Path::new("base"), true).starts_with("base"));
    }

    #[test]
    fn the_data_folder_moves_only_once_and_never_over_another() {
        let (old, new) = (Path::new("roaming/Wirecatch"), Path::new("local/Wirecatch"));
        assert!(needs_move(old, new, true, false));
        // Already moved, or never there: nothing to do.
        assert!(!needs_move(old, new, true, true));
        assert!(!needs_move(old, new, false, false));
        // Linux: one folder for both.
        assert!(!needs_move(old, old, true, false));
    }

    #[test]
    fn an_old_roaming_data_folder_moves_to_the_local_one() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("Roaming").join("Wirecatch");
        let new = root.path().join("Local").join("Wirecatch");
        std::fs::create_dir_all(old.join("addons").join("media")).unwrap();
        std::fs::write(old.join("wirecatch.db"), b"downloads").unwrap();
        std::fs::write(
            old.join("addons").join("media").join("yt-dlp.exe"),
            b"program",
        )
        .unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Moved));
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(new.join("wirecatch.db")).unwrap(),
            b"downloads"
        );
        assert_eq!(
            std::fs::read(new.join("addons").join("media").join("yt-dlp.exe")).unwrap(),
            b"program"
        );
        // A second start finds the new folder and leaves everything alone.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("wirecatch.db"), b"stale").unwrap();
        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Nothing));
        assert_eq!(
            std::fs::read(new.join("wirecatch.db")).unwrap(),
            b"downloads"
        );
    }

    /// Another drive: the folder is copied, then the old one deleted.
    #[test]
    fn a_data_folder_on_another_drive_is_copied_then_deleted() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        std::fs::create_dir_all(old.join("bin")).unwrap();
        std::fs::write(old.join("wirecatch.db"), b"downloads").unwrap();
        std::fs::write(old.join("bin").join("bridge"), b"bridge").unwrap();
        copy_then_delete(&old, &new).unwrap();
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(new.join("wirecatch.db")).unwrap(),
            b"downloads"
        );
        assert_eq!(
            std::fs::read(new.join("bin").join("bridge")).unwrap(),
            b"bridge"
        );
        // A copy that cannot finish leaves the old folder as it was and nothing half done.
        let missing = root.path().join("missing");
        assert!(copy_then_delete(&missing, &root.path().join("half")).is_err());
        assert!(!root.path().join("half").exists());
    }

    #[test]
    fn debug_builds_listen_next_to_the_real_endpoint() {
        let pipe = Endpoint::from_raw(r"\\.\pipe\wirecatch-app-S-1-5-21-1-2-3-1001");
        assert_eq!(
            dev_endpoint(&pipe).to_string(),
            r"\\.\pipe\wirecatch-app-dev-S-1-5-21-1-2-3-1001"
        );
        let socket = Endpoint::from_raw("/run/user/1000/wirecatch/app.sock");
        assert_eq!(
            dev_endpoint(&socket).to_string(),
            "/run/user/1000/wirecatch/app-dev.sock"
        );
    }
}
