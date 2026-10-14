//! Opens the download manager once and hands it to everything that needs it (D13).
//!
//! The manager opens in the background while the window appears, so the window never waits on
//! the database. Commands that arrive first simply wait for it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio::sync::OnceCell;
use wirecatch_core::{
    CoreError, DATABASE_FILE, DATABASE_SIDE_FILES, KnownFolders, Manager, ManagerConfig,
};
use wirecatch_ipc::{Endpoint, IpcError};
use wirecatch_product::Channel;
use wirecatch_protocol::wire::ProtocolError;

/// Overrides the data folder, for tests and for trying things on a throwaway download list.
/// Debug builds already keep their own folder, identity and endpoint (see [`DEV_BUILD`]).
pub const DATA_DIR_VAR: &str = "WIRECATCH_DATA_DIR";

/// Debug builds run as a separate app, [`Channel::Dev`], with their own name, identifier (so the
/// single-instance guard does not hand launches to an installed Wirecatch), data folder and
/// browser endpoint. A development copy can then run next to an installed one without either
/// touching the other's downloads.
pub const DEV_BUILD: bool = cfg!(debug_assertions);

/// The channel this build belongs to, which names its data folder, endpoint and start-at-sign-in
/// entry (`config/product.toml`).
pub const CHANNEL: Channel = if DEV_BUILD {
    Channel::Dev
} else {
    Channel::Release
};

/// The download manager this process hosts, opened on first use.
pub struct Core {
    config: ManagerConfig,
    cell: OnceCell<Result<Manager, Unopened>>,
}

/// Why the manager did not open.
#[derive(Debug, Clone)]
pub struct Unopened {
    /// The core's code and plain-language reason, ready to show to a person.
    pub reason: ProtocolError,
    /// Another copy of Wirecatch has the download list open, so this copy says that Wirecatch runs already and closes, rather than staying open without a list.
    pub another_copy: bool,
}

impl Unopened {
    fn from_core(error: &CoreError) -> Self {
        Self {
            reason: error.to_protocol(),
            another_copy: matches!(error, CoreError::AlreadyOpen { .. }),
        }
    }
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
    /// that cannot be written). The error carries the core's code and plain-language reason,
    /// ready to show to a person, and every later call answers with it again.
    pub async fn manager(&self) -> Result<&Manager, ProtocolError> {
        self.opened()
            .await
            .map_err(|unopened| unopened.reason.clone())
    }

    /// The manager, opening it if nobody has yet.
    ///
    /// # Errors
    ///
    /// Why the manager could not open, including whether another copy of Wirecatch has the list open; every later call answers the same.
    pub async fn opened(&self) -> Result<&Manager, &Unopened> {
        self.cell
            .get_or_init(|| async {
                Manager::open(self.config.clone()).await.map_err(|e| {
                    tracing::error!("could not open the download manager: {e}");
                    Unopened::from_core(&e)
                })
            })
            .await
            .as_ref()
    }

    /// The manager if it has opened successfully, without waiting.
    pub fn get(&self) -> Option<&Manager> {
        self.cell.get().and_then(|r| r.as_ref().ok())
    }
}

/// The per-user folder for the database, cookies and add-ons: [`Channel::data_folder`] in
/// `%LOCALAPPDATA%` on Windows (local, not roaming, so none of it follows a roaming profile to other
/// computers, D21.5), and in `~/.local/share` on Linux.
///
/// `local_base` is the platform's local user data folder and `roaming_base` its roaming one (the same folder on Linux). A data folder an earlier version left in the roaming folder is moved to the local one first, once, when the local one holds no download list yet (see [`migrate`]); if that fails, the old folder is used as it is, so nothing is lost. Blocks on the disk: call it before the manager opens.
pub fn data_dir(local_base: &Path, roaming_base: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_VAR).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let new = local_base.join(CHANNEL.data_folder());
    let old = roaming_base.join(CHANNEL.data_folder());
    let (used, migration) = migrate(&old, &new);
    match &migration {
        Migration::Nothing => {}
        Migration::Moved | Migration::Copied | Migration::Merged => tracing::info!(
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

/// Where the app logs: `logs/` in the data folder [`DATA_DIR_VAR`] names (`data_dir_var`), or else in this channel's local data folder, where [`data_dir`] moves the data folder. Known before the app does anything, so a start that fails early is logged too. `None` when the system names no local data folder.
#[must_use]
pub fn log_folder(data_dir_var: Option<OsString>) -> Option<PathBuf> {
    match data_dir_var.filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(wirecatch_logging::folder_in(Path::new(&dir))),
        None => wirecatch_logging::channel_folder(CHANNEL),
    }
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
    /// Moved entry by entry into a local folder that other programs had made without the download list (the bridge's and the command-line downloader's logs).
    Merged,
    /// Not moved; the old folder stays in use. The reason, for the log.
    Failed(String),
}

/// Whether `folder` holds a download list. That, not whether the folder exists, says a data folder is in use: the bridge and the command-line downloader make the local folder for their logs before an upgraded app first starts there.
fn holds_data(folder: &Path) -> bool {
    folder.join(DATABASE_FILE).is_file()
}

/// Whether the data folder must move from `old` to `new`: only when they are different folders, the old one exists and the new one holds no download list, so it happens once and never overwrites a list.
#[must_use]
pub fn needs_move(old: &Path, new: &Path) -> bool {
    old != new && old.is_dir() && !holds_data(new)
}

/// Moves the data folder `old` to `new` when [`needs_move`] says so: a rename on the same drive, otherwise [`copy_then_delete`], which never leaves a partial copy at `new`; into a `new` that other programs made already, entry by entry ([`merge`]). Returns the folder to use and what happened. Blocks on the disk.
#[must_use]
pub fn migrate(old: &Path, new: &Path) -> (PathBuf, Migration) {
    if !needs_move(old, new) {
        return (new.to_path_buf(), Migration::Nothing);
    }
    if let Some(parent) = new.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return (old.to_path_buf(), Migration::Failed(e.to_string()));
    }
    if !new.exists() {
        if std::fs::rename(old, new).is_ok() {
            return (new.to_path_buf(), Migration::Moved);
        }
        // The bridge may make the folder between the look and the rename.
        if !new.exists() {
            return match copy_then_delete(old, new) {
                Ok(()) => (new.to_path_buf(), Migration::Copied),
                Err(e) => (old.to_path_buf(), Migration::Failed(e.to_string())),
            };
        }
    }
    match merge(old, new) {
        Ok(()) => (new.to_path_buf(), Migration::Merged),
        Err(e) => (old.to_path_buf(), Migration::Failed(e.to_string())),
    }
}

/// Moves everything in the data folder `old` into the folder `new`, which exists without a download list, then deletes `old`. What `new` holds already stays, since programs that ran later wrote it (a running bridge keeps its log open there); only the database's side files give way, because without the database they can only be left from a move cut short. The database goes last, copied, so until it is in place `old` keeps a whole download list to use, and the next start carries on from where this one stopped. Blocks on the disk.
///
/// # Errors
///
/// An entry cannot be read, moved or copied, or the database cannot be put in place.
fn merge(old: &Path, new: &Path) -> std::io::Result<()> {
    let database = database_files();
    for entry in std::fs::read_dir(old)? {
        let entry = entry?;
        if !database.contains(&entry.file_name()) {
            merge_entry(&entry.path(), &new.join(entry.file_name()))?;
        }
    }
    move_database(old, new, &database)?;
    if let Err(e) = std::fs::remove_dir_all(old) {
        // Everything is in the new folder already; the old one is only clutter now.
        tracing::warn!(folder = %old.display(), error = %e, "could not delete the old data folder");
    }
    Ok(())
}

/// The database's file names, its side files first, so the database itself is always the last put in place.
fn database_files() -> Vec<OsString> {
    DATABASE_SIDE_FILES
        .iter()
        .map(|side| format!("{DATABASE_FILE}{side}").into())
        .chain(std::iter::once(DATABASE_FILE.into()))
        .collect()
}

/// Moves the file or folder `from` to `to`, merging into a folder already at `to`; what is there already stays, and the old copy is deleted.
fn merge_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        // Not followed, as in `copy_dir`; it goes with the old folder.
        return Ok(());
    }
    match std::fs::symlink_metadata(to) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::rename(from, to).or_else(|_| copy_then_delete(from, to))
        }
        Err(e) => Err(e),
        Ok(there) if kind.is_dir() && there.is_dir() => {
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                merge_entry(&entry.path(), &to.join(entry.file_name()))?;
            }
            std::fs::remove_dir_all(from)
        }
        Ok(_) if kind.is_dir() => std::fs::remove_dir_all(from),
        Ok(_) => std::fs::remove_file(from),
    }
}

/// Puts the database files `names` of `old` in `new`: each copied beside its place first, then renamed into it over whatever stale file is there, in order, so the database comes last. A side file `old` does not have is removed from `new`, so a stale journal is never paired with the database. `old` keeps its files until the database is in place.
fn move_database(old: &Path, new: &Path, names: &[OsString]) -> std::io::Result<()> {
    let mut copies = Vec::with_capacity(names.len());
    for name in names {
        let (from, to) = (old.join(name), new.join(name));
        if from.is_file() {
            let staging = staging_for(&to);
            std::fs::copy(&from, &staging)?;
            copies.push((staging, to));
        } else {
            remove_if_there(&to)?;
        }
    }
    for (staging, to) in copies {
        std::fs::rename(staging, to)?;
    }
    Ok(())
}

fn remove_if_there(file: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Copies the file or folder `from` to `to` (which must not exist), then deletes `from`. The copy is made under a staging name next to `to` and renamed to `to` only once complete, so `to` never holds a partial copy, even when the app is killed halfway; the next move removes such a leftover and starts over. `from` stays as it was until the copy is in place. Blocks on the disk.
///
/// # Errors
///
/// `to` exists already, a leftover staging copy cannot be removed, or a folder or file cannot be read, made, copied or renamed. A failure to delete `from` afterwards is only logged: everything is in `to` by then.
pub fn copy_then_delete(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    let staging = staging_for(to);
    remove_leftover(&staging)?;
    let is_dir = from.is_dir();
    let copied = if is_dir {
        copy_dir(from, &staging)
    } else {
        std::fs::copy(from, &staging).map(|_| ())
    };
    if let Err(e) = copied.and_then(|()| std::fs::rename(&staging, to)) {
        if let Err(cleanup) = remove_leftover(&staging) {
            tracing::warn!(copy = %staging.display(), error = %cleanup, "could not remove a copy from the data folder that was not put in place; the next move removes it");
        }
        return Err(e);
    }
    let removed = if is_dir {
        std::fs::remove_dir_all(from)
    } else {
        std::fs::remove_file(from)
    };
    if let Err(e) = removed {
        // Everything is in its new place already; the old copy is only clutter now.
        tracing::warn!(old = %from.display(), error = %e, "could not delete what was moved out of the old data folder");
    }
    Ok(())
}

/// Removes the file or folder a copy cut short left at `staging`, if any.
fn remove_leftover(staging: &Path) -> std::io::Result<()> {
    let removed = match std::fs::symlink_metadata(staging) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(there) if there.is_dir() => std::fs::remove_dir_all(staging),
        Ok(_) => std::fs::remove_file(staging),
    };
    match removed {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Where a copy of `to` is made before it takes that name: next to it, so the rename is one step on the same drive.
fn staging_for(to: &Path) -> PathBuf {
    let mut name = to.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
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
        // Links are not followed: the data folder never holds any, and one put there should not pull another folder's files along.
    }
    Ok(())
}

/// The browser endpoint this build serves, as a client finds it: its [`CHANNEL`]'s per-user
/// endpoint, named with the key [`crate::bridge::bind`] keeps.
///
/// # Errors
///
/// No key yet, because this build has not listened for this user; the current user cannot be
/// read (Windows); or the endpoint's folder cannot be created or is not private to this user
/// (Linux); see [`wirecatch_ipc::endpoint_for`].
pub fn endpoint() -> Result<Endpoint, IpcError> {
    wirecatch_ipc::endpoint_for(CHANNEL)
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

    // An unelevated copy beside an elevated one gets past the endpoint (the elevated pipe's label keeps its probe out), and the list's lock then refuses it. It stayed open as a manager that could not open its list, instead of saying that Wirecatch runs already (W4I review R2-L3).
    #[tokio::test]
    async fn a_list_another_copy_has_open_is_told_apart_from_other_failures() {
        let dir = wirecatch_test_support::private_dir("desktop-core").unwrap();
        let mut config = ManagerConfig::new(
            dir.path().join("data"),
            KnownFolders::under_home(dir.path()),
        );
        // Tests never reach the internet.
        config.addon_sources.ytdlp_releases = None;
        let first = Core::new(config.clone());
        let manager = first.opened().await.expect("the first copy opens the list");

        let second = Core::new(config);
        let Err(refused) = second.opened().await else {
            panic!("a second copy opened a list the first has open");
        };
        assert!(refused.another_copy, "{refused:?}");
        let Err(told) = second.manager().await else {
            panic!("commands reached a manager that did not open");
        };
        assert_eq!(told, refused.reason, "commands get the same reason");
        manager.shutdown().await;

        let unwritable = Unopened::from_core(&CoreError::Closed);
        assert!(!unwritable.another_copy);
    }

    /// The tests that build their expectations from `CHANNEL` only mean something if a debug
    /// build really is the dev channel.
    #[test]
    fn a_debug_build_is_the_dev_channel() {
        assert_eq!(CHANNEL == Channel::Dev, cfg!(debug_assertions));
        assert_ne!(Channel::Dev.name(), Channel::Release.name());
    }

    /// The log folder is known before the app does anything, so a start that fails (no WebView2, a second copy, a refused elevation) still leaves its reason in the file.
    #[test]
    fn the_log_folder_is_known_before_the_app_starts() {
        let chosen = Path::new("try").join("data");
        assert_eq!(
            log_folder(Some(chosen.clone().into())),
            Some(wirecatch_logging::folder_in(&chosen))
        );
        assert_eq!(log_folder(Some("".into())), log_folder(None));
        assert_eq!(log_folder(None), wirecatch_logging::channel_folder(CHANNEL));
        if std::env::var_os(wirecatch_logging::LOG_FOLDER_VARIABLE).is_none()
            && let Some(folder) = log_folder(None)
        {
            assert!(
                folder.ends_with(
                    Path::new(CHANNEL.data_folder()).join(wirecatch_logging::LOGS_FOLDER)
                ),
                "{}",
                folder.display()
            );
        }
    }

    #[test]
    fn the_data_folder_moves_only_once_and_never_over_another() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = (root.path().join("Roaming"), root.path().join("Local"));
        // Never there: nothing to do.
        assert!(!needs_move(&old, &new));
        std::fs::create_dir_all(&old).unwrap();
        assert!(needs_move(&old, &new));
        // A local folder without a download list is no reason to stay.
        std::fs::create_dir_all(new.join("logs")).unwrap();
        assert!(needs_move(&old, &new));
        // Already moved.
        std::fs::write(new.join(DATABASE_FILE), b"downloads").unwrap();
        assert!(!needs_move(&old, &new));
        // Linux: one folder for both.
        assert!(!needs_move(&old, &old));
    }

    #[test]
    fn an_old_roaming_data_folder_moves_to_the_local_one() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("Roaming").join("Wirecatch");
        let new = root.path().join("Local").join("Wirecatch");
        std::fs::create_dir_all(old.join("addons").join("media")).unwrap();
        std::fs::write(old.join(DATABASE_FILE), b"downloads").unwrap();
        std::fs::write(
            old.join("addons").join("media").join("yt-dlp.exe"),
            b"program",
        )
        .unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Moved));
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(new.join(DATABASE_FILE)).unwrap(),
            b"downloads"
        );
        assert_eq!(
            std::fs::read(new.join("addons").join("media").join("yt-dlp.exe")).unwrap(),
            b"program"
        );
        // A second start finds the new folder and leaves everything alone.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(DATABASE_FILE), b"stale").unwrap();
        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Nothing));
        assert_eq!(
            std::fs::read(new.join(DATABASE_FILE)).unwrap(),
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
        std::fs::write(old.join(DATABASE_FILE), b"downloads").unwrap();
        std::fs::write(old.join("bin").join("bridge"), b"bridge").unwrap();
        copy_then_delete(&old, &new).unwrap();
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(new.join(DATABASE_FILE)).unwrap(),
            b"downloads"
        );
        assert_eq!(
            std::fs::read(new.join("bin").join("bridge")).unwrap(),
            b"bridge"
        );
        // One entry moved into a folder that exists already, as a merge does.
        let cookies = root.path().join("cookies.txt");
        std::fs::write(&cookies, b"jar").unwrap();
        copy_then_delete(&cookies, &new.join("cookies.txt")).unwrap();
        assert!(!cookies.exists());
        assert_eq!(std::fs::read(new.join("cookies.txt")).unwrap(), b"jar");
        // A copy that cannot finish leaves the old folder as it was and nothing half done.
        let missing = root.path().join("missing");
        assert!(copy_then_delete(&missing, &root.path().join("half")).is_err());
        assert!(!root.path().join("half").exists());
    }

    /// A copy cut short (the app killed, the power lost) is never taken for the moved folder:
    /// it waits under another name, and the next move starts over from the old folder.
    #[test]
    fn a_copy_cut_short_is_started_over_and_never_used() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(DATABASE_FILE), b"downloads").unwrap();
        let staging = staging_for(&new);
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join(DATABASE_FILE), b"half").unwrap();
        std::fs::write(staging.join("stale"), b"stale").unwrap();

        copy_then_delete(&old, &new).unwrap();
        assert!(!staging.exists());
        assert!(!new.join("stale").exists());
        assert_eq!(
            std::fs::read(new.join(DATABASE_FILE)).unwrap(),
            b"downloads"
        );
    }

    /// A roaming data folder as an earlier version left it: the download list with its journal, and an add-on.
    fn old_data_folder(root: &Path) -> PathBuf {
        let old = root.join("Roaming").join("Wirecatch");
        std::fs::create_dir_all(old.join("addons").join("media")).unwrap();
        std::fs::write(old.join(DATABASE_FILE), b"downloads").unwrap();
        std::fs::write(old.join(format!("{DATABASE_FILE}-wal")), b"journal").unwrap();
        std::fs::write(
            old.join("addons").join("media").join("yt-dlp.exe"),
            b"program",
        )
        .unwrap();
        old
    }

    fn assert_moved_into(new: &Path, old: &Path) {
        assert_eq!(
            std::fs::read(new.join(DATABASE_FILE)).unwrap(),
            b"downloads"
        );
        assert_eq!(
            std::fs::read(new.join(format!("{DATABASE_FILE}-wal"))).unwrap(),
            b"journal"
        );
        assert_eq!(
            std::fs::read(new.join("addons").join("media").join("yt-dlp.exe")).unwrap(),
            b"program"
        );
        assert!(!old.exists(), "the old folder is left behind");
    }

    /// The installer runs `wirecatch-bridge install`, and a browser starts the bridge, before the upgraded app first starts: the bridge's log folder, held open by a running bridge, must not stop the download list from moving.
    #[test]
    fn the_data_folder_moves_after_the_bridge_logged_into_the_local_one() {
        let root = tempfile::tempdir().unwrap();
        let old = old_data_folder(root.path());
        let new = root.path().join("Local").join("Wirecatch");
        let logs = new.join(wirecatch_logging::LOGS_FOLDER);
        std::fs::create_dir_all(&logs).unwrap();
        let log = logs.join(format!("{}.log", wirecatch_product::BRIDGE_EXECUTABLE));
        let mut running_bridge = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        std::io::Write::write_all(&mut running_bridge, b"bridge started\n").unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Merged));
        assert_moved_into(&new, &old);
        std::io::Write::write_all(&mut running_bridge, b"still logging\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "bridge started\nstill logging\n"
        );
    }

    /// The upgraded app takes its endpoint before it moves the data folder (`setup`): its listener has made the local folder, written the endpoint key there and holds the key's lock (D41.4), which must not stop the download list from moving, nor be moved over.
    #[test]
    fn the_data_folder_moves_after_the_listener_kept_its_key_in_the_local_one() {
        let root = tempfile::tempdir().unwrap();
        let old = old_data_folder(root.path());
        let new = root.path().join("Local").join("Wirecatch");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(
            new.join("endpoint-key"),
            b"0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        let lock = std::fs::File::create(new.join("endpoint-key.lock")).unwrap();
        lock.try_lock().unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Merged));
        assert_moved_into(&new, &old);
        assert_eq!(
            std::fs::read(new.join("endpoint-key")).unwrap(),
            b"0123456789abcdef0123456789abcdef"
        );
    }

    /// Someone who uses the command-line downloader before upgrading the app.
    #[test]
    fn the_data_folder_moves_after_the_command_line_logged_into_the_local_one() {
        let root = tempfile::tempdir().unwrap();
        let old = old_data_folder(root.path());
        let new = root.path().join("Local").join("Wirecatch");
        let log = new
            .join(wirecatch_logging::LOGS_FOLDER)
            .join(format!("{}.log", wirecatch_product::CLI_EXECUTABLE));
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, b"cli ran\n").unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Merged));
        assert_moved_into(&new, &old);
        assert_eq!(std::fs::read(&log).unwrap(), b"cli ran\n");
    }

    /// A move into an existing folder cut short (the app killed, the power lost) leaves the download list where it was, and the next start finishes it: the journal copied before the cut never stands in for the one still in the old folder.
    #[test]
    fn a_move_into_an_existing_folder_cut_short_is_finished_by_the_next_start() {
        let root = tempfile::tempdir().unwrap();
        let old = old_data_folder(root.path());
        let new = root.path().join("Local").join("Wirecatch");
        // The add-on was moved, a stale journal and half a database copy were left.
        std::fs::create_dir_all(new.join("addons")).unwrap();
        std::fs::rename(
            old.join("addons").join("media"),
            new.join("addons").join("media"),
        )
        .unwrap();
        std::fs::write(new.join(format!("{DATABASE_FILE}-wal")), b"stale").unwrap();
        std::fs::write(staging_for(&new.join(DATABASE_FILE)), b"half").unwrap();

        assert_eq!(migrate(&old, &new), (new.clone(), Migration::Merged));
        assert_moved_into(&new, &old);
        assert!(!staging_for(&new.join(DATABASE_FILE)).exists());
    }

    /// An existing `to` is refused, never copied into.
    #[test]
    fn copying_never_merges_into_an_existing_folder() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(DATABASE_FILE), b"downloads").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        assert!(copy_then_delete(&old, &new).is_err());
        assert!(old.join(DATABASE_FILE).exists());
        assert!(std::fs::read_dir(&new).unwrap().next().is_none());
    }
}
