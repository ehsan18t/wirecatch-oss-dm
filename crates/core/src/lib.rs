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
mod folders;
mod headers;
mod manager;
mod media;
mod naming;
mod profile;
mod proxy;
mod settings;
mod speed;

pub use clock::{Clock, ManualClock, Sleep, SystemClock};
pub use error::{CoreError, Result};
pub use folders::{KnownFolders, PathError};
pub use manager::{
    DEFAULT_CAPTURE_TIMEOUT, DEFAULT_MEDIA_REFRESH_DELAYS, DEFAULT_MEDIA_TIMEOUT,
    DEFAULT_STOP_TIMEOUT, LinkFrom, Manager, ManagerConfig, PUBLIC_SUFFIX_LIST_URL,
};
pub use proxy::{PROXY_TEST_URL, UpdaterProxy, from_system as system_proxy};
pub use settings::{DEFAULT_QUEUE_ID, default_settings};
pub use wirecatch_addons::manifest::{
    AddonComponent, AddonFile, ArchiveKind, ComponentId, Fingerprint, MEDIA_ADDON_FOLDER,
    ManifestError, MediaAddonSources, PinnedKey, ReleaseFile, Sha256Digest,
};
pub use wirecatch_store::{DATABASE_FILE, DATABASE_SIDE_FILES, StoreError};
