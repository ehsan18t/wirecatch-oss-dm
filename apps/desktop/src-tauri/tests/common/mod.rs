//! What every test binary here shares: a manager in a temporary folder. The local HTTP server
//! the command tests download from is the shared one in `crates/test-support`.

use tempfile::TempDir;
use wirecatch_core::{KnownFolders, Manager, ManagerConfig};
use wirecatch_test_support::TestResult;

/// A temporary home folder and data folder for a manager.
pub struct Sandbox {
    pub dir: TempDir,
}

impl Sandbox {
    pub fn new() -> TestResult<Self> {
        Ok(Self {
            dir: tempfile::tempdir()?,
        })
    }

    /// The manager's configuration: its data in `data`, the person's folders under `home`.
    pub fn config(&self) -> TestResult<ManagerConfig> {
        let home = self.dir.path().join("home");
        std::fs::create_dir_all(&home)?;
        Ok(ManagerConfig::new(
            self.dir.path().join("data"),
            KnownFolders::under_home(&home),
        ))
    }

    pub async fn open(&self) -> TestResult<Manager> {
        Ok(Manager::open(self.config()?).await?)
    }
}
