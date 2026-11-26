//! Wirecatch download manager.
//!
//! [`Manager`] owns every download, queue and setting of the desktop app: it picks
//! file names and folders, runs the engine within each queue's and the global
//! limits and schedules, keeps everything in SQLite, and reports changes as
//! [`UiEvent`](wirecatch_protocol::app::UiEvent)s for the UI and protocol
//! [`Event`](wirecatch_protocol::wire::Event)s for socket clients, which `wirecatch-server` serves.

mod batches;
mod clock;
mod error;
mod headers;
mod manager;
mod naming;
mod proxy;
mod speed;

pub use clock::{Clock, ManualClock, Sleep, SystemClock};
pub use error::{CoreError, Result};
pub use manager::app_update::{AppReleases, AppUpdate, UpdateDownload};
pub use manager::page_reads::PageReaderClaim;
pub use manager::private_sessions::{PRIVATE_SESSION_GRACE, SessionClaim};
pub use manager::{
    DEFAULT_CAPTURE_TIMEOUT, DEFAULT_MEDIA_REFRESH_DELAYS, DEFAULT_MEDIA_TIMEOUT,
    DEFAULT_STOP_TIMEOUT, LinkFrom, LinkWatch, Manager, ManagerConfig, PROGRESS_TICK,
    PUBLIC_SUFFIX_LIST_URL, RemoveFiles, can_move_to_queue,
};
pub use proxy::PROXY_TEST_URL;
pub use wirecatch_addons::manifest::MediaAddonSources;
pub use wirecatch_records::activity::Happened;
pub use wirecatch_settings::SettingsFileError;
pub use wirecatch_settings::folders::{KnownFolders, PathError};
pub use wirecatch_settings::proxy::ComputerProxy;
pub use wirecatch_store::StoreError;
