use std::io;

/// Why the download list could not be opened or saved. The `Display` text is plain language and
/// says how to fix the problem, so the UI can show it as is.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Another manager has the download list open: another copy of Wirecatch, which may run
    /// with other rights than this one.
    #[error(
        "Another copy of Wirecatch has the download list in {path} open. Close it, or keep using that copy."
    )]
    AlreadyOpen {
        /// The folder that holds the list.
        path: String,
    },

    /// The database was written by a newer Wirecatch.
    #[error(
        "The download list was saved by a newer version of Wirecatch (database version {found}, this version reads up to {supported}). Update Wirecatch to open it."
    )]
    TooNew {
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

    /// A file of the download list could not be read, written or moved.
    #[error(
        "Could not use {path} ({source}). Check that the folder exists and that you are allowed to write there."
    )]
    File {
        /// The file involved.
        path: String,
        /// What the operating system reported.
        source: io::Error,
    },
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_follow_the_house_style() {
        let errors = [
            StoreError::AlreadyOpen { path: "/x".into() },
            StoreError::TooNew {
                found: 9,
                supported: 3,
            },
            StoreError::Database(rusqlite::Error::InvalidQuery),
            StoreError::File {
                path: "/x".into(),
                source: io::Error::from(io::ErrorKind::PermissionDenied),
            },
        ];
        for error in &errors {
            let message = error.to_string();
            assert!(!message.contains('\u{2014}'), "em dash in {message}");
            assert!(message.ends_with('.'), "{message}");
            assert!(message.starts_with(char::is_uppercase), "{message}");
        }
    }
}
