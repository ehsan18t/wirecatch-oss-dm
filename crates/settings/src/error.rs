use wirecatch_files::FileError;

use crate::folders::PathError;

/// Why a setting, a queue or a settings file was refused. The `Display` text is plain language
/// and says how to fix the problem, so the UI can show it as is.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// A value is out of range or inconsistent.
    #[error("{0}")]
    Invalid(String),

    /// There are already as many of something as there may be; the text says which and what to do.
    #[error("{0}")]
    LimitReached(String),

    /// A folder does not resolve on this machine.
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Profile(#[from] SettingsFileError),
}

/// Why a settings file could not be imported.
#[derive(Debug, thiserror::Error)]
pub enum SettingsFileError {
    #[error(
        "This settings file was made by a newer version of Wirecatch (format {found}, this version reads up to {supported}). Update Wirecatch, then import it again."
    )]
    TooNew {
        /// Format version found in the file, as written there.
        found: u64,
        /// Newest format version this build reads.
        supported: u32,
    },

    #[error(
        "This settings file was made by an earlier version of Wirecatch (format {found}), whose settings files this version cannot read. Export the settings again with this version."
    )]
    TooOld { found: u64 },

    /// A file that is not a readable settings profile.
    #[error(
        "This file is not a Wirecatch settings file ({0}). Choose a file made with Export settings."
    )]
    Invalid(String),
}

pub type Result<T, E = SettingsError> = std::result::Result<T, E>;
