//! The user's standard folders, and portable paths built on them (D6).
//!
//! Settings store folders as [`PortablePath`]s such as `{Downloads}/Video`, so a
//! settings profile exported on one machine resolves to the right folders on another.

use std::path::{Component, Path, PathBuf, Prefix};

use wirecatch_protocol::app::PortablePath;

/// Why a [`PortablePath`] does not name a folder on this machine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// A `{Name}` placeholder that is not one of the known folders.
    #[error(
        "\"{0}\" uses an unknown folder name. Use one of {{Downloads}}, {{Home}}, {{Documents}}, {{Videos}}, {{Music}} or {{Pictures}}."
    )]
    UnknownPlaceholder(String),
    /// A plain path that is not absolute on this machine.
    #[error("\"{0}\" is not a full folder path on this computer. Choose the folder again.")]
    NotAbsolute(String),
    /// A path that climbs out of its folder with `..`.
    #[error("\"{0}\" must not contain \"..\". Choose the folder again.")]
    ParentReference(String),
    /// A part after the placeholder that is not a plain folder name (for example a
    /// drive like `C:`, or `.`), which could point outside the named folder.
    #[error(
        "\"{0}\" contains a part that is not a plain folder name. Folder names cannot contain \":\". Choose the folder again."
    )]
    NotAFolderName(String),
    /// A Windows device or verbatim path (`\\.\` or `\\?\`), which names a device or
    /// skips the usual checks on a path rather than naming an ordinary folder.
    #[error(
        "\"{0}\" is a special Windows path, not an ordinary folder. Choose a folder on a drive."
    )]
    Special(String),
    /// A network folder (`\\server\share`) from a settings file, which is only used
    /// when the user chooses it on this computer.
    #[error(
        "\"{0}\" is a network folder. Folders from a settings file must be on this computer; choose the network folder in Wirecatch if you want to use it."
    )]
    Network(String),
}

/// Whether `segment` is one ordinary folder name, the same on every system: no
/// drive, root, `.` or `..`, and no `:` (which Windows reads as a drive or stream).
fn is_plain_name(segment: &str) -> bool {
    let mut parts = Path::new(segment).components();
    !segment.contains(':')
        && matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
}

/// The user's standard folders on one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownFolders {
    /// `{Home}`.
    pub home: PathBuf,
    /// `{Downloads}`.
    pub downloads: PathBuf,
    /// `{Documents}`.
    pub documents: PathBuf,
    /// `{Videos}`.
    pub videos: PathBuf,
    /// `{Music}`.
    pub music: PathBuf,
    /// `{Pictures}`.
    pub pictures: PathBuf,
}

impl KnownFolders {
    /// The current user's folders as the operating system names them. A folder the
    /// system does not name falls back to the usual subfolder of home. Returns `None`
    /// only when the home folder itself is unknown.
    pub fn detect() -> Option<Self> {
        let home = dirs::home_dir()?;
        let or_home = |found: Option<PathBuf>, name: &str| found.unwrap_or_else(|| home.join(name));
        Some(Self {
            downloads: or_home(dirs::download_dir(), "Downloads"),
            documents: or_home(dirs::document_dir(), "Documents"),
            videos: or_home(dirs::video_dir(), "Videos"),
            music: or_home(dirs::audio_dir(), "Music"),
            pictures: or_home(dirs::picture_dir(), "Pictures"),
            home,
        })
    }

    /// Folders with their conventional names under `home`. Useful for tests and for
    /// simulating another machine.
    pub fn under_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            downloads: home.join("Downloads"),
            documents: home.join("Documents"),
            videos: home.join("Videos"),
            music: home.join("Music"),
            pictures: home.join("Pictures"),
            home,
        }
    }

    /// The folder `path` names on this machine.
    pub fn resolve(&self, path: &PortablePath) -> Result<PathBuf, PathError> {
        let raw = path.0.trim();
        let Some(rest) = raw.strip_prefix('{') else {
            return self.resolve_plain(raw);
        };
        let (name, tail) = rest
            .split_once('}')
            .ok_or_else(|| PathError::UnknownPlaceholder(raw.to_string()))?;
        let base = self
            .placeholders()
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, folder)| folder)
            .ok_or_else(|| PathError::UnknownPlaceholder(raw.to_string()))?;
        if !tail.is_empty() && !tail.starts_with(['/', '\\']) {
            return Err(PathError::UnknownPlaceholder(raw.to_string()));
        }
        let mut out = base.to_path_buf();
        for segment in tail.split(['/', '\\']).filter(|s| !s.is_empty()) {
            if segment == ".." {
                return Err(PathError::ParentReference(raw.to_string()));
            }
            if !is_plain_name(segment) {
                return Err(PathError::NotAFolderName(raw.to_string()));
            }
            out.push(segment);
        }
        // Belt and braces: whatever the segments were, the result must be a full
        // path inside the folder the placeholder names.
        if !out.is_absolute() || !out.starts_with(base) {
            return Err(PathError::NotAFolderName(raw.to_string()));
        }
        Ok(out)
    }

    /// The folder `path` names on this machine, for a folder from a settings file
    /// rather than the user's own choice: network folders are refused too.
    pub fn resolve_local(&self, path: &PortablePath) -> Result<PathBuf, PathError> {
        let resolved = self.resolve(path)?;
        // A placeholder names this user's own folder, wherever the system keeps it
        // (Downloads redirected to a server is still the user's Downloads).
        let placeholder = path.0.trim_start().starts_with('{');
        if !placeholder
            && let Some(Component::Prefix(prefix)) = resolved.components().next()
            && matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
        {
            return Err(PathError::Network(path.0.trim().to_string()));
        }
        Ok(resolved)
    }

    /// `path` written relative to the most specific known folder that contains it,
    /// so it resolves correctly on another machine. Placeholders and paths outside
    /// every known folder are returned unchanged.
    pub fn to_portable(&self, path: &PortablePath) -> PortablePath {
        if path.0.trim_start().starts_with('{') {
            return path.clone();
        }
        let absolute = Path::new(path.0.trim());
        let mut candidates = self.placeholders();
        // Most specific first: `{Downloads}` wins over `{Home}` for a path in Downloads.
        candidates.sort_by_key(|(_, folder)| std::cmp::Reverse(folder.components().count()));
        for (name, folder) in candidates {
            if let Ok(tail) = absolute.strip_prefix(folder) {
                let mut out = format!("{{{name}}}");
                for part in tail.components() {
                    out.push('/');
                    out.push_str(&part.as_os_str().to_string_lossy());
                }
                return PortablePath(out);
            }
        }
        path.clone()
    }

    fn resolve_plain(&self, raw: &str) -> Result<PathBuf, PathError> {
        let path = Path::new(raw);
        // A device (`\\.\`) or a verbatim path (`\\?\`, which turns off the usual
        // checks on names) is never a folder to download into. A verbatim network
        // path (`\\?\UNC\`) is still a network folder, judged as one.
        if let Some(Component::Prefix(prefix)) = path.components().next()
            && matches!(
                prefix.kind(),
                Prefix::DeviceNS(_) | Prefix::Verbatim(_) | Prefix::VerbatimDisk(_)
            )
        {
            return Err(PathError::Special(raw.to_string()));
        }
        if !path.is_absolute() {
            return Err(PathError::NotAbsolute(raw.to_string()));
        }
        if path.components().any(|c| c == Component::ParentDir) {
            return Err(PathError::ParentReference(raw.to_string()));
        }
        Ok(path.to_path_buf())
    }

    /// In order of preference when two names point at the same folder.
    fn placeholders(&self) -> Vec<(&'static str, &Path)> {
        vec![
            ("Downloads", &self.downloads),
            ("Documents", &self.documents),
            ("Videos", &self.videos),
            ("Music", &self.music),
            ("Pictures", &self.pictures),
            ("Home", &self.home),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\ada")
        } else {
            PathBuf::from("/home/ada")
        }
    }

    fn portable(s: &str) -> PortablePath {
        PortablePath(s.to_string())
    }

    #[test]
    fn placeholders_resolve_under_this_machines_folders() {
        let folders = KnownFolders::under_home(home());
        assert_eq!(
            folders.resolve(&portable("{Downloads}/Video/Clips")),
            Ok(home().join("Downloads").join("Video").join("Clips"))
        );
        assert_eq!(folders.resolve(&portable("{Home}")), Ok(home()));
        assert_eq!(
            folders.resolve(&portable("{Music}/")),
            Ok(home().join("Music"))
        );
    }

    #[test]
    fn bad_paths_are_refused_with_a_reason() {
        let folders = KnownFolders::under_home(home());
        assert!(matches!(
            folders.resolve(&portable("{Desktop}/x")),
            Err(PathError::UnknownPlaceholder(_))
        ));
        assert!(matches!(
            folders.resolve(&portable("{Downloads}x")),
            Err(PathError::UnknownPlaceholder(_))
        ));
        assert!(matches!(
            folders.resolve(&portable("relative/folder")),
            Err(PathError::NotAbsolute(_))
        ));
        assert!(matches!(
            folders.resolve(&portable("{Downloads}/../../etc")),
            Err(PathError::ParentReference(_))
        ));
    }

    #[test]
    fn placeholder_paths_cannot_escape_their_folder() {
        let folders = KnownFolders::under_home(home());
        for escape in [
            "{Downloads}/C:Windows/System32",
            "{Downloads}/C:/Windows",
            "{Downloads}/file.txt:stream",
            "{Downloads}/./x",
        ] {
            assert!(
                matches!(
                    folders.resolve(&portable(escape)),
                    Err(PathError::NotAFolderName(_))
                ),
                "{escape} was accepted"
            );
        }
        // Doubled separators are harmless and stay inside.
        assert_eq!(
            folders.resolve(&portable("{Downloads}//a\\\\b")),
            Ok(home().join("Downloads").join("a").join("b"))
        );
        // A machine whose folders are not full paths resolves nothing.
        let relative = KnownFolders::under_home("relative-home");
        assert!(relative.resolve(&portable("{Downloads}/x")).is_err());
    }

    #[test]
    fn absolute_paths_resolve_to_themselves() {
        let folders = KnownFolders::under_home(home());
        let elsewhere = if cfg!(windows) {
            r"D:\Media"
        } else {
            "/srv/media"
        };
        assert_eq!(
            folders.resolve(&portable(elsewhere)),
            Ok(PathBuf::from(elsewhere))
        );
        assert_eq!(
            folders.to_portable(&portable(elsewhere)),
            portable(elsewhere)
        );
    }

    #[cfg(windows)]
    #[test]
    fn device_and_verbatim_paths_are_never_folders() {
        let folders = KnownFolders::under_home(home());
        for special in [
            r"\\.\C:\Downloads",
            r"\\.\PhysicalDrive0",
            r"\\.\pipe\wirecatch",
            r"\\?\C:\Downloads",
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\Downloads",
        ] {
            for resolved in [
                folders.resolve(&portable(special)),
                folders.resolve_local(&portable(special)),
            ] {
                assert!(
                    matches!(resolved, Err(PathError::Special(_))),
                    "{special} gave {resolved:?}"
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn network_folders_need_the_users_own_choice() {
        let folders = KnownFolders::under_home(home());
        for network in [
            r"\\server\share\Downloads",
            r"\\?\UNC\server\share\Downloads",
        ] {
            // Chosen by the user in a dialog: fine.
            assert_eq!(
                folders.resolve(&portable(network)),
                Ok(PathBuf::from(network))
            );
            // From a settings file made somewhere else: not used.
            assert!(
                matches!(
                    folders.resolve_local(&portable(network)),
                    Err(PathError::Network(_))
                ),
                "{network} was accepted from a settings file"
            );
        }
        assert_eq!(
            folders.resolve_local(&portable(r"D:\Media")),
            Ok(PathBuf::from(r"D:\Media"))
        );
        assert_eq!(
            folders.resolve_local(&portable("{Videos}/Talks")),
            Ok(home().join("Videos").join("Talks"))
        );
        // The user's own Downloads may live on a server; it is still theirs.
        let mut redirected = KnownFolders::under_home(home());
        redirected.downloads = PathBuf::from(r"\\server\home\ada\Downloads");
        assert_eq!(
            redirected.resolve_local(&portable("{Downloads}/Video")),
            Ok(PathBuf::from(r"\\server\home\ada\Downloads\Video"))
        );
    }

    #[test]
    fn to_portable_picks_the_most_specific_folder() {
        let folders = KnownFolders::under_home(home());
        let in_downloads = home().join("Downloads").join("Video");
        assert_eq!(
            folders.to_portable(&portable(&in_downloads.to_string_lossy())),
            portable("{Downloads}/Video")
        );
        let in_home = home().join("Projects");
        assert_eq!(
            folders.to_portable(&portable(&in_home.to_string_lossy())),
            portable("{Home}/Projects")
        );
        assert_eq!(
            folders.to_portable(&portable("{Videos}/x")),
            portable("{Videos}/x")
        );
    }

    #[test]
    fn round_trip_between_machines() {
        let here = KnownFolders::under_home(home());
        let there = KnownFolders::under_home(home().join("other-user"));
        let original = home().join("Videos").join("Talks");
        let exported = here.to_portable(&portable(&original.to_string_lossy()));
        assert_eq!(
            there.resolve(&exported),
            Ok(home().join("other-user").join("Videos").join("Talks"))
        );
    }
}
