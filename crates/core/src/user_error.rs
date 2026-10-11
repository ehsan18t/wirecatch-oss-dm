//! Turns engine errors into messages for a person, each with the fix to offer (D3, rule 4).

use std::io;
use std::path::Path;

use wirecatch_engine::Error as EngineError;
use wirecatch_protocol::app::{FixAction, UserError};

use crate::store::Recovery;

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
        EngineError::Network(e) if e.is_timeout() => fix(
            "The server took too long to answer. Try again in a few minutes.",
            FixAction::Retry,
        ),
        EngineError::Network(_) => fix(
            "Could not reach the server. Check your internet connection, then try again.",
            FixAction::Retry,
        ),
        EngineError::TooManyRedirects { .. } => fix(
            "This link keeps sending Wirecatch somewhere else and never reaches the file. Open it in your browser to check it, or refresh the link with a fresh one.",
            FixAction::RefreshLink,
        ),
        EngineError::RedirectWithoutLocation { .. } => fix(
            "The server sent a broken redirect for this link. Refresh the link with one copied from the page.",
            FixAction::RefreshLink,
        ),
        // Deliberately no fix: retrying would hit the same insecure redirect.
        EngineError::InsecureRedirect { .. } => UserError {
            message: "The site tried to switch this secure download to an unencrypted connection, where it could be read or changed on the way, so Wirecatch stopped it. Get the file from another link or ask the site to fix it.".into(),
            fix: None,
        },
        // Only the manager's own resolver refuses a file, and the reason that
        // reaches a person is that the server sent something other than the file
        // the browser saw: a fresh link from the page is the way out.
        EngineError::TargetRejected(reason) => fix(reason.clone(), FixAction::RefreshLink),
        EngineError::Internal(detail) => fix(
            format!("Something went wrong inside Wirecatch ({detail}). Try again, and please report it if it keeps happening."),
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
        EngineError::FtpReply { code, message, .. } => ftp_reply(*code, message),
        EngineError::FtpLoginRefused {
            host,
            code,
            message,
            anonymous,
            ..
        } => fix(
            if *anonymous {
                format!(
                    "The FTP server {host} does not allow anonymous downloads (it answered \"{code} {}\"). Refresh the link with one that includes your user name and password, like ftp://name:password@{host}/file.",
                    message.trim().trim_end_matches('.')
                )
            } else {
                format!(
                    "The FTP server {host} did not accept the user name or password in the link (it answered \"{code} {}\"). Refresh the link with the right ones.",
                    message.trim().trim_end_matches('.')
                )
            },
            FixAction::RefreshLink,
        ),
        EngineError::FtpTlsUnavailable { host, .. } => fix(
            format!(
                "The FTP server {host} does not offer encryption, so Wirecatch stopped rather than download without it. If an unencrypted download is acceptable, refresh the link with one that starts with ftp:// instead."
            ),
            FixAction::RefreshLink,
        ),
        EngineError::FtpProxyUnsupported { variable, .. } => fix(
            format!(
                "FTP downloads cannot go through a proxy yet, and the {variable} setting on this computer asks for one. Remove {variable} from the environment to download this link directly, then try again."
            ),
            FixAction::Retry,
        ),
        // Settings are checked when they change and brought into range when they load, so this is a fault in Wirecatch: trying again cannot help.
        EngineError::InvalidOption(option) => UserError {
            message: format!(
                "Wirecatch could not start this download because a setting it uses ({option}) is out of range. Check Settings, then add the download again."
            ),
            fix: None,
        },
        // A kind of error added to the engine after this was written.
        other => fix(
            format!("The download failed ({other}). Try again."),
            FixAction::Retry,
        ),
    }
}

/// Something the person should know about how Wirecatch started, shown once as a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupNotice {
    /// A short title.
    pub title: String,
    /// What happened and what it means for the person, in whole sentences.
    pub message: String,
}

/// The notice for a download list that had to be repaired while opening.
pub(crate) fn list_recovered(recovery: &Recovery) -> StartupNotice {
    match recovery {
        Recovery::Damaged { kept_at } => StartupNotice {
            title: "Wirecatch started a new download list".to_owned(),
            message: format!(
                "The download list was damaged, so Wirecatch set it aside and started an empty one. Files you already downloaded are still in their folders. The damaged list is kept at {}.",
                kept_at.display()
            ),
        },
        Recovery::UnreadableKept(count) => StartupNotice {
            title: "Some downloads are hidden from the list".to_owned(),
            message: format!(
                "Wirecatch could not read {count} {} in its list, perhaps saved by a newer version. {} kept for a version that can read {}, without {} sign-in. Files already downloaded are still in their folders.",
                if *count == 1 { "download" } else { "downloads" },
                if *count == 1 { "It is" } else { "They are" },
                if *count == 1 { "it" } else { "them" },
                if *count == 1 { "its" } else { "their" },
            ),
        },
    }
}

/// The message shown when saving to the download list keeps failing.
pub(crate) fn storage_failed(reason: &str) -> UserError {
    fix(
        format!(
            "Wirecatch could not save changes to the download list ({reason}). Recent changes may be lost when it closes. Make sure the disk has free space."
        ),
        FixAction::FreeDiskSpace,
    )
}

/// The message for a download whose run ended without a result (a bug).
pub(crate) fn stopped_unexpectedly() -> UserError {
    fix(
        "The download stopped unexpectedly. Try again; the part already downloaded is kept.",
        FixAction::Retry,
    )
}

/// An FTP server's error reply: 550 and friends mean the file is not there or not
/// readable, 4xx a temporary problem.
fn ftp_reply(code: u16, message: &str) -> UserError {
    let said = message.trim().trim_end_matches('.');
    let lower = said.to_ascii_lowercase();
    match code {
        550 if lower.contains("permission") || lower.contains("denied") => fix(
            format!(
                "The FTP server does not allow reading this file (\"{code} {said}\"). Refresh the link with one that includes a user name and password that may read it."
            ),
            FixAction::RefreshLink,
        ),
        550 => fix(
            format!(
                "The FTP server says this file does not exist (\"{code} {said}\"). Check the link, or refresh it with one copied from the page."
            ),
            FixAction::RefreshLink,
        ),
        400..=499 => fix(
            format!(
                "The FTP server is busy or had a temporary problem (\"{code} {said}\"). Try again in a few minutes."
            ),
            FixAction::Retry,
        ),
        _ => fix(
            format!(
                "The FTP server refused the download (\"{code} {said}\"). Try again, or refresh the link if it keeps happening."
            ),
            FixAction::Retry,
        ),
    }
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

    // It fell into the catch-all and told the person to try again, which can never help.
    #[test]
    fn an_option_out_of_range_is_not_offered_a_retry() {
        let error = EngineError::InvalidOption("connections");
        let described = describe(&error, Path::new("/downloads"), false);
        assert_eq!(described.fix, None);
        assert!(
            !described.message.contains("Try again"),
            "{}",
            described.message
        );
        assert!(
            described.message.contains("connections"),
            "{}",
            described.message
        );
    }

    #[test]
    fn ftp_replies_say_what_happened_and_offer_a_fix() {
        let missing = ftp_reply(550, "No such file or directory.");
        assert_eq!(missing.fix, Some(FixAction::RefreshLink));
        assert!(
            missing.message.contains("does not exist"),
            "{}",
            missing.message
        );
        assert!(
            missing
                .message
                .contains("\"550 No such file or directory\"")
        );
        let denied = ftp_reply(550, "Permission denied");
        assert!(denied.message.contains("does not allow reading"));
        let busy = ftp_reply(421, "Too many connections");
        assert_eq!(busy.fix, Some(FixAction::Retry));
        assert!(busy.message.contains("busy"));
        let other = ftp_reply(502, "Command not implemented");
        assert_eq!(other.fix, Some(FixAction::Retry));
        for e in [missing, denied, busy, other] {
            assert!(e.message.ends_with('.') && !e.message.contains('\u{2014}'));
        }
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

    fn insecure() -> EngineError {
        EngineError::InsecureRedirect {
            from: "https://example.com/f".into(),
            to: "http://example.com/f".into(),
        }
    }

    #[test]
    fn redirect_problems_are_explained() {
        let insecure = describe(&insecure(), Path::new("/d"), true);
        assert_eq!(
            insecure.fix, None,
            "an insecure redirect must not offer a retry"
        );
        assert!(
            insecure.message.contains("unencrypted"),
            "{}",
            insecure.message
        );
        assert_eq!(
            action(&EngineError::TooManyRedirects { url: "u".into() }, false),
            Some(FixAction::RefreshLink)
        );
        // The app refused what the server sent (another file than the browser
        // saw, or a sign-in page): a fresh link from the page is the way out.
        let rejected = describe(
            &EngineError::TargetRejected(
                "The server sent a web page instead of the file. Refresh the link.".into(),
            ),
            Path::new("/d"),
            false,
        );
        assert!(rejected.message.starts_with("The server sent a web page"));
        assert_eq!(rejected.fix, Some(FixAction::RefreshLink));
        assert_eq!(
            action(&EngineError::Internal("bug".into()), false),
            Some(FixAction::Retry)
        );
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
            EngineError::TooManyRedirects { url: "u".into() },
            EngineError::RedirectWithoutLocation {
                status: 302,
                url: "u".into(),
            },
            insecure(),
            EngineError::Internal("x".into()),
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
