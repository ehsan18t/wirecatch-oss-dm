//! Wirecatch download manager.
//!
//! [`Manager`] owns every download, queue and setting of the desktop app: it picks
//! file names and folders, runs the engine within each queue's and the global
//! limits and schedules, keeps everything in SQLite, and reports changes as
//! [`UiEvent`](wirecatch_protocol::app::UiEvent)s for the UI and protocol
//! [`Event`](wirecatch_protocol::wire::Event)s for socket clients.
//! [`serve_connection`] speaks the local protocol (D14) over any byte stream.

mod addons;
mod clock;
mod error;
mod folders;
mod headers;
mod manager;
mod media;
mod naming;
mod owner_only;
mod profile;
mod redact;
mod schedule;
mod server;
mod settings;
mod speed;
mod store;
mod user_error;
mod zone;

pub use addons::manifest::{
    AddonComponent, AddonFile, ArchiveKind, MEDIA_ADDON_ID, MediaAddonSources, PinnedKey,
};
pub use clock::{Clock, ManualClock, Sleep, SystemClock};
pub use error::{CoreError, Result};
pub use folders::{KnownFolders, PathError};
pub use manager::{
    AddonRequestItem, DEFAULT_CAPTURE_TIMEOUT, DEFAULT_MEDIA_REFRESH_DELAYS, DEFAULT_MEDIA_TIMEOUT,
    DEFAULT_STOP_TIMEOUT, Manager, ManagerConfig, MediaReading,
};
pub use server::{MAX_REQUESTS_IN_FLIGHT, serve_connection};
pub use settings::{DEFAULT_QUEUE_ID, default_settings};
pub use store::DATABASE_FILE;
pub use user_error::StartupNotice;
