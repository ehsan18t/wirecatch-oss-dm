//! Errors returned by manager operations. Messages are written for the user.

use std::io;

use wirecatch_protocol::wire::{ErrorCode, ProtocolError};
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};

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
    #[error("That download was already confirmed or dismissed, or the browser took it back.")]
    ConfirmationNotFound(QuestionId),

    /// A link that is not an http, https, ftp, ftps or ftpes URL.
    #[error(
        "\"{0}\" is not a valid link. Check that it starts with http://, https://, ftp:// or ftps://."
    )]
    InvalidUrl(String),

    /// The choice needs the media add-on, which is not installed. The UI was
    /// told with `UiEvent::AddonRequired`; clients see `ErrorCode::AddonRequired`.
    #[error(
        "This needs the media add-on, which joins video and audio and reads video pages. Install it from Settings, Add-ons (it is offered right away), then try again."
    )]
    AddonRequired,

    /// A value the user or a client sent is out of range or inconsistent.
    #[error("{0}")]
    Invalid(String),

    /// The app already holds as many of something as it may (downloads waiting for an answer,
    /// queues); the text says which and what to do.
    #[error("{0}")]
    LimitReached(String),

    /// The operation does not apply to the download or add-on in its current state.
    #[error("{0}")]
    WrongState(&'static str),

    /// An add-on this build of Wirecatch does not know.
    #[error(
        "There is no such add-on in this version of Wirecatch. Update Wirecatch if this add-on is new."
    )]
    UnknownAddon,

    /// Looking for a newer add-on failed: its releases could not be reached, or a newer
    /// release's checksum list is not signed by its developers. The text says which.
    #[error("{0}")]
    UpdateCheckFailed(String),

    /// An add-on install was stopped, but its files could not all be deleted.
    #[error(
        "The install was stopped, but some of its files could not be deleted ({source}). Remove the media add-on in Settings, Add-ons to clean up."
    )]
    AddonFilesLeft {
        /// What the operating system reported.
        source: io::Error,
    },

    /// An add-on's files could not all be deleted when it was removed.
    #[error(
        "The media add-on could not be removed completely ({source}). Close any program that may be using its files, then remove it again."
    )]
    AddonNotRemoved {
        /// What the operating system reported.
        source: io::Error,
    },

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
        /// Format version found in the file, as written there.
        found: u64,
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

    /// Moving a download's files to another folder failed, and some of the files
    /// already moved could not be put back, so they are split between the two
    /// folders.
    #[error(
        "Could not move {path} ({source}), and some of this download's files could not be moved back: {stuck}. Move each one back by hand, under the name given, then try the download again."
    )]
    FilesSplit {
        /// The file that could not be moved.
        path: String,
        /// What the operating system reported.
        source: io::Error,
        /// Each file left in the new folder and the path it must have to be
        /// found again, in words.
        stuck: String,
    },

    /// A captured download could not be taken over; the text says why, in one or
    /// more whole sentences.
    #[error("Wirecatch could not take over this download. {0} The browser keeps it.")]
    NotStarted(String),

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

/// Runs blocking work (disk access) on `runtime`'s blocking thread pool.
pub(crate) async fn blocking<T: Send + 'static>(
    runtime: &tokio::runtime::Handle,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    runtime
        .spawn_blocking(work)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
}

impl CoreError {
    /// The same error as sent to a protocol client.
    #[must_use]
    pub fn to_protocol(&self) -> ProtocolError {
        let code = match self {
            CoreError::DownloadNotFound(_)
            | CoreError::QueueNotFound(_)
            | CoreError::ConfirmationNotFound(_)
            | CoreError::UnknownAddon => ErrorCode::NotFound,
            CoreError::InvalidUrl(_) => ErrorCode::InvalidUrl,
            CoreError::NotStarted(_) => ErrorCode::NotStarted,
            CoreError::LimitReached(_) => ErrorCode::LimitReached,
            CoreError::WrongState(_) => ErrorCode::WrongState,
            CoreError::UpdateCheckFailed(_) => ErrorCode::UpdateCheckFailed,
            CoreError::Invalid(_)
            | CoreError::DefaultQueue
            | CoreError::Path(_)
            | CoreError::ProfileTooNew { .. }
            | CoreError::InvalidProfile(_) => ErrorCode::InvalidRequest,
            CoreError::AddonRequired => ErrorCode::AddonRequired,
            CoreError::DatabaseTooNew { .. }
            | CoreError::Database(_)
            | CoreError::File { .. }
            | CoreError::FilesSplit { .. }
            | CoreError::AddonFilesLeft { .. }
            | CoreError::AddonNotRemoved { .. }
            | CoreError::Closed
            | CoreError::Internal(_) => ErrorCode::Internal,
        };
        ProtocolError {
            code,
            message: self.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_that_was_not_taken_over_reads_as_whole_sentences() {
        let error = CoreError::NotStarted("The server did not answer within 8 seconds.".into());
        assert_eq!(
            error.to_string(),
            "Wirecatch could not take over this download. The server did not answer within 8 seconds. The browser keeps it."
        );
    }

    /// A client tells "this needs the media add-on" from other refusals by its
    /// code, never by its wording.
    #[test]
    fn a_missing_add_on_has_its_own_code() {
        assert_eq!(
            CoreError::AddonRequired.to_protocol().code,
            ErrorCode::AddonRequired
        );
    }

    /// A client acts on what went wrong by the code, so a request it got wrong, a limit, a
    /// state the call does not apply to and a failed update check each have their own.
    #[test]
    fn each_cause_a_client_acts_on_has_its_own_code() {
        let code = |error: CoreError| error.to_protocol().code;
        assert_eq!(
            code(CoreError::Invalid("x".into())),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(CoreError::LimitReached("x".into())),
            ErrorCode::LimitReached
        );
        assert_eq!(code(CoreError::WrongState("x")), ErrorCode::WrongState);
        assert_eq!(
            code(CoreError::UpdateCheckFailed("x".into())),
            ErrorCode::UpdateCheckFailed
        );
        assert_eq!(code(CoreError::UnknownAddon), ErrorCode::NotFound);
        let disk = || io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(
            code(CoreError::AddonNotRemoved { source: disk() }),
            ErrorCode::Internal
        );
        assert_eq!(
            code(CoreError::AddonFilesLeft { source: disk() }),
            ErrorCode::Internal
        );
    }

    #[test]
    fn messages_follow_the_house_style() {
        let io = || io::Error::from(io::ErrorKind::PermissionDenied);
        let errors = [
            CoreError::DownloadNotFound(DownloadId(1)),
            CoreError::QueueNotFound(QueueId(1)),
            CoreError::ConfirmationNotFound(QuestionId(1)),
            CoreError::InvalidUrl("ftp://x".into()),
            CoreError::DefaultQueue,
            CoreError::Path(PathError::Special(r"\\.\C:".into())),
            CoreError::Path(PathError::Network(r"\\server\share".into())),
            CoreError::ProfileTooNew {
                found: 2,
                supported: 1,
            },
            CoreError::InvalidProfile("it has no format version".into()),
            CoreError::DatabaseTooNew {
                found: 9,
                supported: 3,
            },
            CoreError::File {
                path: "/x".into(),
                source: io(),
            },
            CoreError::FilesSplit {
                path: "/x".into(),
                source: io(),
                stuck: "/new/a (1).bin.part (it belongs at /old/a.bin.part)".into(),
            },
            CoreError::NotStarted("It was removed.".into()),
            CoreError::AddonRequired,
            CoreError::LimitReached("There can be at most 100 queues. Delete one first.".into()),
            CoreError::UnknownAddon,
            CoreError::UpdateCheckFailed("The releases could not be reached.".into()),
            CoreError::AddonFilesLeft { source: io() },
            CoreError::AddonNotRemoved { source: io() },
            CoreError::Closed,
            CoreError::Internal("a bug".into()),
        ];
        for error in &errors {
            let message = error.to_string();
            assert!(!message.contains('\u{2014}'), "em dash in {message}");
            assert!(message.ends_with('.'), "{message}");
            assert!(
                message.starts_with(|c: char| c.is_uppercase() || c == '"'),
                "{message}"
            );
        }
    }
}
