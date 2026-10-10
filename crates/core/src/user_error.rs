//! Turns engine errors into messages for a person, each with the fix to offer (D3, rule 4).

use std::io;
use std::path::Path;

use wirecatch_engine::Error as EngineError;
use wirecatch_protocol::app::{FixAction, UserError};

/// Raw OS codes for a full disk, in case the standard library does not classify them.
#[cfg(windows)]
const DISK_FULL_CODES: &[i32] = &[39, 112]; // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL
#[cfg(unix)]
const DISK_FULL_CODES: &[i32] = &[28, 122]; // ENOSPC, EDQUOT
#[cfg(not(any(windows, unix)))]
const DISK_FULL_CODES: &[i32] = &[];

/// Describes why a download failed.
///
/// `folder` is where the file is saved; `previously_worked` tells whether the link
/// delivered data before, which turns a refusal into an expired link.
pub(crate) fn describe(error: &EngineError, folder: &Path, previously_worked: bool) -> UserError {
    match error {
        EngineError::HttpStatus { status, .. } => http_status(*status, previously_worked),
        EngineError::InvalidUrl(_) => fix(
            "This link is not a valid web address. Refresh the link with one copied from the page.",
            FixAction::RefreshLink,
        ),
        EngineError::Network(_) => fix(
            "Could not reach the server. Check your internet connection, then try again.",
            FixAction::Retry,
        ),
        EngineError::Stalled(secs) => fix(
            format!(
                "The server stopped sending data for {secs} seconds. Try again; the part already downloaded is kept."
            ),
            FixAction::Retry,
        ),
        EngineError::PrematureEof { .. } => fix(
            "The connection closed before the file was finished. Try again; the part already downloaded is kept.",
            FixAction::Retry,
        ),
        EngineError::RangeNotHonored { .. } => fix(
            "The server stopped allowing downloads to continue where they left off. Try again to restart the download.",
            FixAction::Retry,
        ),
        EngineError::RemoteChanged => fix(
            "The file changed on the server while it was downloading. Try again to download the new version.",
            FixAction::Retry,
        ),
        EngineError::Io(e) => disk(e, folder),
        EngineError::State(_) => fix(
            "The saved progress of this download is damaged. Try again to start it over.",
            FixAction::Retry,
        ),
        EngineError::Cancelled => fix(
            "The download stopped before it began. Try again.",
            FixAction::Retry,
        ),
    }
}

/// The message for a download whose run ended without a result (a bug).
pub(crate) fn stopped_unexpectedly() -> UserError {
    fix(
        "The download stopped unexpectedly. Try again; the part already downloaded is kept.",
        FixAction::Retry,
    )
}

fn http_status(status: u16, previously_worked: bool) -> UserError {
    match status {
        401 | 403 | 404 | 410 if previously_worked => fix(
            format!(
                "The download link has expired (the server answered {status}). Open the page again and refresh the link; the part already downloaded is kept."
            ),
            FixAction::RefreshLink,
        ),
        401 | 403 => fix(
            format!(
                "The server refused access to this file (error {status}). You may need to sign in on the page first. Then refresh the link."
            ),
            FixAction::RefreshLink,
        ),
        404 | 410 => fix(
            format!(
                "The server says this file does not exist (error {status}). Check the link, or refresh it with one copied from the page."
            ),
            FixAction::RefreshLink,
        ),
        408 | 425 | 429 | 500..=599 => fix(
            format!(
                "The server is busy or having trouble (error {status}). Try again in a few minutes."
            ),
            FixAction::Retry,
        ),
        _ => fix(
            format!(
                "The server rejected the download (error {status}). Try again, or refresh the link if it keeps happening."
            ),
            FixAction::Retry,
        ),
    }
}

fn disk(error: &io::Error, folder: &Path) -> UserError {
    let folder = folder.display();
    let disk_full = matches!(
        error.kind(),
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded
    ) || error
        .raw_os_error()
        .is_some_and(|code| DISK_FULL_CODES.contains(&code));
    if disk_full {
        return fix(
            format!(
                "The disk is full. Free up space on the drive that holds {folder}, then try again."
            ),
            FixAction::FreeDiskSpace,
        );
    }
    match error.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => fix(
            format!("Wirecatch is not allowed to save files in {folder}. Choose another folder."),
            FixAction::ChooseFolder,
        ),
        // A file standing where a folder must be created shows up as AlreadyExists.
        io::ErrorKind::NotFound
        | io::ErrorKind::NotADirectory
        | io::ErrorKind::AlreadyExists
        | io::ErrorKind::InvalidFilename => fix(
            format!("The folder {folder} cannot be used. Choose another folder."),
            FixAction::ChooseFolder,
        ),
        _ => fix(
            format!(
                "Could not write the file to {folder} ({error}). Check the drive, then try again."
            ),
            FixAction::Retry,
        ),
    }
}

fn fix(message: impl Into<String>, action: FixAction) -> UserError {
    UserError {
        message: message.into(),
        fix: Some(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> EngineError {
        EngineError::HttpStatus {
            status: code,
            url: "https://example.com/f".into(),
        }
    }

    fn action(error: &EngineError, worked: bool) -> Option<FixAction> {
        describe(error, Path::new("/downloads"), worked).fix
    }

    #[test]
    fn refused_link_that_worked_before_is_expired() {
        for code in [403, 404, 410] {
            let e = describe(&status(code), Path::new("/d"), true);
            assert_eq!(e.fix, Some(FixAction::RefreshLink));
            assert!(e.message.contains("expired"), "{}", e.message);
        }
        assert!(
            !describe(&status(404), Path::new("/d"), false)
                .message
                .contains("expired")
        );
    }

    #[test]
    fn server_trouble_and_network_problems_offer_retry() {
        assert_eq!(action(&status(503), false), Some(FixAction::Retry));
        assert_eq!(action(&status(429), true), Some(FixAction::Retry));
        assert_eq!(
            action(&EngineError::Stalled(30), true),
            Some(FixAction::Retry)
        );
        assert_eq!(
            action(&EngineError::RemoteChanged, true),
            Some(FixAction::Retry)
        );
        assert_eq!(
            action(&EngineError::PrematureEof { at: 1, end: 2 }, true),
            Some(FixAction::Retry)
        );
    }

    #[test]
    fn full_disk_asks_for_space() {
        let kind = EngineError::Io(io::ErrorKind::StorageFull.into());
        assert_eq!(action(&kind, true), Some(FixAction::FreeDiskSpace));
        let raw = EngineError::Io(io::Error::from_raw_os_error(DISK_FULL_CODES[0]));
        assert_eq!(action(&raw, true), Some(FixAction::FreeDiskSpace));
    }

    #[test]
    fn forbidden_folder_asks_for_another() {
        let e = describe(
            &EngineError::Io(io::ErrorKind::PermissionDenied.into()),
            Path::new("/locked"),
            false,
        );
        assert_eq!(e.fix, Some(FixAction::ChooseFolder));
        assert!(e.message.contains("locked"));
    }

    #[test]
    fn messages_follow_the_house_style() {
        let errors = [
            status(403),
            status(404),
            status(500),
            status(400),
            EngineError::InvalidUrl("x".into()),
            EngineError::Stalled(30),
            EngineError::RangeNotHonored {
                expected: 1,
                got: None,
            },
            EngineError::State("x".into()),
            EngineError::Cancelled,
            EngineError::Io(io::ErrorKind::Other.into()),
        ];
        for e in &errors {
            for worked in [false, true] {
                let m = describe(e, Path::new("/d"), worked).message;
                assert!(!m.contains('\u{2014}'), "em dash in {m}");
                assert!(m.ends_with('.'), "{m}");
            }
        }
    }
}
