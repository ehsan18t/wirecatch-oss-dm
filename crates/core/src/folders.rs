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
    /// A startup, Start menu or system folder (or, on Linux, a hidden folder in
    /// home) from a settings file: a file downloaded there could run by itself.
    #[error(
        "\"{0}\" is a system or startup folder, where a downloaded file could run by itself. Folders from a settings file cannot be there; choose another folder."
    )]
    Protected(String),
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
    /// Folders a settings file may never point downloads into, because a file
    /// saved there runs by itself or shows up as a program: startup folders,
    /// the Start menu and the system's own folders on Windows; autostart,
    /// application entries, systemd units and system folders on Linux (where
    /// every hidden folder in home is refused too). See
    /// [`is_protected`](Self::is_protected).
    pub protected: Vec<PathBuf>,
}

impl KnownFolders {
    /// The current user's folders as the operating system names them. A folder the
    /// system does not name falls back to the usual subfolder of home. Returns `None`
    /// only when the home folder itself is unknown.
    #[must_use]
    pub fn detect() -> Option<Self> {
        let home = dirs::home_dir()?;
        let or_home = |found: Option<PathBuf>, name: &str| found.unwrap_or_else(|| home.join(name));
        Some(Self {
            downloads: or_home(dirs::download_dir(), "Downloads"),
            documents: or_home(dirs::document_dir(), "Documents"),
            videos: or_home(dirs::video_dir(), "Videos"),
            music: or_home(dirs::audio_dir(), "Music"),
            pictures: or_home(dirs::picture_dir(), "Pictures"),
            protected: protected_folders(&home, |name| {
                std::env::var_os(name)
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
            }),
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
            protected: protected_folders(&home, |_| None),
            home,
        }
    }

    /// Whether `path` is in a folder downloads from a settings file must never
    /// go to (see [`protected`](Self::protected)). Both the path as written and
    /// the real path of its nearest existing folder are checked, so a short
    /// (8.3) name, a different case, a trailing dot or a link does not get
    /// around it. Blocks on the disk.
    #[must_use]
    pub fn is_protected(&self, path: &Path) -> bool {
        let real = |p: &Path| real_path(p);
        let mut roots: Vec<PathBuf> = self.protected.clone();
        roots.extend(self.protected.iter().filter_map(|p| real(p)));
        let mut homes = vec![self.home.clone()];
        homes.extend(real(&self.home));
        let mut forms = vec![path.to_path_buf()];
        forms.extend(real(path));
        forms.iter().any(|form| {
            roots.iter().any(|root| is_within(form, root))
                || (cfg!(unix) && homes.iter().any(|home| in_hidden_folder(form, home)))
        })
    }

    /// Whether `path` is outside the home folder and every known folder (which
    /// may be moved elsewhere, such as Downloads on another drive): worth a
    /// second look before a settings file sends downloads there.
    #[must_use]
    pub fn is_outside_user_folders(&self, path: &Path) -> bool {
        !self
            .placeholders()
            .iter()
            .any(|(_, folder)| is_within(path, folder))
    }

    /// The folder `path` names on this machine.
    ///
    /// # Errors
    ///
    /// A [`PathError`] when `path` is not a full path on this machine, names an unknown
    /// `{Folder}` placeholder, climbs out of its folder with `..`, has a part after the
    /// placeholder that is not a plain folder name, or is a Windows device or verbatim
    /// path.
    pub fn resolve(&self, path: &PortablePath) -> Result<PathBuf, PathError> {
        let raw = path.0.trim();
        let Some(rest) = raw.strip_prefix('{') else {
            return Self::resolve_plain(raw);
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
    /// rather than the user's own choice: network folders are refused too, and
    /// so are startup and system folders (see [`is_protected`](Self::is_protected)).
    /// Blocks on the disk.
    ///
    /// # Errors
    ///
    /// The errors of [`resolve`](Self::resolve), and [`PathError::Network`] for a
    /// network folder written as a plain path or [`PathError::Protected`] for a startup
    /// or system folder.
    pub fn resolve_local(&self, path: &PortablePath) -> Result<PathBuf, PathError> {
        let resolved = self.resolve(path)?;
        // A placeholder names this user's own folder, wherever the system keeps it
        // (Downloads redirected to a server is still the user's Downloads).
        // Checked first, so a network folder from a file is never contacted.
        let placeholder = path.0.trim_start().starts_with('{');
        if !placeholder && is_network(&resolved) {
            return Err(PathError::Network(path.0.trim().to_string()));
        }
        if self.is_protected(&resolved) {
            return Err(PathError::Protected(path.0.trim().to_string()));
        }
        Ok(resolved)
    }

    /// `path` written relative to the most specific known folder that contains it,
    /// so it resolves correctly on another machine. Placeholders and paths outside
    /// every known folder are returned unchanged.
    #[must_use]
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

    fn resolve_plain(raw: &str) -> Result<PathBuf, PathError> {
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
        // Rebuilt from its parts, so a folder written with `/` (in settings or a profile)
        // uses the platform's separator throughout, and paths made from it are never mixed.
        Ok(path.components().collect())
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

/// The folders on Windows where a file runs by itself or shows up as a program:
/// the Start menu of the user and of everyone (with the Startup folders in
/// them), the folder of app shortcuts on the user's `PATH`, Windows itself and
/// the program folders. `env` reads an environment variable (absolute paths
/// only); the usual places are used where one is missing.
#[cfg(windows)]
fn protected_folders(home: &Path, env: impl Fn(&str) -> Option<PathBuf>) -> Vec<PathBuf> {
    let roaming = env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
    let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
    let everyone = env("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let start_menu = Path::new("Microsoft").join("Windows").join("Start Menu");
    let mut out = vec![
        roaming.join(&start_menu),
        everyone.join(&start_menu),
        local.join("Microsoft").join("WindowsApps"),
        env("SystemRoot")
            .or_else(|| env("windir"))
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows")),
        PathBuf::from(r"C:\Program Files"),
        PathBuf::from(r"C:\Program Files (x86)"),
    ];
    out.extend(
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(|name| env(name)),
    );
    out.dedup();
    out
}

/// The same for Linux: autostart entries, application entries (menus and file
/// handlers) and systemd user units, wherever `XDG_CONFIG_HOME` and
/// `XDG_DATA_HOME` put them, and the system's own folders. Hidden folders in
/// home (`~/.config`, `~/.local`, `~/.ssh` and the like) are refused on top of
/// these by [`KnownFolders::is_protected`].
#[cfg(not(windows))]
fn protected_folders(home: &Path, env: impl Fn(&str) -> Option<PathBuf>) -> Vec<PathBuf> {
    let config = env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let data = env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
    let mut out = vec![
        config.join("autostart"),
        config.join("systemd"),
        data.join("applications"),
    ];
    out.extend(
        ["/etc", "/usr", "/bin", "/sbin", "/lib", "/lib64", "/boot"]
            .iter()
            .map(PathBuf::from),
    );
    out
}

/// Whether `path` is on a network share (`\\server\share`), on Windows.
fn is_network(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    )
}

/// The real path of `path`: its nearest existing folder with links, short
/// names and case resolved, and the rest of `path` after it. `None` when no
/// part of it exists, and for a network path, which is never contacted for
/// this. Blocks on the disk.
fn real_path(path: &Path) -> Option<PathBuf> {
    if is_network(path) {
        return None;
    }
    let mut rest = Vec::new();
    let mut current = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(current) {
            let mut out = real;
            out.extend(rest.iter().rev());
            return Some(out);
        }
        rest.push(current.file_name()?.to_os_string());
        current = current.parent()?;
    }
}

/// Whether `path` is `root` or inside it. Windows ignores case in names and
/// trailing dots and spaces, so there they are compared that way.
fn is_within(path: &Path, root: &Path) -> bool {
    if cfg!(windows) {
        let parts = |p: &Path| -> Vec<String> {
            p.components()
                .filter(|c| !matches!(c, Component::RootDir | Component::CurDir))
                .map(|c| {
                    c.as_os_str()
                        .to_string_lossy()
                        .trim_end_matches(['.', ' '])
                        .to_lowercase()
                })
                .collect()
        };
        let (path, root) = (parts(path), parts(root));
        !root.is_empty() && path.len() >= root.len() && path[..root.len()] == root[..]
    } else {
        path.starts_with(root)
    }
}

/// Whether `path` is inside a hidden folder of `home` (a name starting with
/// `.` right under it), where Linux keeps settings, autostart entries and
/// shell start-up files.
fn in_hidden_folder(path: &Path, home: &Path) -> bool {
    path.strip_prefix(home).is_ok_and(|rest| {
        rest.components()
            .next()
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    })
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

    #[cfg(windows)]
    #[test]
    fn a_folder_written_with_slashes_uses_backslashes_on_windows() {
        let folders = KnownFolders::under_home(home());
        let resolved = folders.resolve(&portable("D:/Media/Clips/")).unwrap();
        assert_eq!(resolved.to_str(), Some(r"D:\Media\Clips"));
        let file = resolved.join("a.mp4");
        assert!(!file.to_string_lossy().contains('/'), "{}", file.display());
        // Mixed already: made uniform.
        let mixed = folders.resolve(&portable(r"D:/Media\Clips")).unwrap();
        assert_eq!(mixed.to_str(), Some(r"D:\Media\Clips"));
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

    fn refused(folders: &KnownFolders, path: &str) -> bool {
        matches!(
            folders.resolve_local(&portable(path)),
            Err(PathError::Protected(_))
        )
    }

    #[cfg(windows)]
    #[test]
    fn startup_and_system_folders_from_a_file_are_refused_on_windows() {
        let folders = KnownFolders::under_home(home());
        for path in [
            "{Home}/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup",
            "{Home}/AppData/Roaming/Microsoft/Windows/Start Menu/Programs",
            "{Home}/AppData/Roaming/Microsoft/Windows/Start Menu",
            r"C:\Users\ada\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp",
            "{Home}/AppData/Local/Microsoft/WindowsApps",
            r"C:\Windows",
            r"C:\Windows\System32",
            r"c:\windows\system32",
            r"C:\Windows.\Temp",
            r"C:\Program Files\Wirecatch",
            r"C:\PROGRAM FILES (X86)",
        ] {
            assert!(refused(&folders, path), "{path} was allowed");
        }
        let error = folders
            .resolve_local(&portable(r"C:\Windows"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("system or startup folder"), "{error}");
        // The desktop and ordinary folders are normal places.
        for path in [
            "{Home}/Desktop",
            "{Downloads}/Video",
            "{Home}/AppData/Roaming/Other",
            r"D:\Media",
            r"C:\Windows Media",
        ] {
            assert!(folders.resolve_local(&portable(path)).is_ok(), "{path}");
        }
        // Where the system says the folders are counts.
        let moved = protected_folders(&home(), |name| match name {
            "APPDATA" => Some(PathBuf::from(r"E:\Roaming")),
            "SystemRoot" => Some(PathBuf::from(r"D:\WinDir")),
            _ => None,
        });
        assert!(moved.contains(&PathBuf::from(r"E:\Roaming\Microsoft\Windows\Start Menu")));
        assert!(moved.contains(&PathBuf::from(r"D:\WinDir")));
    }

    /// A short (8.3) name for a protected folder is caught through the real
    /// path, when this computer has short names.
    #[cfg(windows)]
    #[test]
    fn a_short_name_does_not_get_around_the_check() {
        let Some(windows) = std::env::var_os("SystemRoot").map(PathBuf::from) else {
            return;
        };
        let folders = KnownFolders {
            protected: vec![windows.join("System32")],
            ..KnownFolders::under_home(home())
        };
        // The same folder written differently: different case and a trailing dot.
        let odd = format!("{}.\\Drivers", windows.join("SYSTEM32").to_string_lossy());
        assert!(folders.is_protected(Path::new(&odd)), "{odd}");
        let short = Path::new(r"C:\PROGRA~1");
        if std::fs::canonicalize(short)
            .is_ok_and(|p| p.to_string_lossy().ends_with("Program Files"))
        {
            let program_files = KnownFolders {
                protected: vec![PathBuf::from(r"C:\Program Files")],
                ..KnownFolders::under_home(home())
            };
            assert!(program_files.is_protected(&short.join("Evil")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn autostart_and_system_folders_from_a_file_are_refused_on_linux() {
        let folders = KnownFolders::under_home(home());
        for path in [
            "{Home}/.config/autostart",
            "{Home}/.config/systemd/user",
            "{Home}/.local/share/applications",
            "{Home}/.bashrc.d",
            "{Home}/.ssh",
            "{Home}/.profile.d/x",
            "/home/ada/.config",
            "/etc",
            "/etc/profile.d",
            "/usr/local/bin",
        ] {
            assert!(refused(&folders, path), "{path} was allowed");
        }
        for path in [
            "{Home}/Desktop",
            "{Downloads}/Video",
            "{Home}/Videos/.thumbnails",
            "/mnt/media",
            "/home/ada/Projects",
        ] {
            assert!(folders.resolve_local(&portable(path)).is_ok(), "{path}");
        }
        let moved = protected_folders(&home(), |name| match name {
            "XDG_CONFIG_HOME" => Some(PathBuf::from("/data/config")),
            _ => None,
        });
        assert!(moved.contains(&PathBuf::from("/data/config/autostart")));
    }

    /// A link that points into a protected folder is followed.
    #[cfg(unix)]
    #[test]
    fn a_link_into_a_protected_folder_is_followed() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let autostart = home.join(".config").join("autostart");
        std::fs::create_dir_all(&autostart).unwrap();
        std::os::unix::fs::symlink(&autostart, home.join("Innocent")).unwrap();
        let folders = KnownFolders::under_home(&home);
        assert!(refused(&folders, "{Home}/Innocent"));
        assert!(refused(&folders, "{Home}/Innocent/New"));
    }

    #[test]
    fn folders_outside_the_persons_own_are_flagged() {
        let mut folders = KnownFolders::under_home(home());
        let elsewhere = if cfg!(windows) {
            r"D:\Media"
        } else {
            "/mnt/media"
        };
        assert!(folders.is_outside_user_folders(Path::new(elsewhere)));
        assert!(!folders.is_outside_user_folders(&home().join("Projects")));
        assert!(!folders.is_outside_user_folders(&folders.videos.join("Talks")));
        // Downloads moved to another drive is still the person's own.
        folders.downloads = PathBuf::from(elsewhere).join("Downloads");
        assert!(!folders.is_outside_user_folders(&folders.downloads.join("x")));
    }
}
