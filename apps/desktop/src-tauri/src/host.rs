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
    pub fn new(config: ManagerConfig) -> Self {
        Self {
            config,
            cell: OnceCell::new(),
        }
    }

    /// The manager, opening it if nobody has yet. The error is the plain-language reason it
    /// could not open, ready to show to a person.
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

/// The per-user folder for the database: `%APPDATA%\Wirecatch` on Windows,
/// `~/.local/share/wirecatch` on Linux (`Wirecatch Dev` and `wirecatch-dev` for debug builds).
/// `base` is the platform's user data folder.
pub fn data_dir(base: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_VAR).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    data_dir_for(base, DEV_BUILD)
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
pub fn endpoint() -> Result<Endpoint, IpcError> {
    let real = wirecatch_ipc::endpoint()?;
    Ok(if DEV_BUILD { dev_endpoint(&real) } else { real })
}

/// The debug build's endpoint next to `real`: `\\.\pipe\wirecatch-app-dev-<SID>` on Windows,
/// `app-dev.sock` in the same private folder on Linux.
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
