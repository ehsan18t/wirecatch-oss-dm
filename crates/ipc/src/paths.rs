//! Pure endpoint and permission rules, kept free of system calls so they are unit
//! tested on every platform.

//! Each platform uses only its half of this module; the other half is still
//! compiled and tested everywhere, hence the `allow(dead_code)` markers.

use std::ffi::OsStr;
use std::path::PathBuf;

/// Prefix of the per-user pipe name on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const PIPE_PREFIX: &str = r"\\.\pipe\wirecatch-app-";

/// Name of the folder created inside `XDG_RUNTIME_DIR`.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const RUNTIME_SUBDIR: &str = "wirecatch";

/// File name of the socket.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const SOCKET_NAME: &str = "app.sock";

/// File name of the lock that marks the live listener.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const LOCK_NAME: &str = "app.lock";

/// The Windows pipe name for a user, given the user's SID in string form.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn pipe_name(user_sid: &str) -> String {
    format!("{PIPE_PREFIX}{user_sid}")
}

/// The Linux socket path: `$XDG_RUNTIME_DIR/wirecatch/app.sock`, or
/// `/tmp/wirecatch-<uid>/app.sock` when `XDG_RUNTIME_DIR` is unset, empty or not an
/// absolute path (the XDG Base Directory specification says relative values must
/// be ignored).
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn socket_path(xdg_runtime_dir: Option<&OsStr>, uid: u32) -> PathBuf {
    let usable = xdg_runtime_dir.filter(|dir| dir.as_encoded_bytes().first() == Some(&b'/'));
    let dir = match usable {
        Some(runtime) => PathBuf::from(runtime).join(RUNTIME_SUBDIR),
        None => PathBuf::from("/tmp").join(format!("wirecatch-{uid}")),
    };
    dir.join(SOCKET_NAME)
}

/// What is known about an existing folder, read without following symbolic links.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct DirFacts {
    pub is_dir: bool,
    pub is_symlink: bool,
    pub owner_uid: u32,
    pub mode: u32,
}

/// Checks that a folder is private to `uid`. Returns the reason when it is not.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn check_private_dir(facts: DirFacts, uid: u32) -> Result<(), String> {
    if facts.is_symlink {
        return Err("it is a symbolic link".into());
    }
    if !facts.is_dir {
        return Err("it is not a folder".into());
    }
    if facts.owner_uid != uid {
        return Err(format!(
            "it belongs to user {} instead of you (user {uid})",
            facts.owner_uid
        ));
    }
    let open_to_others = facts.mode & 0o077;
    if open_to_others != 0 {
        return Err(format!(
            "other users have access to it (permissions {:03o}, expected 700)",
            facts.mode & 0o777
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_embeds_the_user_sid() {
        assert_eq!(
            pipe_name("S-1-5-21-1-2-3-1001"),
            r"\\.\pipe\wirecatch-app-S-1-5-21-1-2-3-1001"
        );
    }

    #[test]
    fn socket_goes_in_the_runtime_dir_when_set() {
        assert_eq!(
            socket_path(Some(OsStr::new("/run/user/1000")), 1000),
            PathBuf::from("/run/user/1000")
                .join("wirecatch")
                .join("app.sock")
        );
    }

    #[test]
    fn socket_falls_back_to_tmp_without_a_usable_runtime_dir() {
        let fallback = PathBuf::from("/tmp")
            .join("wirecatch-1000")
            .join("app.sock");
        assert_eq!(socket_path(None, 1000), fallback);
        assert_eq!(socket_path(Some(OsStr::new("")), 1000), fallback);
        assert_eq!(
            socket_path(Some(OsStr::new("run/user/1000")), 1000),
            fallback
        );
    }

    fn dir(owner_uid: u32, mode: u32) -> DirFacts {
        DirFacts {
            is_dir: true,
            is_symlink: false,
            owner_uid,
            mode: 0o040000 | mode,
        }
    }

    #[test]
    fn a_private_folder_owned_by_the_user_is_accepted() {
        assert!(check_private_dir(dir(1000, 0o700), 1000).is_ok());
        assert!(check_private_dir(dir(1000, 0o500), 1000).is_ok());
    }

    #[test]
    fn a_folder_others_can_reach_is_refused() {
        for mode in [0o701, 0o710, 0o750, 0o755, 0o777, 0o707] {
            let err = check_private_dir(dir(1000, mode), 1000).unwrap_err();
            assert!(err.contains("other users"), "{mode:o}: {err}");
        }
    }

    #[test]
    fn a_folder_owned_by_someone_else_is_refused() {
        let err = check_private_dir(dir(0, 0o700), 1000).unwrap_err();
        assert!(err.contains("belongs to user 0"), "{err}");
    }

    #[test]
    fn symlinks_and_files_are_refused() {
        let link = DirFacts {
            is_symlink: true,
            ..dir(1000, 0o700)
        };
        assert!(check_private_dir(link, 1000).is_err());
        let file = DirFacts {
            is_dir: false,
            ..dir(1000, 0o600)
        };
        assert!(check_private_dir(file, 1000).is_err());
    }
}
