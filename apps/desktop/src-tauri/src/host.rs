//! Opens the download manager once and hands it to everything that needs it (D13).
//!
//! The manager opens in the background while the window appears, so the window never waits on
//! the database. Commands that arrive first simply wait for it.

use std::path::{Path, PathBuf};

use tokio::sync::OnceCell;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};

/// Overrides the data folder. For development and tests only: it lets a debug build run next to
/// an installed Wirecatch without sharing its download list.
pub const DATA_DIR_VAR: &str = "WIRECATCH_DATA_DIR";

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
/// `~/.local/share/wirecatch` on Linux. `base` is the platform's user data folder.
pub fn data_dir(base: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_VAR).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    base.join(if cfg!(windows) {
        "Wirecatch"
    } else {
        "wirecatch"
    })
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
    fn the_data_folder_is_named_after_the_app() {
        // The override is only read when set; tests never set it.
        if std::env::var_os(DATA_DIR_VAR).is_some() {
            return;
        }
        let dir = data_dir(Path::new("base"));
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        assert_eq!(name.to_lowercase(), "wirecatch");
        assert!(dir.starts_with("base"));
    }
}
