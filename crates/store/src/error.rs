use wirecatch_files::FileError;
use wirecatch_product::PRODUCT_NAME;

/// Why the download list could not be opened or saved. The `Display` text is plain language and
/// says how to fix the problem, so the UI can show it as is.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Another manager has the download list open: another copy of Wirecatch, which may run
    /// with other rights than this one.
    #[error(
        "Another copy of {app} has the download list in {path} open. Close it, or keep using that copy.", app = PRODUCT_NAME
    )]
    AlreadyOpen {
        /// The folder that holds the list.
        path: String,
    },

    /// The database was written by a newer Wirecatch.
    #[error(
        "The download list was saved by a newer version of {app} (database version {found}, this version reads up to {supported}). Update {app} to open it.", app = PRODUCT_NAME
    )]
    TooNew {
        /// Schema version found in the database.
        found: u32,
        /// Newest schema version this build reads.
        supported: u32,
    },

    /// The database could not be read or written.
    #[error(
        "The download list could not be read or saved ({0}). Check that {app}'s data folder can be written to and the disk is not full, then restart {app}.", app = PRODUCT_NAME
    )]
    Database(#[from] rusqlite::Error),

    /// A file of the download list could not be read, written or moved.
    #[error(transparent)]
    File(#[from] FileError),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use std::io;

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
            StoreError::File(FileError {
                path: "/x".into(),
                source: io::Error::from(io::ErrorKind::PermissionDenied),
            }),
        ];
        for error in &errors {
            let message = error.to_string();
            assert!(!message.contains('\u{2014}'), "em dash in {message}");
            assert!(message.ends_with('.'), "{message}");
            assert!(message.starts_with(char::is_uppercase), "{message}");
        }
    }
}
