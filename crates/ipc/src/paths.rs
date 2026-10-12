//! Pure endpoint and permission rules, kept free of system calls so they are unit
//! tested on every platform.
//!
//! Each platform builds only its half of this module; tests build both halves
//! everywhere, so the Linux rules are tested on Windows too and the other way round.

#[cfg(any(unix, test))]
use std::ffi::OsStr;
#[cfg(any(unix, test))]
use std::path::{Path, PathBuf};

use wirecatch_product::Channel;
#[cfg(any(unix, test))]
use wirecatch_product::{IPC_FALLBACK_FOLDER_PREFIX, IPC_RUNTIME_FOLDER};

/// Where every named pipe lives on Windows.
#[cfg(any(windows, test))]
const PIPE_ROOT: &str = r"\\.\pipe\";

/// The lock that marks the live listener of the socket at `socket`: the socket's
/// path with `.lock` after it. Each socket has a lock of its own, so release and
/// debug builds can listen on sibling sockets in one folder.
#[cfg(any(unix, test))]
pub(crate) fn lock_path(socket: &Path) -> PathBuf {
    let mut path = socket.as_os_str().to_owned();
    path.push(".lock");
    PathBuf::from(path)
}

/// The Windows pipe name of `channel` for a user, given the user's SID in string
/// form: `\\.\pipe\wirecatch-app-<SID>` for release builds.
#[cfg(any(windows, test))]
pub(crate) fn pipe_name(channel: Channel, user_sid: &str) -> String {
    format!("{PIPE_ROOT}{}{user_sid}", channel.pipe_prefix())
}

/// The Linux socket path of `channel`: `$XDG_RUNTIME_DIR/wirecatch/app.sock` for
/// release builds, or `/tmp/wirecatch-<uid>/app.sock` when `XDG_RUNTIME_DIR` is
/// unset, empty or not an absolute path (the XDG Base Directory specification says
/// relative values must be ignored). Every channel's socket is in the same folder.
#[cfg(any(unix, test))]
pub(crate) fn socket_path(channel: Channel, xdg_runtime_dir: Option<&OsStr>, uid: u32) -> PathBuf {
    let usable = xdg_runtime_dir.filter(|dir| dir.as_encoded_bytes().first() == Some(&b'/'));
    let dir = match usable {
        Some(runtime) => PathBuf::from(runtime).join(IPC_RUNTIME_FOLDER),
        None => PathBuf::from("/tmp").join(format!("{IPC_FALLBACK_FOLDER_PREFIX}{uid}")),
    };
    dir.join(channel.socket_name())
}

/// What is known about an existing folder, read without following symbolic links.
#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct DirFacts {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub owner_uid: u32,
    pub mode: u32,
}

/// Why a folder is not private, and what the user can do about it.
#[cfg(any(unix, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirProblem {
    pub reason: String,
    pub fix: String,
}

/// Checks that a folder is private to `uid`. `shown` is the folder as it appears
/// in advice.
#[cfg(any(unix, test))]
pub(crate) fn check_private_dir(facts: DirFacts, uid: u32, shown: &str) -> Result<(), DirProblem> {
    let problem = |reason: String, fix: String| Err(DirProblem { reason, fix });
    if facts.is_symlink || !facts.is_dir {
        let what = if facts.is_symlink {
            "it is a symbolic link"
        } else {
            "it is not a folder"
        };
        return problem(
            what.into(),
            format!("Wirecatch did not create it. Remove {shown}, then open Wirecatch again."),
        );
    }
    if facts.owner_uid != uid {
        // chmod or deleting it is not possible for us; only its owner or an
        // administrator can remove it.
        return problem(
            format!(
                "it belongs to user {} instead of you (user {uid}), so that user could intercept your downloads",
                facts.owner_uid
            ),
            "Ask an administrator to delete it, or restart the computer (which empties the temporary folder on most systems), then open Wirecatch again.".into(),
        );
    }
    if facts.mode & 0o077 != 0 {
        return problem(
            format!(
                "other users have access to it (permissions {:03o}, expected 700)",
                facts.mode & 0o777
            ),
            format!("Run `chmod 700 {shown}` (or delete the folder), then open Wirecatch again."),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_embeds_the_user_sid() {
        assert_eq!(
            pipe_name(Channel::Release, "S-1-5-21-1-2-3-1001"),
            r"\\.\pipe\wirecatch-app-S-1-5-21-1-2-3-1001"
        );
        assert_eq!(
            pipe_name(Channel::Dev, "S-1-5-21-1-2-3-1001"),
            r"\\.\pipe\wirecatch-app-dev-S-1-5-21-1-2-3-1001"
        );
    }

    #[test]
    fn each_socket_has_its_own_lock_next_to_it() {
        assert_eq!(
            lock_path(Path::new("/run/user/1000/wirecatch/app.sock")),
            Path::new("/run/user/1000/wirecatch/app.sock.lock")
        );
        assert_ne!(
            lock_path(Path::new("/d/app.sock")),
            lock_path(Path::new("/d/app-dev.sock"))
        );
    }

    #[test]
    fn socket_goes_in_the_runtime_dir_when_set() {
        assert_eq!(
            socket_path(Channel::Release, Some(OsStr::new("/run/user/1000")), 1000),
            PathBuf::from("/run/user/1000")
                .join("wirecatch")
                .join("app.sock")
        );
        assert_eq!(
            socket_path(Channel::Dev, Some(OsStr::new("/run/user/1000")), 1000),
            PathBuf::from("/run/user/1000")
                .join("wirecatch")
                .join("app-dev.sock")
        );
    }

    #[test]
    fn socket_falls_back_to_tmp_without_a_usable_runtime_dir() {
        let fallback = PathBuf::from("/tmp")
            .join("wirecatch-1000")
            .join("app.sock");
        let release = |dir| socket_path(Channel::Release, dir, 1000);
        assert_eq!(release(None), fallback);
        assert_eq!(release(Some(OsStr::new(""))), fallback);
        assert_eq!(release(Some(OsStr::new("run/user/1000"))), fallback);
    }

    /// The file type bits of a folder in a mode (`S_IFDIR`, which `libc` has on Linux
    /// only).
    const FOLDER: u32 = 0o040_000;

    fn dir(owner_uid: u32, mode: u32) -> DirFacts {
        DirFacts {
            is_dir: true,
            is_symlink: false,
            owner_uid,
            mode: FOLDER | mode,
        }
    }

    const SHOWN: &str = "/tmp/wirecatch-1000";

    #[test]
    fn a_private_folder_owned_by_the_user_is_accepted() {
        assert!(check_private_dir(dir(1000, 0o700), 1000, SHOWN).is_ok());
        assert!(check_private_dir(dir(1000, 0o500), 1000, SHOWN).is_ok());
    }

    #[test]
    fn a_folder_others_can_reach_is_refused_with_chmod_advice() {
        for mode in [0o701, 0o710, 0o750, 0o755, 0o777, 0o707] {
            let err = check_private_dir(dir(1000, mode), 1000, SHOWN).unwrap_err();
            assert!(err.reason.contains("other users"), "{mode:o}: {err:?}");
            assert!(err.fix.contains("chmod 700 /tmp/wirecatch-1000"), "{err:?}");
        }
    }

    #[test]
    fn a_folder_owned_by_someone_else_gets_advice_the_user_can_follow() {
        let err = check_private_dir(dir(0, 0o700), 1000, SHOWN).unwrap_err();
        assert!(err.reason.contains("belongs to user 0"), "{err:?}");
        // The user cannot chmod a folder they do not own.
        assert!(!err.fix.contains("chmod"), "{err:?}");
        assert!(err.fix.contains("administrator"), "{err:?}");
    }

    #[test]
    fn symlinks_and_files_are_refused() {
        let link = DirFacts {
            is_symlink: true,
            ..dir(1000, 0o700)
        };
        assert!(check_private_dir(link, 1000, SHOWN).is_err());
        let file = DirFacts {
            is_dir: false,
            ..dir(1000, 0o600)
        };
        let err = check_private_dir(file, 1000, SHOWN).unwrap_err();
        assert!(err.fix.contains("Remove /tmp/wirecatch-1000"), "{err:?}");
    }
}
