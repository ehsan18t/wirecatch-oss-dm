//! Errors returned by manager operations. Messages are written for the user.

use std::io;

use wirecatch_protocol::wire::{ErrorCode, ProtocolError};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::folders::PathError;

/// Why a manager operation failed. The `Display` text is plain language and says
/// how to fix the problem, so the UI can show it as is.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The download does not exist (any more).
    #[error("That download no longer exists. It may have been removed.")]
    DownloadNotFound(DownloadId),

    /// The queue does not exist (any more).
    #[error("That queue no longer exists. It may have been deleted.")]
    QueueNotFound(QueueId),

    /// The confirmation was already answered or never existed.
    #[error("That download was already confirmed or dismissed.")]
    ConfirmationNotFound(u64),

    /// A link that is not an http or https URL.
    #[error("\"{0}\" is not a valid web link. Check that it starts with http:// or https://.")]
    InvalidUrl(String),

    /// A value the user entered is out of range or inconsistent.
    #[error("{0}")]
    Invalid(String),

    /// The operation does not apply to the download in its current state.
    #[error("{0}")]
    WrongState(&'static str),

    /// The default queue is permanent.
    #[error("The default queue cannot be deleted. You can rename it or change its limits instead.")]
    DefaultQueue,

    /// A folder in the settings does not resolve on this machine.
    #[error(transparent)]
    Path(#[from] PathError),

    /// A settings file written by a newer Wirecatch.
    #[error(
        "This settings file was made by a newer version of Wirecatch (format {found}, this version reads up to {supported}). Update Wirecatch, then import it again."
    )]
    ProfileTooNew {
        /// Format version found in the file.
        found: u32,
        /// Newest format version this build reads.
        supported: u32,
    },

    /// A file that is not a readable settings profile.
    #[error(
        "This file is not a Wirecatch settings file ({0}). Choose a file made with Export settings."
    )]
    InvalidProfile(String),

    /// The database was written by a newer Wirecatch.
    #[error(
        "The download list was saved by a newer version of Wirecatch (database version {found}, this version reads up to {supported}). Update Wirecatch to open it."
    )]
    DatabaseTooNew {
        /// Schema version found in the database.
        found: u32,
        /// Newest schema version this build reads.
        supported: u32,
    },

    /// The database could not be read or written.
    #[error(
        "The download list could not be read or saved ({0}). Check that Wirecatch's data folder can be written to and the disk is not full, then restart Wirecatch."
    )]
    Database(#[from] rusqlite::Error),

    /// A file the user named could not be read or written.
    #[error(
        "Could not use {path} ({source}). Check that the folder exists and that you are allowed to write there."
    )]
    File {
        /// The file involved.
        path: String,
        /// What the operating system reported.
        source: io::Error,
    },

    /// The manager has shut down.
    #[error("Wirecatch is shutting down. Start it again to continue.")]
    Closed,

    /// A bug: something that should not fail did.
    #[error(
        "Something went wrong inside Wirecatch ({0}). Try again, and please report it if it keeps happening."
    )]
    Internal(String),
}

/// Result of a manager operation.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Runs blocking work (disk access) on the blocking thread pool.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
}

impl CoreError {
    /// The same error as sent to a protocol client.
    pub fn to_protocol(&self) -> ProtocolError {
        let code = match self {
            CoreError::DownloadNotFound(_)
            | CoreError::QueueNotFound(_)
            | CoreError::ConfirmationNotFound(_) => ErrorCode::NotFound,
            CoreError::InvalidUrl(_) => ErrorCode::InvalidUrl,
            CoreError::Invalid(_)
            | CoreError::WrongState(_)
            | CoreError::DefaultQueue
            | CoreError::Path(_)
            | CoreError::ProfileTooNew { .. }
            | CoreError::InvalidProfile(_) => ErrorCode::InvalidRequest,
            CoreError::DatabaseTooNew { .. }
            | CoreError::Database(_)
            | CoreError::File { .. }
            | CoreError::Closed
            | CoreError::Internal(_) => ErrorCode::Internal,
        };
        ProtocolError {
            code,
            message: self.to_string(),
        }
    }
}
