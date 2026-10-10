//! Wirecatch download manager.
//!
//! [`Manager`] owns every download, queue and setting of the desktop app: it picks
//! file names and folders, runs the engine within each queue's and the global
//! limits and schedules, keeps everything in SQLite, and reports changes as
//! [`UiEvent`](wirecatch_protocol::app::UiEvent)s for the UI and protocol
//! [`Event`](wirecatch_protocol::wire::Event)s for socket clients.
//! [`serve_connection`] speaks the local protocol (D14) over any byte stream.

#![warn(missing_docs)]

mod clock;
mod error;
mod folders;
mod headers;
mod manager;
mod naming;
mod owner_only;
mod profile;
mod schedule;
mod server;
mod settings;
mod speed;
mod store;
mod user_error;

pub use clock::{Clock, ManualClock, Sleep, SystemClock};
pub use error::{CoreError, Result};
pub use folders::{KnownFolders, PathError};
pub use manager::{DEFAULT_CAPTURE_TIMEOUT, DEFAULT_STOP_TIMEOUT, Manager, ManagerConfig};
pub use profile::PROFILE_FORMAT_VERSION;
pub use server::serve_connection;
pub use settings::{DEFAULT_QUEUE_ID, default_settings};
pub use store::DATABASE_FILE;
