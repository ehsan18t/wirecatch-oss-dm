//! Turns engine errors into messages for a person, each with the fix to offer (D3, rule 4).

use std::io;
use std::path::Path;

use wirecatch_engine::{Error as EngineError, IoAction, ProxySetting};
use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_sites::local_network::ChosenBy;

/// Raw OS codes for a full disk, in case the standard library does not classify them.
#[cfg(windows)]
const DISK_FULL_CODES: &[i32] = &[39, 112]; // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL
#[cfg(unix)]
const DISK_FULL_CODES: &[i32] = &[28, 122]; // ENOSPC, EDQUOT
#[cfg(not(any(windows, unix)))]
const DISK_FULL_CODES: &[i32] = &[];

/// Whether the link delivered data before it failed, which turns a refusal into an expired link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Worked {
    Before,
    Never,
}

fn network_problem(error: &wirecatch_engine::NetworkError) -> UserError {
    let message = if error.is_timeout() {
        "The server took too long to answer. Try again in a few minutes."
    } else {
        "Could not reach the server. Check your internet connection, then try again."
    };
    fix(message, FixAction::Retry)
}

/// Describes why a download failed, saved in `folder`.
#[must_use]
pub fn describe(error: &EngineError, folder: &Path, worked: Worked) -> UserError {
    match error {
        EngineError::HttpStatus { status, .. } => http_status(*status, worked),
        EngineError::InvalidUrl(_) => fix(
            "This link is not a valid web address. Refresh the link with one copied from the page.",
            FixAction::RefreshLink,
        ),
        EngineError::Network(e) => network_problem(e),
        EngineError::Certificate { .. } => fix(
            "This site's security certificate could not be verified, so Wirecatch did not download from it. Check that your computer's date and time are right. If they are, the site needs to fix its certificate.",
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
        EngineError::TargetRejected(reason) => fix(reason.message(), FixAction::RefreshLink),
        // Who chose the link decides the words and the fix: a download's run
        // uses `private_network`; this is for requests no one link stands for.
        EngineError::PrivateNetwork { .. } => UserError {
            message: "Wirecatch did not follow this address to a device on your own network, which it blocks for safety.".into(),
            fix: None,
        },
        EngineError::Internal(detail) => fix(
            format!("Something went wrong inside Wirecatch ({detail}). Try again, and please report it if it keeps happening."),
            FixAction::Retry,
        ),
        EngineError::Stalled { after } => fix(
            format!(
                "The server stopped sending data for {}. Try again; the part already downloaded is kept.",
                how_long(*after)
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
        EngineError::Io {
            action,
            path,
            source,
        } => io_failure(*action, path, source, folder),
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
        } => ftp_login_refused(host, *code, message, *anonymous),
        EngineError::FtpTlsUnavailable { host, .. } => ftp_tls_unavailable(host),
        EngineError::FtpProxyUnsupported { setting, .. } => ftp_proxy_unsupported(setting),
        EngineError::ProxyLoginRefused => proxy_login_refused(),
        EngineError::InvalidOption(option) => invalid_option(option),
        EngineError::TooLarge { .. } => too_large(),
        // A kind of error added to the engine after this was written.
        other => fix(
            format!("The download failed ({other}). Try again."),
            FixAction::Retry,
        ),
    }
}

/// The same answer would come again, so only another link can help.
fn too_large() -> UserError {
    fix(
        "The server sent far more than expected for this address, so Wirecatch stopped reading it. Refresh the link with one copied from the page.",
        FixAction::RefreshLink,
    )
}

/// What `action` on `path` failing with `source` comes to, while saving into `folder`.
fn io_failure(action: IoAction, path: &Path, source: &io::Error, folder: &Path) -> UserError {
    let existing = matches!(
        action,
        IoAction::OpenFile
            | IoAction::Write
            | IoAction::Resize
            | IoAction::Flush
            | IoAction::Finish
    )
    .then_some(path);
    let what = format!("{action} ({})", path.display());
    disk_failure(source, folder, &what, existing)
}

/// A download's saved link that no longer parses.
#[must_use]
pub fn invalid_link() -> UserError {
    describe(
        &EngineError::InvalidUrl(String::new()),
        Path::new(""),
        Worked::Never,
    )
}

/// Why a file the app fetches for itself (an add-on, an update) failed. No row shows it, so nothing here points at a link to refresh, a folder to choose or a queue to change.
#[must_use]
pub fn describe_app_file(error: &EngineError, folder: &Path) -> String {
    const CANNOT_SEND: &str = "Try again later, and please report it if it keeps happening.";
    match error {
        EngineError::HttpStatus { status, .. } if !matches!(status, 408 | 425 | 429 | 500..=599) => {
            format!("The server refused to send this file (error {status}). {CANNOT_SEND}")
        }
        EngineError::InvalidUrl(_)
        | EngineError::TooManyRedirects { .. }
        | EngineError::RedirectWithoutLocation { .. }
        | EngineError::InsecureRedirect { .. }
        | EngineError::TooLarge { .. } => {
            format!("The server did not send this file properly. {CANNOT_SEND}")
        }
        EngineError::TargetRejected(reason) => reason.message().to_owned(),
        EngineError::ProxyLoginRefused => "The proxy asked for a user name and password, or did not accept the ones it was given. Check them in Settings > Network.".to_owned(),
        other => {
            let told = describe(other, folder, Worked::Never);
            if told.fix == Some(FixAction::ChooseFolder) {
                format!(
                    "Wirecatch cannot save files in {}. Check that you may write there and that its drive is connected, then try again.",
                    folder.display()
                )
            } else {
                told.message
            }
        }
    }
}

/// Why a link was refused for leading to a device on the person's own network,
/// in the words for whoever chose it (D34.5, D39).
#[must_use]
pub fn private_network(chosen_by: ChosenBy) -> UserError {
    match chosen_by {
        // Pasting the same link in the main window makes it the person's own, which
        // may reach their network: the one way out, and it does not loop. A link
        // given in a download's own window keeps the website's rule (D43.5).
        ChosenBy::Website => UserError {
            message: "This link leads to a device on your own network, and Wirecatch does not follow a link a website gave it there. If you trust this link, give the download a new link in Wirecatch's main window and paste the same one: a link you paste there yourself may reach your own network.".into(),
            fix: Some(FixAction::RefreshLink),
        },
        // The link itself was allowed; somewhere on the internet it was sent
        // on into the local network, which pasting it again would not change.
        ChosenBy::Person | ChosenBy::LocalPage => UserError {
            message: "This link sent Wirecatch on from the internet to a device on your own network, which Wirecatch does not follow. Use the device's own address on your network instead.".into(),
            fix: None,
        },
    }
}

/// [`describe`] for a download's own run, whose link `chosen_by` chose and which went through
/// the proxy `through_proxy` names, if one.
#[must_use]
pub fn describe_run(
    error: &EngineError,
    folder: &Path,
    worked: Worked,
    chosen_by: ChosenBy,
    through_proxy: Option<&str>,
) -> UserError {
    match (error, through_proxy) {
        (EngineError::PrivateNetwork { .. }, _) => private_network(chosen_by),
        // Through a proxy, the first connection is the proxy's (D73.4).
        (EngineError::Network(e), Some(proxy)) if e.is_connect() => fix(
            format!(
                "Could not reach the proxy {proxy}. Move the download to a queue with another proxy, or check the proxy in Settings > Network."
            ),
            FixAction::ChangeQueue,
        ),
        // The proxy's own certificate, or one it put in the site's place.
        (EngineError::Certificate { .. }, Some(proxy)) => fix(
            format!(
                "A security certificate on this connection could not be verified, and the proxy {proxy} may be the cause. Move the download to a queue with another proxy, or check the proxy in Settings > Network."
            ),
            FixAction::ChangeQueue,
        ),
        (other, _) => describe(other, folder, worked),
    }
}

/// Why a website's link to the local network is not taken over, for the
/// browser's notice: there is no download in the app to act on.
pub const CAPTURE_TO_OWN_NETWORK: &str = "It leads to a device on your own network, which Wirecatch does not reach for a link a website gave.";

/// The message shown when saving to the download list keeps failing.
#[must_use]
pub fn storage_failed(reason: &str) -> UserError {
    fix(
        format!(
            "Wirecatch could not save changes to the download list ({reason}). Recent changes may be lost when it closes. Make sure the disk has free space."
        ),
        FixAction::FreeDiskSpace,
    )
}

/// The message for a download whose run ended without a result (a bug).
#[must_use]
pub fn stopped_unexpectedly() -> UserError {
    fix(
        "The download stopped unexpectedly. Try again; the part already downloaded is kept.",
        FixAction::Retry,
    )
}

/// An `ftps` or `ftpes` link to a server that offers no encryption.
fn ftp_tls_unavailable(host: &str) -> UserError {
    fix(
        format!(
            "The FTP server {host} does not offer encryption, so Wirecatch stopped rather than download without it. If an unencrypted download is acceptable, refresh the link with one that starts with ftp:// instead."
        ),
        FixAction::RefreshLink,
    )
}

/// A proxy that wants a login it was not given, or refused the one it was.
fn proxy_login_refused() -> UserError {
    fix(
        "The proxy asked for a user name and password, or did not accept the ones it was given. Move the download to a queue with another proxy, or check the proxy's user name and password in Settings > Network.",
        FixAction::ChangeQueue,
    )
}

/// An FTP link while `setting` asks for a proxy that cannot carry it.
fn ftp_proxy_unsupported(setting: &ProxySetting) -> UserError {
    match setting {
        ProxySetting::Variable(name) => fix(
            format!(
                "FTP downloads can only go through a SOCKS5 proxy, and the {name} setting on this computer asks for another kind. Remove {name} from the environment to download this link directly, or choose a proxy in the proxy settings, then try again."
            ),
            FixAction::ProxySettings,
        ),
        ProxySetting::System => fix(
            "FTP downloads can only go through a SOCKS5 proxy, and the proxy in this computer's network settings, which covers this link, is another kind. Turn that proxy off or add this server to its exceptions, or choose a proxy in the proxy settings, then try again.",
            FixAction::ProxySettings,
        ),
        ProxySetting::Given => fix(
            "FTP downloads can only go through a SOCKS5 proxy, and the proxy in the proxy settings is another kind. Turn on “FTP links skip the proxy” in Settings > Network, add this server to the hosts that skip the proxy, or use a SOCKS5 proxy, then try again.",
            FixAction::ProxySettings,
        ),
        // A kind of setting added to the engine after this was written.
        _ => fix(
            "FTP downloads can only go through a SOCKS5 proxy, and a proxy set on this computer covers this link. Turn it off to download this link directly, then try again.",
            FixAction::ProxySettings,
        ),
    }
}

/// A download option out of range. Settings are checked when they change and
/// brought into range when they load, so this is a fault in Wirecatch: trying
/// again cannot help, and no fix is offered.
fn invalid_option(option: &str) -> UserError {
    UserError {
        message: format!(
            "Wirecatch could not start this download because a setting it uses ({option}) is out of range. Check Settings, then add the download again."
        ),
        fix: None,
    }
}

/// An FTP server that refused the login: an anonymous one needs a name and
/// password in the link, a named one the right ones.
fn ftp_login_refused(host: &str, code: u16, message: &str, anonymous: bool) -> UserError {
    let answer = message.trim().trim_end_matches('.');
    let message = if anonymous {
        format!(
            "The FTP server {host} does not allow anonymous downloads (it answered \"{code} {answer}\"). Refresh the link with one that includes your user name and password, like ftp://name:password@{host}/file."
        )
    } else {
        format!(
            "The FTP server {host} did not accept the user name or password in the link (it answered \"{code} {answer}\"). Refresh the link with the right ones."
        )
    };
    fix(message, FixAction::RefreshLink)
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

fn http_status(status: u16, worked: Worked) -> UserError {
    match status {
        401 | 403 | 404 | 410 if worked == Worked::Before => fix(
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

/// A span of time in words: `less than a second`, `1 second`, `30 seconds`.
fn how_long(span: std::time::Duration) -> String {
    match span.as_secs() {
        0 => "less than a second".to_owned(),
        1 => "1 second".to_owned(),
        seconds => format!("{seconds} seconds"),
    }
}

/// Describes a file operation that failed while saving into `folder`.
#[must_use]
pub fn disk(error: &io::Error, folder: &Path) -> UserError {
    disk_failure(
        error,
        folder,
        &format!("write the file to {}", folder.display()),
        None,
    )
}

/// Describes a file operation that failed while saving into `folder`; `what`
/// says what could not be done, as in `create the folder (/x)`, and `existing` is
/// the file already there that it was done to, if any: being refused one says nothing about the folder.
fn disk_failure(
    error: &io::Error,
    folder: &Path,
    what: &str,
    existing: Option<&Path>,
) -> UserError {
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
    if error.kind() == io::ErrorKind::PermissionDenied
        && let Some(file) = existing
    {
        return fix(
            format!(
                "Wirecatch could not use {}. Another program may have it open, or it may be read-only. Close that program, then try again.",
                file.display()
            ),
            FixAction::Retry,
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
            format!("Could not {what}: {error}. Check the drive, then try again."),
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

    #[test]
    fn a_saved_link_that_no_longer_parses_asks_for_a_new_one() {
        let given = invalid_link();
        assert_eq!(given.fix, Some(FixAction::RefreshLink));
        assert!(given.message.contains("not a valid web address"));
    }

    #[test]
    fn a_file_the_app_fetches_for_itself_is_told_nothing_about_links_folders_or_queues() {
        let errors = [
            status(401),
            status(403),
            status(404),
            status(410),
            status(400),
            EngineError::InvalidUrl("x".into()),
            EngineError::TooManyRedirects { url: "u".into() },
            EngineError::RedirectWithoutLocation {
                status: 302,
                url: "u".into(),
            },
            insecure(),
            EngineError::TooLarge {
                url: "u".into(),
                limit: 1,
            },
            EngineError::ProxyLoginRefused,
            EngineError::Io {
                action: wirecatch_engine::IoAction::CreateFile,
                path: "/d/f".into(),
                source: io::ErrorKind::PermissionDenied.into(),
            },
            EngineError::Io {
                action: wirecatch_engine::IoAction::CreateFolder,
                path: "/d".into(),
                source: io::ErrorKind::NotADirectory.into(),
            },
        ];
        for error in &errors {
            let told = describe_app_file(error, Path::new("/d")).to_lowercase();
            for advice in ["link", "another folder", "queue"] {
                assert!(!told.contains(advice), "{advice} in {told}");
            }
            assert!(told.ends_with('.'), "{told}");
        }
        let full = describe_app_file(&io(io::ErrorKind::StorageFull.into()), Path::new("/d"));
        assert!(full.contains("disk is full"), "{full}");
        let rejected = EngineError::TargetRejected(wirecatch_engine::PathRejected::new(
            "The installer is wrong.",
        ));
        assert_eq!(
            describe_app_file(&rejected, Path::new("/d")),
            "The installer is wrong."
        );
    }

    #[test]
    fn a_file_held_by_another_program_is_not_blamed_on_the_folder() {
        for action in [
            wirecatch_engine::IoAction::Finish,
            wirecatch_engine::IoAction::OpenFile,
        ] {
            let error = EngineError::Io {
                action,
                path: "/d/f.bin".into(),
                source: io::ErrorKind::PermissionDenied.into(),
            };
            let told = describe(&error, Path::new("/d"), Worked::Before);
            assert_eq!(told.fix, Some(FixAction::Retry), "{}", told.message);
            assert!(told.message.contains("/d/f.bin"), "{}", told.message);
            assert!(told.message.contains("Another program"), "{}", told.message);
        }
    }

    #[cfg(windows)]
    #[test]
    fn replacing_a_file_another_program_holds_is_not_blamed_on_the_folder() {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_SHARE_READ: u32 = 1;

        let dir = wirecatch_test_support::private_dir("messages-held").unwrap();
        let (part, target) = (dir.join("f.bin.wcpart"), dir.join("f.bin"));
        std::fs::write(&part, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&target)
            .unwrap();
        let source = std::fs::rename(&part, &target).unwrap_err();
        drop(held);
        let error = EngineError::Io {
            action: wirecatch_engine::IoAction::Finish,
            path: target,
            source,
        };
        let told = describe(&error, &dir, Worked::Before);
        assert_ne!(told.fix, Some(FixAction::ChooseFolder), "{}", told.message);
    }

    #[test]
    fn an_answer_over_its_limit_is_not_offered_a_retry() {
        let error = EngineError::TooLarge {
            url: "u".into(),
            limit: 1,
        };
        let told = describe(&error, Path::new("/d"), Worked::Never);
        assert_eq!(told.fix, Some(FixAction::RefreshLink));
        assert!(!told.message.contains("Try again"), "{}", told.message);
    }

    fn status(code: u16) -> EngineError {
        EngineError::HttpStatus {
            status: code,
            url: "https://example.com/f".into(),
        }
    }

    fn action(error: &EngineError, worked: Worked) -> Option<FixAction> {
        describe(error, Path::new("/downloads"), worked).fix
    }

    // It fell into the catch-all and told the person to try again, which can never help.
    #[test]
    fn an_option_out_of_range_is_not_offered_a_retry() {
        let error = EngineError::InvalidOption("connections");
        let described = describe(&error, Path::new("/downloads"), Worked::Never);
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
    fn an_ftp_link_behind_a_proxy_says_which_setting_to_change() {
        let variable = ftp_proxy_unsupported(&ProxySetting::Variable("ALL_PROXY".into()));
        assert!(
            variable
                .message
                .contains("Remove ALL_PROXY from the environment")
        );
        let system = ftp_proxy_unsupported(&ProxySetting::System);
        assert!(system.message.contains("network settings"));
        assert!(system.message.contains("exceptions"));
        let given = ftp_proxy_unsupported(&ProxySetting::Given);
        assert!(given.message.contains("FTP links skip the proxy"));
        // A proxy that refuses the login is another queue's to avoid (D73.4).
        let login = describe(
            &EngineError::ProxyLoginRefused,
            Path::new(""),
            Worked::Never,
        );
        assert!(login.message.contains("user name and password"));
        assert_eq!(login.fix, Some(FixAction::ChangeQueue));
        for e in [variable, system, given] {
            assert_eq!(e.fix, Some(FixAction::ProxySettings));
            assert!(e.message.ends_with('.') && !e.message.contains('\u{2014}'));
        }
    }

    #[test]
    fn refused_link_that_worked_before_is_expired() {
        for code in [403, 404, 410] {
            let e = describe(&status(code), Path::new("/d"), Worked::Before);
            assert_eq!(e.fix, Some(FixAction::RefreshLink));
            assert!(e.message.contains("expired"), "{}", e.message);
        }
        assert!(
            !describe(&status(404), Path::new("/d"), Worked::Never)
                .message
                .contains("expired")
        );
    }

    #[test]
    fn server_trouble_and_network_problems_offer_retry() {
        assert_eq!(action(&status(503), Worked::Never), Some(FixAction::Retry));
        assert_eq!(action(&status(429), Worked::Before), Some(FixAction::Retry));
        assert_eq!(action(&stalled(), Worked::Before), Some(FixAction::Retry));
        assert_eq!(
            action(&EngineError::RemoteChanged, Worked::Before),
            Some(FixAction::Retry)
        );
        assert_eq!(
            action(&EngineError::PrematureEof { at: 1, end: 2 }, Worked::Before),
            Some(FixAction::Retry)
        );
    }

    /// A stall as the engine reports it.
    fn stalled() -> EngineError {
        EngineError::Stalled {
            after: std::time::Duration::from_secs(30),
        }
    }

    /// A disk error as the engine reports it.
    fn io(source: io::Error) -> EngineError {
        EngineError::Io {
            action: wirecatch_engine::IoAction::Write,
            path: "/downloads/f.bin.wcpart".into(),
            source,
        }
    }

    #[test]
    fn full_disk_asks_for_space() {
        let kind = io(io::ErrorKind::StorageFull.into());
        assert_eq!(
            action(&kind, Worked::Before),
            Some(FixAction::FreeDiskSpace)
        );
        let raw = io(io::Error::from_raw_os_error(DISK_FULL_CODES[0]));
        assert_eq!(action(&raw, Worked::Before), Some(FixAction::FreeDiskSpace));
    }

    #[test]
    fn a_stall_says_how_long_in_words() {
        let said = |after| {
            describe(
                &EngineError::Stalled { after },
                Path::new("/d"),
                Worked::Before,
            )
            .message
        };
        let short = said(std::time::Duration::from_millis(500));
        assert!(short.contains("for less than a second"), "{short}");
        let one = said(std::time::Duration::from_secs(1));
        assert!(
            one.contains("for 1 second.") || one.contains("for 1 second "),
            "{one}"
        );
        let many = said(std::time::Duration::from_secs(30));
        assert!(many.contains("for 30 seconds"), "{many}");
    }

    #[test]
    fn a_disk_error_says_what_failed_with_which_file() {
        let error = EngineError::Io {
            action: wirecatch_engine::IoAction::Flush,
            path: "/downloads/f.bin.wcpart".into(),
            source: io::ErrorKind::Other.into(),
        };
        let message = describe(&error, Path::new("/downloads"), Worked::Before).message;
        assert_eq!(
            message,
            "Could not flush the file to the disk (/downloads/f.bin.wcpart): other error. Check the drive, then try again."
        );
        let resize = EngineError::Io {
            action: wirecatch_engine::IoAction::Resize,
            path: "/downloads/f.bin.wcpart".into(),
            source: io::ErrorKind::Other.into(),
        };
        let message = describe(&resize, Path::new("/downloads"), Worked::Before).message;
        assert!(
            message.starts_with("Could not set the size of the file (/downloads/f.bin.wcpart)"),
            "{message}"
        );
    }

    #[test]
    fn forbidden_folder_asks_for_another() {
        let e = describe(
            &EngineError::Io {
                action: wirecatch_engine::IoAction::CreateFile,
                path: "/locked/f.bin.wcpart".into(),
                source: io::ErrorKind::PermissionDenied.into(),
            },
            Path::new("/locked"),
            Worked::Never,
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
        let insecure = describe(&insecure(), Path::new("/d"), Worked::Before);
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
            action(
                &EngineError::TooManyRedirects { url: "u".into() },
                Worked::Never
            ),
            Some(FixAction::RefreshLink)
        );
        // The app refused what the server sent (another file than the browser
        // saw, or a sign-in page): a fresh link from the page is the way out.
        let rejected = describe(
            &EngineError::TargetRejected(wirecatch_engine::PathRejected::new(
                "The server sent a web page instead of the file. Refresh the link.",
            )),
            Path::new("/d"),
            Worked::Never,
        );
        assert!(rejected.message.starts_with("The server sent a web page"));
        assert_eq!(rejected.fix, Some(FixAction::RefreshLink));
        assert_eq!(
            action(&EngineError::Internal("bug".into()), Worked::Never),
            Some(FixAction::Retry)
        );
    }

    #[test]
    fn an_untrusted_certificate_is_not_called_a_connection_problem() {
        let error = EngineError::Certificate {
            reason: "UnknownIssuer".into(),
        };
        let told = describe(&error, Path::new("/d"), Worked::Never);
        assert_eq!(
            told.message,
            "This site's security certificate could not be verified, so Wirecatch did not download from it. Check that your computer's date and time are right. If they are, the site needs to fix its certificate."
        );
        assert_eq!(told.fix, Some(FixAction::Retry));
    }

    #[test]
    fn an_untrusted_certificate_through_a_proxy_names_the_proxy() {
        let told = describe_run(
            &EngineError::Certificate {
                reason: "UnknownIssuer".into(),
            },
            Path::new("/d"),
            Worked::Never,
            ChosenBy::Person,
            Some("proxy.example:8080"),
        );
        assert_eq!(
            told.message,
            "A security certificate on this connection could not be verified, and the proxy proxy.example:8080 may be the cause. Move the download to a queue with another proxy, or check the proxy in Settings > Network."
        );
        assert_eq!(told.fix, Some(FixAction::ChangeQueue));
    }

    #[test]
    fn a_refused_link_says_who_can_open_it() {
        let error = EngineError::PrivateNetwork { url: "u".into() };
        let website = describe_run(
            &error,
            Path::new("/d"),
            Worked::Never,
            ChosenBy::Website,
            None,
        );
        assert_eq!(website.fix, Some(FixAction::RefreshLink));
        assert!(
            website.message.contains("a website gave"),
            "{}",
            website.message
        );
        // The fix is named as the person meets it: the row's button gives a
        // download a new link ("Get a fresh link", "New link"); no button is
        // called "Refresh link".
        assert!(
            website.message.contains("a new link") && !website.message.contains("Refresh link"),
            "{}",
            website.message
        );
        for own in [ChosenBy::Person, ChosenBy::LocalPage] {
            let refused = describe_run(&error, Path::new("/d"), Worked::Never, own, None);
            assert_eq!(refused.fix, None, "pasting it again would not help");
            assert!(!refused.message.contains("website"), "{}", refused.message);
        }
        for text in [
            website.message,
            private_network(ChosenBy::Person).message,
            CAPTURE_TO_OWN_NETWORK.into(),
        ] {
            assert!(!text.contains('\u{2014}') && text.ends_with('.'), "{text}");
        }
    }

    #[test]
    fn messages_follow_the_house_style() {
        let errors = [
            status(403),
            status(404),
            status(500),
            status(400),
            EngineError::InvalidUrl("x".into()),
            stalled(),
            EngineError::RangeNotHonored {
                expected: 1,
                got: None,
            },
            EngineError::Cancelled,
            io(io::ErrorKind::Other.into()),
            EngineError::TooManyRedirects { url: "u".into() },
            EngineError::RedirectWithoutLocation {
                status: 302,
                url: "u".into(),
            },
            insecure(),
            EngineError::PrivateNetwork { url: "u".into() },
            EngineError::Internal("x".into()),
        ];
        for e in &errors {
            for worked in [Worked::Never, Worked::Before] {
                let m = describe(e, Path::new("/d"), worked).message;
                assert!(!m.contains('\u{2014}'), "em dash in {m}");
                assert!(m.ends_with('.'), "{m}");
            }
        }
    }
}
