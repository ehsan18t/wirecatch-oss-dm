//! Every supported browser and where it looks for native messaging hosts.

/// Browser engines, which decide the manifest format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Engine {
    /// Manifest lists `allowed_origins`.
    Chromium,
    /// Manifest lists `allowed_extensions`.
    Firefox,
}

/// One supported browser and where it looks for hosts.
pub(super) struct Browser {
    pub(super) name: &'static str,
    pub(super) engine: Engine,
    /// Windows: the key under HKCU whose `NativeMessagingHosts` subkey the
    /// browser reads; None when it has no Windows version.
    pub(super) windows_root: Option<&'static str>,
    /// Linux: the per-user hosts folder it reads; None when it has no Linux
    /// version.
    pub(super) linux: Option<LinuxHosts>,
}

/// A per-user hosts folder on Linux.
pub(super) struct LinuxHosts {
    /// Where the browser's folder lives.
    pub(super) base: LinuxBase,
    /// The folder below that base, as path components, ending with the hosts
    /// folder itself.
    pub(super) dir: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinuxBase {
    /// `$XDG_CONFIG_HOME`, or `~/.config`.
    ConfigHome,
    /// The home folder.
    Home,
}

/// Google Chrome's key. Upstream Chromium reads only this one, except builds
/// with Chromium branding, which read [`CHROMIUM_KEY`] first and fall back to it
/// (`chrome/browser/extensions/api/messaging/launch_context_win.cc`,
/// `GetManifestPathWithFlags`). Forks that do not change that file (Brave,
/// Vivaldi, Opera, Arc, Thorium, ungoogled-chromium) read it too.
pub(super) const CHROME_KEY: &str = r"Software\Google\Chrome";
/// Read first by builds with Chromium branding (Chromium, Brave, Thorium,
/// ungoogled-chromium), before [`CHROME_KEY`].
pub(super) const CHROMIUM_KEY: &str = r"Software\Chromium";
/// Firefox's key (`REGPATH` in
/// `toolkit/components/extensions/NativeManifests.sys.mjs`). The Firefox forks
/// below keep it unchanged.
pub(super) const MOZILLA_KEY: &str = r"Software\Mozilla";

/// Chromium's per-user hosts folder on Linux: `NativeMessagingHosts` inside the
/// browser's user data folder (`chrome::DIR_USER_NATIVE_MESSAGING`).
pub(super) const fn chromium_linux(dir: &'static [&'static str]) -> LinuxHosts {
    LinuxHosts {
        base: LinuxBase::ConfigHome,
        dir,
    }
}

/// Firefox's per-user hosts folder on Linux: `~/.mozilla/native-messaging-hosts`
/// (`XRE_USER_NATIVE_MANIFESTS` in `toolkit/xre/nsXREDirProvider.cpp`, which keeps
/// this legacy path even for profiles in the XDG folders).
pub(super) const MOZILLA_LINUX: LinuxHosts = LinuxHosts {
    base: LinuxBase::Home,
    dir: &[".mozilla", "native-messaging-hosts"],
};

/// The browsers `install` registers with, and where each one really looks.
/// Entries that share a key or folder are written once.
pub(super) const BROWSERS: &[Browser] = &[
    Browser {
        name: "Google Chrome",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: Some(chromium_linux(&["google-chrome", "NativeMessagingHosts"])),
    },
    Browser {
        name: "Chromium",
        engine: Engine::Chromium,
        windows_root: Some(CHROMIUM_KEY),
        linux: Some(chromium_linux(&["chromium", "NativeMessagingHosts"])),
    },
    // Edge reads its own key first, then Chromium's and Chrome's: "Native
    // messaging", Microsoft Edge developer documentation (learn.microsoft.com).
    Browser {
        name: "Microsoft Edge",
        engine: Engine::Chromium,
        windows_root: Some(r"Software\Microsoft\Edge"),
        linux: Some(chromium_linux(&["microsoft-edge", "NativeMessagingHosts"])),
    },
    // Brave does not change the Windows lookup, so it reads Chromium's and
    // Chrome's keys; on Linux it keeps its own user data folder
    // (brave-core `app/brave_main_delegate.cc`, `PreSandboxStartup`).
    Browser {
        name: "Brave",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: Some(chromium_linux(&[
            "BraveSoftware",
            "Brave-Browser",
            "NativeMessagingHosts",
        ])),
    },
    // Vivaldi reads Chrome's key on Windows (KeePassXC moved its Vivaldi entry
    // there in commit 2ad8036: "Vivaldi uses Chrome's registry settings").
    Browser {
        name: "Vivaldi",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: Some(chromium_linux(&["vivaldi", "NativeMessagingHosts"])),
    },
    Browser {
        name: "Opera",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: Some(chromium_linux(&["opera", "NativeMessagingHosts"])),
    },
    // Opera GX and Arc have no Linux version.
    Browser {
        name: "Opera GX",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: None,
    },
    Browser {
        name: "Arc",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: None,
    },
    // Thorium's Linux user data folder is `thorium`
    // (Alex313031/thorium `src/chrome/common/chrome_paths_linux.cc`).
    Browser {
        name: "Thorium",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: Some(chromium_linux(&["thorium", "NativeMessagingHosts"])),
    },
    // ungoogled-chromium keeps Chromium's names everywhere.
    Browser {
        name: "ungoogled-chromium",
        engine: Engine::Chromium,
        windows_root: Some(CHROMIUM_KEY),
        linux: Some(chromium_linux(&["chromium", "NativeMessagingHosts"])),
    },
    // Yandex Browser on Linux: `yandex-browser` (browserpass-native's
    // `hosts-yandex-user` target, Video DownloadHelper's companion app). Its
    // closed Windows build could not be checked, so no key is written for it
    // there beyond the ones above.
    Browser {
        name: "Yandex Browser",
        engine: Engine::Chromium,
        windows_root: None,
        linux: Some(chromium_linux(&["yandex-browser", "NativeMessagingHosts"])),
    },
    Browser {
        name: "Mozilla Firefox",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: Some(MOZILLA_LINUX),
    },
    // LibreWolf reads `~/.librewolf/native-messaging-hosts` and then Firefox's
    // folder, and keeps Firefox's key (librewolf/source `patches/mozilla_dirs.patch`).
    Browser {
        name: "LibreWolf",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: Some(MOZILLA_LINUX),
    },
    // Waterfox reads `~/.waterfox/native-messaging-hosts` and then Firefox's
    // folder (`COMPAT_DIR_NAMES` in its `NativeManifests.sys.mjs`), and keeps
    // Firefox's key.
    Browser {
        name: "Waterfox",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: Some(MOZILLA_LINUX),
    },
    // Zen changes where profiles live, not where native manifests are read
    // (zen-browser/desktop `src/toolkit/xre/nsXREDirProvider-cpp.patch`).
    Browser {
        name: "Zen",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: Some(MOZILLA_LINUX),
    },
    // Floorp maps its own folder names back to Mozilla's
    // (Floorp `tools/patches/modules-NativeManifests.sys.patch`).
    Browser {
        name: "Floorp",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: Some(MOZILLA_LINUX),
    },
];

/// Keys earlier versions wrote for browsers that never read them (Brave and
/// Vivaldi read Chrome's key, see [`BROWSERS`]). `install` and `uninstall` remove
/// them, by the same rule as every other key, when they point to this copy's
/// Chromium manifest.
pub(super) const RETIRED_WINDOWS_ROOTS: &[&str] =
    &[r"Software\BraveSoftware\Brave-Browser", r"Software\Vivaldi"];
