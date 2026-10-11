//! `wirecatch-bridge install` and `uninstall`: register the bridge as a native
//! messaging host in every supported browser, for the current user or, on Windows, for every user of the computer (`--all-users`).
//!
//! The per-machine installer registers for all users (D25): the manifests go to a `NativeMessagingHosts` folder next to the bridge in `Program Files`, which only administrators can change, and the keys go under `HKEY_LOCAL_MACHINE`. A per-user `install`, which the app runs on every start, then does nothing for a bridge the machine registration already covers, so an uninstall never leaves other users with a registration pointing at removed files. Without a machine registration (a portable copy, Linux) everything is per user and needs no administrator rights.
//!
//! - **Windows**: two manifests (one for Chromium browsers, one for Firefox) are
//!   written to `%LOCALAPPDATA%\Wirecatch\NativeMessagingHosts`, and each browser
//!   gets a key under `HKEY_CURRENT_USER` whose default value is the manifest
//!   path. The per-user data folder is used rather than the folder next to the
//!   executable because a per-machine install puts the executable under
//!   `Program Files`, which a normal user cannot write to.
//! - **Linux**: a manifest is written into each browser's per-user
//!   `NativeMessagingHosts` folder (honoring `XDG_CONFIG_HOME` the way Chromium
//!   browsers do) and Firefox's `~/.mozilla/native-messaging-hosts`.
//!
//! Many browsers read another browser's location instead of their own (see
//! [`BROWSERS`]), so one manifest or key often serves several browsers. A
//! location is only written when a browser really reads it.
//!
//! Sandboxed Linux browsers: a Flatpak or Snap browser reads manifests inside its
//! sandbox, but the bridge they name is installed outside it, where the sandbox
//! cannot start it, so no manifest is written there. Firefox as a Snap (Ubuntu's
//! default) and as a Flatpak can still reach the bridge through the
//! WebExtensions portal (`xdg-desktop-portal`), which reads the host's own
//! `~/.mozilla/native-messaging-hosts` and asks the person once; this needs
//! portal support in the distribution. Chromium-based Flatpaks and Snaps cannot
//! start a host outside their sandbox; use a packaged (.deb, .rpm) browser.
//!
//! `uninstall` removes a manifest only if it still holds exactly what `install`
//! would write from this copy of the bridge, and a registry key only if it points
//! to such a manifest (or to one that no longer exists). Every copy installed for
//! one user writes the same manifest paths, so the manifest contents (which name
//! the executable) are what tell the copies apart; a registration that belongs to
//! another copy is never removed.
//!
//! Afterwards it removes the `NativeMessagingHosts` folder or key it wrote into
//! when that is left empty, and nothing above it: the browser folders and keys
//! above may belong to the browser even when empty. Removing an empty folder is a
//! single atomic step. For a key, the only realistic concurrent change is another
//! host registering a subkey, which makes the delete fail instead of losing it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Text shown in each manifest.
const DESCRIPTION: &str = "Wirecatch browser bridge";

/// Browser engines, which decide the manifest format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    /// Manifest lists `allowed_origins`.
    Chromium,
    /// Manifest lists `allowed_extensions`.
    Firefox,
}

/// One supported browser and where it looks for hosts.
struct Browser {
    name: &'static str,
    engine: Engine,
    /// Windows: the key under HKCU whose `NativeMessagingHosts` subkey the
    /// browser reads; None when it has no Windows version.
    windows_root: Option<&'static str>,
    /// Linux: the per-user hosts folder it reads; None when it has no Linux
    /// version.
    linux: Option<LinuxHosts>,
}

/// A per-user hosts folder on Linux.
struct LinuxHosts {
    /// Where the browser's folder lives.
    base: LinuxBase,
    /// The folder below that base, as path components, ending with the hosts
    /// folder itself.
    dir: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxBase {
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
const CHROME_KEY: &str = r"Software\Google\Chrome";
/// Read first by builds with Chromium branding (Chromium, Brave, Thorium,
/// ungoogled-chromium), before [`CHROME_KEY`].
const CHROMIUM_KEY: &str = r"Software\Chromium";
/// Firefox's key (`REGPATH` in
/// `toolkit/components/extensions/NativeManifests.sys.mjs`). The Firefox forks
/// below keep it unchanged.
const MOZILLA_KEY: &str = r"Software\Mozilla";

/// Chromium's per-user hosts folder on Linux: `NativeMessagingHosts` inside the
/// browser's user data folder (`chrome::DIR_USER_NATIVE_MESSAGING`).
const fn chromium_linux(dir: &'static [&'static str]) -> Option<LinuxHosts> {
    Some(LinuxHosts {
        base: LinuxBase::ConfigHome,
        dir,
    })
}

/// Firefox's per-user hosts folder on Linux: `~/.mozilla/native-messaging-hosts`
/// (`XRE_USER_NATIVE_MANIFESTS` in `toolkit/xre/nsXREDirProvider.cpp`, which keeps
/// this legacy path even for profiles in the XDG folders).
const MOZILLA_LINUX: Option<LinuxHosts> = Some(LinuxHosts {
    base: LinuxBase::Home,
    dir: &[".mozilla", "native-messaging-hosts"],
});

/// The browsers `install` registers with, and where each one really looks.
/// Entries that share a key or folder are written once.
const BROWSERS: &[Browser] = &[
    Browser {
        name: "Google Chrome",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: chromium_linux(&["google-chrome", "NativeMessagingHosts"]),
    },
    Browser {
        name: "Chromium",
        engine: Engine::Chromium,
        windows_root: Some(CHROMIUM_KEY),
        linux: chromium_linux(&["chromium", "NativeMessagingHosts"]),
    },
    // Edge reads its own key first, then Chromium's and Chrome's: "Native
    // messaging", Microsoft Edge developer documentation (learn.microsoft.com).
    Browser {
        name: "Microsoft Edge",
        engine: Engine::Chromium,
        windows_root: Some(r"Software\Microsoft\Edge"),
        linux: chromium_linux(&["microsoft-edge", "NativeMessagingHosts"]),
    },
    // Brave does not change the Windows lookup, so it reads Chromium's and
    // Chrome's keys; on Linux it keeps its own user data folder
    // (brave-core `app/brave_main_delegate.cc`, `PreSandboxStartup`).
    Browser {
        name: "Brave",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: chromium_linux(&["BraveSoftware", "Brave-Browser", "NativeMessagingHosts"]),
    },
    // Vivaldi reads Chrome's key on Windows (KeePassXC moved its Vivaldi entry
    // there in commit 2ad8036: "Vivaldi uses Chrome's registry settings").
    Browser {
        name: "Vivaldi",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: chromium_linux(&["vivaldi", "NativeMessagingHosts"]),
    },
    Browser {
        name: "Opera",
        engine: Engine::Chromium,
        windows_root: Some(CHROME_KEY),
        linux: chromium_linux(&["opera", "NativeMessagingHosts"]),
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
        linux: chromium_linux(&["thorium", "NativeMessagingHosts"]),
    },
    // ungoogled-chromium keeps Chromium's names everywhere.
    Browser {
        name: "ungoogled-chromium",
        engine: Engine::Chromium,
        windows_root: Some(CHROMIUM_KEY),
        linux: chromium_linux(&["chromium", "NativeMessagingHosts"]),
    },
    // Yandex Browser on Linux: `yandex-browser` (browserpass-native's
    // `hosts-yandex-user` target, Video DownloadHelper's companion app). Its
    // closed Windows build could not be checked, so no key is written for it
    // there beyond the ones above.
    Browser {
        name: "Yandex Browser",
        engine: Engine::Chromium,
        windows_root: None,
        linux: chromium_linux(&["yandex-browser", "NativeMessagingHosts"]),
    },
    Browser {
        name: "Mozilla Firefox",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: MOZILLA_LINUX,
    },
    // LibreWolf reads `~/.librewolf/native-messaging-hosts` and then Firefox's
    // folder, and keeps Firefox's key (librewolf/source `patches/mozilla_dirs.patch`).
    Browser {
        name: "LibreWolf",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: MOZILLA_LINUX,
    },
    // Waterfox reads `~/.waterfox/native-messaging-hosts` and then Firefox's
    // folder (`COMPAT_DIR_NAMES` in its `NativeManifests.sys.mjs`), and keeps
    // Firefox's key.
    Browser {
        name: "Waterfox",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: MOZILLA_LINUX,
    },
    // Zen changes where profiles live, not where native manifests are read
    // (zen-browser/desktop `src/toolkit/xre/nsXREDirProvider-cpp.patch`).
    Browser {
        name: "Zen",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: MOZILLA_LINUX,
    },
    // Floorp maps its own folder names back to Mozilla's
    // (Floorp `tools/patches/modules-NativeManifests.sys.patch`).
    Browser {
        name: "Floorp",
        engine: Engine::Firefox,
        windows_root: Some(MOZILLA_KEY),
        linux: MOZILLA_LINUX,
    },
];

/// Keys earlier versions wrote for browsers that never read them (Brave and
/// Vivaldi read Chrome's key, see [`BROWSERS`]). `install` and `uninstall` remove
/// them, by the same rule as every other key, when they point to this copy's
/// Chromium manifest.
const RETIRED_WINDOWS_ROOTS: &[&str] =
    &[r"Software\BraveSoftware\Brave-Browser", r"Software\Vivaldi"];

/// Where things go on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Windows paths.
    Windows {
        /// The bridge executable the manifests point to.
        exe: PathBuf,
        /// The per-user local app data folder (`%LOCALAPPDATA%`).
        local_app_data: PathBuf,
    },
    /// Windows, for every user of the computer: manifests next to the bridge, keys under `HKEY_LOCAL_MACHINE`. Writing it needs administrator rights.
    WindowsMachine {
        /// The bridge executable the manifests point to.
        exe: PathBuf,
    },
    /// Linux paths.
    Linux {
        /// The bridge executable the manifests point to.
        exe: PathBuf,
        /// The home folder.
        home: PathBuf,
        /// `$XDG_CONFIG_HOME`, or `~/.config` when unset.
        config_home: PathBuf,
    },
}

impl Layout {
    /// The layout for the current user and this executable.
    pub(crate) fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("Could not find the bridge executable")?;
        if cfg!(windows) {
            let local_app_data = absolute_env("LOCALAPPDATA")
                .context("The LOCALAPPDATA environment variable is missing or not a full path")?;
            Ok(Self::Windows {
                exe,
                local_app_data,
            })
        } else {
            let home = absolute_env("HOME")
                .context("The HOME environment variable is missing or not a full path")?;
            Ok(Self::linux(exe, home, absolute_env("XDG_CONFIG_HOME")))
        }
    }

    /// A Linux layout. `config_home` falls back to `~/.config`.
    pub(crate) fn linux(exe: PathBuf, home: PathBuf, config_home: Option<PathBuf>) -> Self {
        let config_home = config_home.unwrap_or_else(|| home.join(".config"));
        Self::Linux {
            exe,
            home,
            config_home,
        }
    }

    /// The layout for every user of this computer and this executable (Windows only).
    pub(crate) fn current_machine() -> anyhow::Result<Self> {
        if !cfg!(windows) {
            bail!(
                "Registering for every user is only needed on Windows; on Linux the packages and each user's first start register the bridge"
            );
        }
        let exe = std::env::current_exe().context("Could not find the bridge executable")?;
        Ok(Self::WindowsMachine { exe })
    }

    fn exe(&self) -> &Path {
        match self {
            Self::Windows { exe, .. } | Self::WindowsMachine { exe } | Self::Linux { exe, .. } => {
                exe
            }
        }
    }
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// A manifest file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestFile {
    pub(crate) path: PathBuf,
    pub(crate) contents: String,
    pub(crate) browsers: Vec<&'static str>,
    /// The `NativeMessagingHosts` folder holding it; `uninstall` removes it when
    /// it is left empty.
    pub(crate) hosts_dir: PathBuf,
}

/// The registry root a key lives under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Hive {
    /// `HKEY_CURRENT_USER`: the current user only.
    CurrentUser,
    /// `HKEY_LOCAL_MACHINE`: every user of the computer.
    LocalMachine,
}

impl Hive {
    /// The root's name as Windows tools show it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKEY_CURRENT_USER",
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
        }
    }
}

/// A registry key whose default value is a manifest path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryEntry {
    pub(crate) hive: Hive,
    pub(crate) key: String,
    /// The manifest path.
    pub(crate) value: String,
    /// What this copy writes into that manifest.
    pub(crate) manifest_contents: String,
    pub(crate) browsers: Vec<&'static str>,
    /// The `NativeMessagingHosts` key holding it; `uninstall` removes it when it
    /// is left empty.
    pub(crate) hosts_key: String,
}

/// Everything `install` writes and `uninstall` removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) files: Vec<ManifestFile>,
    pub(crate) keys: Vec<RegistryEntry>,
    /// Keys earlier versions wrote that no browser reads; removed, never written.
    pub(crate) retired_keys: Vec<RegistryEntry>,
}

#[derive(Serialize)]
struct ChromiumManifest<'a> {
    name: &'a str,
    description: &'a str,
    path: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    allowed_origins: Vec<String>,
}

#[derive(Serialize)]
struct FirefoxManifest<'a> {
    name: &'a str,
    description: &'a str,
    path: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    allowed_extensions: &'a [String],
}

fn manifest(config: &Config, exe: &str, engine: Engine) -> anyhow::Result<String> {
    let mut text = match engine {
        Engine::Chromium => serde_json::to_string_pretty(&ChromiumManifest {
            name: &config.native_host_name,
            description: DESCRIPTION,
            path: exe,
            kind: "stdio",
            allowed_origins: config.chromium_origins(),
        })?,
        Engine::Firefox => serde_json::to_string_pretty(&FirefoxManifest {
            name: &config.native_host_name,
            description: DESCRIPTION,
            path: exe,
            kind: "stdio",
            allowed_extensions: &config.firefox_ids,
        })?,
    };
    text.push('\n');
    Ok(text)
}

/// Works out every file and key for `layout`.
pub(crate) fn plan(config: &Config, layout: &Layout) -> anyhow::Result<Plan> {
    let exe = layout.exe();
    if !exe.is_absolute() {
        bail!("The bridge path {} is not a full path", exe.display());
    }
    let exe_text = exe.to_str().with_context(|| {
        format!(
            "The bridge path {} contains characters a manifest cannot hold. Install Wirecatch in a folder with a plain name",
            exe.display()
        )
    })?;
    let host = &config.native_host_name;

    let mut plan = Plan {
        files: Vec::new(),
        keys: Vec::new(),
        retired_keys: Vec::new(),
    };
    match layout {
        Layout::Windows { local_app_data, .. } => {
            let dir = local_app_data
                .join("Wirecatch")
                .join("NativeMessagingHosts");
            plan_windows(&mut plan, config, exe_text, &dir, Hive::CurrentUser)?;
        }
        Layout::WindowsMachine { exe } => {
            let dir = exe
                .parent()
                .context("The bridge has no folder")?
                .join("NativeMessagingHosts");
            plan_windows(&mut plan, config, exe_text, &dir, Hive::LocalMachine)?;
        }
        Layout::Linux {
            home, config_home, ..
        } => {
            for browser in BROWSERS {
                let Some(linux) = &browser.linux else {
                    continue;
                };
                let base = match linux.base {
                    LinuxBase::ConfigHome => config_home,
                    LinuxBase::Home => home,
                };
                let mut dir = base.clone();
                dir.extend(linux.dir);
                let path = dir.join(format!("{host}.json"));
                if let Some(file) = plan.files.iter_mut().find(|f| f.path == path) {
                    file.browsers.push(browser.name);
                    continue;
                }
                plan.files.push(ManifestFile {
                    path,
                    contents: manifest(config, exe_text, browser.engine)?,
                    browsers: vec![browser.name],
                    hosts_dir: dir,
                });
            }
        }
    }
    Ok(plan)
}

/// The Windows part of [`plan`]: two manifests in `dir` and a key per browser under `hive`.
fn plan_windows(
    plan: &mut Plan,
    config: &Config,
    exe_text: &str,
    dir: &Path,
    hive: Hive,
) -> anyhow::Result<()> {
    let host = &config.native_host_name;
    for engine in [Engine::Chromium, Engine::Firefox] {
        let suffix = match engine {
            Engine::Chromium => "chromium",
            Engine::Firefox => "firefox",
        };
        let path = dir.join(format!("{host}.{suffix}.json"));
        let value = path
            .to_str()
            .with_context(|| format!("The folder {} has an unusual name", dir.display()))?
            .to_string();
        let contents = manifest(config, exe_text, engine)?;
        plan.files.push(ManifestFile {
            path,
            contents: contents.clone(),
            browsers: Vec::new(),
            hosts_dir: dir.to_path_buf(),
        });
        for browser in BROWSERS.iter().filter(|b| b.engine == engine) {
            let Some(root) = browser.windows_root else {
                continue;
            };
            if let Some(file) = plan.files.last_mut() {
                file.browsers.push(browser.name);
            }
            let hosts_key = format!(r"{root}\NativeMessagingHosts");
            let key = format!(r"{hosts_key}\{host}");
            if let Some(entry) = plan.keys.iter_mut().find(|e| e.key == key) {
                entry.browsers.push(browser.name);
                continue;
            }
            plan.keys.push(RegistryEntry {
                hive,
                key,
                value: value.clone(),
                manifest_contents: contents.clone(),
                browsers: vec![browser.name],
                hosts_key,
            });
        }
        if engine == Engine::Chromium {
            for root in RETIRED_WINDOWS_ROOTS {
                let hosts_key = format!(r"{root}\NativeMessagingHosts");
                plan.retired_keys.push(RegistryEntry {
                    hive,
                    key: format!(r"{hosts_key}\{host}"),
                    value: value.clone(),
                    manifest_contents: contents.clone(),
                    browsers: Vec::new(),
                    hosts_key,
                });
            }
        }
    }
    Ok(())
}

/// File system and registry access, behind a trait so tests can run against a
/// temporary folder and an in-memory registry.
pub(crate) trait Host {
    /// The file's contents, or `None` if it does not exist.
    fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;
    /// Whether a file exists at `path`.
    fn is_file(&self, path: &Path) -> bool;
    /// Writes the file, creating missing folders, replacing it atomically.
    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// Removes the file.
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    /// Removes the folder if it exists and is empty. Returns whether it did.
    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool>;
    /// The default value of a key, or `None` if the key or value is missing.
    fn registry_value(&self, hive: Hive, key: &str) -> io::Result<Option<String>>;
    /// Creates the key (and missing parents) and sets its default value.
    fn set_registry_value(&mut self, hive: Hive, key: &str, value: &str) -> io::Result<()>;
    /// Deletes the key, which must have no subkeys. Missing is fine.
    fn delete_registry_key(&mut self, hive: Hive, key: &str) -> io::Result<()>;
    /// Deletes the key if it exists with no subkeys and no values. Returns
    /// whether it did.
    fn delete_registry_key_if_empty(&mut self, hive: Hive, key: &str) -> io::Result<bool>;
}

/// The real file system and (on Windows) registry.
pub(crate) struct RealHost;

pub(crate) mod files {
    //! File operations shared by the real host and the test host.

    use super::*;

    pub(crate) fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        fs::create_dir_all(dir)?;
        let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
        temp_name.push(".tmp");
        let temp = dir.join(temp_name);
        let result = fs::write(&temp, contents).and_then(|()| fs::rename(&temp, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    pub(crate) fn remove_dir_if_empty(path: &Path) -> io::Result<bool> {
        // remove_dir only succeeds on an empty folder, so the check and the
        // removal are one step.
        match fs::remove_dir(path) {
            Ok(()) => Ok(true),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
}

impl Host for RealHost {
    fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        files::read(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
        files::write(path, contents)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool> {
        files::remove_dir_if_empty(path)
    }

    fn registry_value(&self, hive: Hive, key: &str) -> io::Result<Option<String>> {
        registry::value(hive, key)
    }

    fn set_registry_value(&mut self, hive: Hive, key: &str, value: &str) -> io::Result<()> {
        registry::set_value(hive, key, value)
    }

    fn delete_registry_key(&mut self, hive: Hive, key: &str) -> io::Result<()> {
        registry::delete_key(hive, key)
    }

    fn delete_registry_key_if_empty(&mut self, hive: Hive, key: &str) -> io::Result<bool> {
        registry::delete_key_if_empty(hive, key)
    }
}

#[cfg(not(windows))]
mod registry {
    //! There is no registry outside Windows; the Linux plan never uses it.

    use std::io;

    use super::Hive;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no registry on this system",
        )
    }

    pub(crate) fn value(_hive: Hive, _key: &str) -> io::Result<Option<String>> {
        Err(unsupported())
    }
    pub(crate) fn set_value(_hive: Hive, _key: &str, _value: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key(_hive: Hive, _key: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key_if_empty(_hive: Hive, _key: &str) -> io::Result<bool> {
        Err(unsupported())
    }
}

#[cfg(windows)]
mod registry {
    //! The registry (HKEY_CURRENT_USER or HKEY_LOCAL_MACHINE) through the Win32 API.

    use std::io;
    use std::ptr;

    use super::Hive;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteKeyW, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
    };

    fn root(hive: Hive) -> HKEY {
        match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(status: u32) -> io::Result<()> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    /// Closes the key on drop.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by us and is closed exactly once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    pub(crate) fn value(hive: Hive, key: &str) -> io::Result<Option<String>> {
        let key = wide(key);
        let mut buf: Vec<u16> = vec![0; 260];
        // The value can change size between calls; retry a few times.
        for _ in 0..4 {
            let mut size = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
            // SAFETY: `key` is NUL-terminated; `buf` is writable for `size` bytes.
            let status = unsafe {
                RegGetValueW(
                    root(hive),
                    key.as_ptr(),
                    ptr::null(),
                    RRF_RT_REG_SZ,
                    ptr::null_mut(),
                    buf.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    let units = (size as usize / 2).min(buf.len());
                    let text = &buf[..units];
                    let text = text.strip_suffix(&[0]).unwrap_or(text);
                    return String::from_utf16(text)
                        .map(Some)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
                }
                ERROR_FILE_NOT_FOUND => return Ok(None),
                ERROR_MORE_DATA => buf.resize(size as usize / 2 + 1, 0),
                other => return Err(io::Error::from_raw_os_error(other as i32)),
            }
        }
        Err(io::Error::other("the registry value kept changing size"))
    }

    pub(crate) fn set_value(hive: Hive, key: &str, value: &str) -> io::Result<()> {
        let key = wide(key);
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: `key` is NUL-terminated; on success `handle` is a key we own.
        check(unsafe {
            RegCreateKeyExW(
                root(hive),
                key.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &mut handle,
                ptr::null_mut(),
            )
        })?;
        let handle = Key(handle);
        let data = wide(value);
        let bytes = u32::try_from(data.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "value too long"))?;
        // SAFETY: `data` is a NUL-terminated UTF-16 string of `bytes` bytes.
        check(unsafe {
            RegSetValueExW(
                handle.0,
                ptr::null(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        })
    }

    pub(crate) fn delete_key(hive: Hive, key: &str) -> io::Result<()> {
        let key = wide(key);
        // SAFETY: `key` is NUL-terminated.
        match unsafe { RegDeleteKeyW(root(hive), key.as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            other => Err(io::Error::from_raw_os_error(other as i32)),
        }
    }

    pub(crate) fn delete_key_if_empty(hive: Hive, key: &str) -> io::Result<bool> {
        let wide_key = wide(key);
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: `wide_key` is NUL-terminated; on success `handle` is ours.
        match unsafe {
            RegOpenKeyExW(
                root(hive),
                wide_key.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            )
        } {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(false),
            other => return Err(io::Error::from_raw_os_error(other as i32)),
        }
        let handle = Key(handle);
        let mut subkeys = 0u32;
        let mut values = 0u32;
        // SAFETY: `handle` is open; every out pointer is either null or valid.
        check(unsafe {
            RegQueryInfoKeyW(
                handle.0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                &mut subkeys,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut values,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        })?;
        drop(handle);
        if subkeys != 0 || values != 0 {
            return Ok(false);
        }
        delete_key(hive, key)?;
        Ok(true)
    }
}

/// What happened, one line per item, for the person running the command.
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) lines: Vec<String>,
    pub(crate) failures: usize,
}

impl Report {
    fn ok(&mut self, line: String) {
        self.lines.push(line);
    }

    fn fail(&mut self, line: String) {
        self.failures += 1;
        self.lines.push(line);
    }
}

/// Writes every manifest and registry key in `plan`.
pub(crate) fn install(plan: &Plan, host: &mut dyn Host) -> Report {
    let mut report = Report::default();
    for file in &plan.files {
        match host.write_file(&file.path, file.contents.as_bytes()) {
            Ok(()) => report.ok(format!(
                "Wrote {} ({})",
                file.path.display(),
                file.browsers.join(", ")
            )),
            Err(e) => report.fail(format!("Could not write {}: {e}", file.path.display())),
        }
    }
    for entry in &plan.keys {
        let name = format!(r"{}\{}", entry.hive.name(), entry.key);
        match host.set_registry_value(entry.hive, &entry.key, &entry.value) {
            Ok(()) => report.ok(format!("Registered {name} ({})", entry.browsers.join(", "))),
            Err(e) => report.fail(format!("Could not register {name}: {e}")),
        }
    }
    // After the manifests, so a key an earlier version wrote is recognized as
    // this copy's by the manifest just written.
    for entry in &plan.retired_keys {
        remove_key(entry, host, &mut report);
    }
    report
}

/// Whether the machine registration in `machine` is in place for this very bridge: every key points at its manifest and every manifest holds what this bridge writes. Then the browsers of every user already reach this bridge.
pub(crate) fn covered_by_machine(machine: &Plan, host: &dyn Host) -> bool {
    let keys = machine.keys.iter().all(|entry| {
        matches!(host.registry_value(entry.hive, &entry.key), Ok(Some(value)) if value == entry.value)
    });
    let files = machine.files.iter().all(|file| {
        matches!(host.read_file(&file.path), Ok(Some(bytes)) if bytes == file.contents.as_bytes())
    });
    !machine.keys.is_empty() && keys && files
}

/// The per-user install the app runs on every start. When the per-machine installer already registered this bridge for every user, it removes any registration of its own for this user instead of adding one: a per-user copy would outlive an uninstall, which can only clean up the account that runs it. It also removes this account's registrations of bridges that no longer exist, which would hide the machine one.
pub(crate) fn install_for_user(user: &Plan, machine: Option<&Plan>, host: &mut dyn Host) -> Report {
    match machine {
        Some(machine) if covered_by_machine(machine, host) => {
            let mut report = uninstall(user, host);
            remove_dead_registrations(user, host, &mut report);
            report.ok(
                "Registered for every user of this computer; nothing to add for this account."
                    .to_owned(),
            );
            report
        }
        _ => install(user, host),
    }
}

/// Removes what [`install`] wrote from this copy of the bridge, leaving anything
/// that another copy wrote or that changed since alone.
pub(crate) fn uninstall(plan: &Plan, host: &mut dyn Host) -> Report {
    let mut report = Report::default();
    // Keys first, while the manifests they point to can still be compared.
    for entry in plan.keys.iter().chain(&plan.retired_keys) {
        remove_key(entry, host, &mut report);
    }
    for file in &plan.files {
        let shown = file.path.display();
        match host.read_file(&file.path) {
            Ok(None) => {}
            Ok(Some(bytes)) if bytes == file.contents.as_bytes() => {
                delete_manifest(file, host, &mut report);
            }
            Ok(Some(_)) => report.ok(format!(
                "Left {shown} in place: it was changed or written by another copy of Wirecatch"
            )),
            Err(e) => report.fail(format!("Could not read {shown}: {e}")),
        }
    }
    report
}

/// Removes a registry key if it is this copy's: it points to this copy's
/// manifest path, and that manifest holds what this copy writes or is gone.
fn remove_key(entry: &RegistryEntry, host: &mut dyn Host, report: &mut Report) {
    let name = format!(r"{}\{}", entry.hive.name(), entry.key);
    let value = match host.registry_value(entry.hive, &entry.key) {
        Ok(None) => return,
        Ok(Some(value)) => value,
        Err(e) => {
            report.fail(format!("Could not read {name}: {e}"));
            return;
        }
    };
    if value != entry.value {
        report.ok(format!(
            "Left {name} in place: it points to {value}, which this copy did not write"
        ));
        return;
    }
    // Every copy for this user writes the same manifest path, so the path
    // alone does not say whose registration this is; the manifest does.
    match host.read_file(Path::new(&entry.value)) {
        Ok(None) => {}
        Ok(Some(bytes)) if bytes == entry.manifest_contents.as_bytes() => {}
        Ok(Some(_)) => {
            report.ok(format!(
                "Left {name} in place: the manifest it points to belongs to another copy of Wirecatch"
            ));
            return;
        }
        Err(e) => {
            report.fail(format!("Could not read {}: {e}", entry.value));
            return;
        }
    }
    delete_key(entry, host, report);
}

/// Removes this account's registrations whose manifest names a bridge that no longer exists: left by a per-user install of an earlier version, or by a portable copy that was deleted. Browsers read the account's registration before the machine's, so such a leftover hides the machine registration and the extension cannot start any bridge. A registration of another copy that still exists is left alone: that copy chose it, and registers again on its next start.
fn remove_dead_registrations(user: &Plan, host: &mut dyn Host, report: &mut Report) {
    for file in &user.files {
        let bridge = match host.read_file(&file.path) {
            Ok(None) => continue,
            Ok(Some(bytes)) => manifest_bridge(&bytes),
            Err(e) => {
                report.fail(format!("Could not read {}: {e}", file.path.display()));
                continue;
            }
        };
        if bridge.is_some_and(|bridge| host.is_file(&bridge)) {
            continue;
        }
        // Keys first, like `uninstall`, so no browser is left pointing at a removed manifest.
        for entry in user
            .keys
            .iter()
            .filter(|e| Path::new(&e.value) == file.path)
        {
            if matches!(host.registry_value(entry.hive, &entry.key), Ok(Some(value)) if value == entry.value)
            {
                delete_key(entry, host, report);
            }
        }
        delete_manifest(file, host, report);
    }
}

/// The bridge a manifest names (its `path`), or `None` when it cannot be read as a manifest.
fn manifest_bridge(bytes: &[u8]) -> Option<PathBuf> {
    #[derive(Deserialize)]
    struct Named {
        path: PathBuf,
    }
    serde_json::from_slice::<Named>(bytes)
        .ok()
        .map(|named| named.path)
}

/// Removes a manifest, and its folder when that is left empty.
fn delete_manifest(file: &ManifestFile, host: &mut dyn Host, report: &mut Report) {
    let shown = file.path.display();
    match host.remove_file(&file.path) {
        Ok(()) => {
            report.ok(format!("Removed {shown}"));
            if let Err(e) = host.remove_dir_if_empty(&file.hosts_dir) {
                report.fail(format!("Could not tidy {}: {e}", file.hosts_dir.display()));
            }
        }
        Err(e) => report.fail(format!("Could not remove {shown}: {e}")),
    }
}

/// Removes a key, and its `NativeMessagingHosts` parent when that is left empty.
fn delete_key(entry: &RegistryEntry, host: &mut dyn Host, report: &mut Report) {
    let name = format!(r"{}\{}", entry.hive.name(), entry.key);
    match host.delete_registry_key(entry.hive, &entry.key) {
        Ok(()) => {
            report.ok(format!("Removed {name}"));
            if let Err(e) = host.delete_registry_key_if_empty(entry.hive, &entry.hosts_key) {
                report.fail(format!(
                    r"Could not tidy {}\{}: {e}",
                    entry.hive.name(),
                    entry.hosts_key
                ));
            }
        }
        Err(e) => report.fail(format!("Could not remove {name}: {e}")),
    }
}

/// Which command to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// `wirecatch-bridge install`
    Install,
    /// `wirecatch-bridge uninstall`
    Uninstall,
}

/// Whose registration a command changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The current user.
    User,
    /// Every user of the computer (`--all-users`, Windows, administrator rights).
    Machine,
}

/// Runs `install` or `uninstall` and prints what happened.
#[expect(
    clippy::print_stdout,
    reason = "a terminal command, not native messaging mode"
)]
pub(crate) fn run(command: Command, scope: Scope) -> ExitCode {
    let report = match planned_report(command, scope) {
        Ok(report) => report,
        Err(e) => {
            crate::log!("{e:#}");
            return ExitCode::FAILURE;
        }
    };
    for line in &report.lines {
        println!("{line}");
    }
    if report.failures > 0 {
        return ExitCode::FAILURE;
    }
    match (command, scope) {
        (Command::Install, Scope::User) => println!("The Wirecatch browser bridge is registered."),
        (Command::Install, Scope::Machine) => {
            println!("The Wirecatch browser bridge is registered for every user of this computer.");
        }
        (Command::Uninstall, _) => {
            println!("The Wirecatch browser bridge is no longer registered.")
        }
    }
    ExitCode::SUCCESS
}

fn planned_report(command: Command, scope: Scope) -> anyhow::Result<Report> {
    let config = Config::embedded()?;
    let report = match (command, scope) {
        (Command::Install, Scope::User) => {
            let user = plan(&config, &Layout::current()?)?;
            let machine = Layout::current_machine()
                .ok()
                .map(|layout| plan(&config, &layout))
                .transpose()?;
            install_for_user(&user, machine.as_ref(), &mut RealHost)
        }
        (Command::Uninstall, Scope::User) => {
            uninstall(&plan(&config, &Layout::current()?)?, &mut RealHost)
        }
        (Command::Install, Scope::Machine) => {
            install(&plan(&config, &Layout::current_machine()?)?, &mut RealHost)
        }
        (Command::Uninstall, Scope::Machine) => {
            uninstall(&plan(&config, &Layout::current_machine()?)?, &mut RealHost)
        }
    };
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A temporary folder removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "wirecatch-bridge-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Real files (inside a temporary folder) and an in-memory registry that
    /// behaves like HKCU: case-insensitive keys, parents created on demand,
    /// deleting a key with subkeys fails.
    #[derive(Default)]
    struct FakeHost {
        /// The current user's registry: lowercased key path to (original spelling, default value).
        registry: BTreeMap<String, (String, Option<String>)>,
        /// The machine's registry, the same way.
        machine: BTreeMap<String, (String, Option<String>)>,
    }

    impl FakeHost {
        fn hive(&self, hive: Hive) -> &BTreeMap<String, (String, Option<String>)> {
            match hive {
                Hive::CurrentUser => &self.registry,
                Hive::LocalMachine => &self.machine,
            }
        }

        fn hive_mut(&mut self, hive: Hive) -> &mut BTreeMap<String, (String, Option<String>)> {
            match hive {
                Hive::CurrentUser => &mut self.registry,
                Hive::LocalMachine => &mut self.machine,
            }
        }

        fn has_subkeys(&self, hive: Hive, key: &str) -> bool {
            let prefix = format!(r"{}\", key.to_lowercase());
            self.hive(hive).keys().any(|k| k.starts_with(&prefix))
        }

        /// The current user's registry.
        fn snapshot(&self) -> BTreeMap<String, Option<String>> {
            self.registry
                .values()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        }
    }

    impl Host for FakeHost {
        fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
            files::read(path)
        }
        fn is_file(&self, path: &Path) -> bool {
            path.is_file()
        }
        fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
            files::write(path, contents)
        }
        fn remove_file(&mut self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }
        fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool> {
            files::remove_dir_if_empty(path)
        }
        fn registry_value(&self, hive: Hive, key: &str) -> io::Result<Option<String>> {
            Ok(self
                .hive(hive)
                .get(&key.to_lowercase())
                .and_then(|(_, value)| value.clone()))
        }
        fn set_registry_value(&mut self, hive: Hive, key: &str, value: &str) -> io::Result<()> {
            let parts: Vec<&str> = key.split('\\').collect();
            let registry = self.hive_mut(hive);
            for depth in 1..parts.len() {
                let parent = parts[..depth].join("\\");
                registry
                    .entry(parent.to_lowercase())
                    .or_insert((parent, None));
            }
            registry.insert(
                key.to_lowercase(),
                (key.to_string(), Some(value.to_string())),
            );
            Ok(())
        }
        fn delete_registry_key(&mut self, hive: Hive, key: &str) -> io::Result<()> {
            if self.has_subkeys(hive, key) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            self.hive_mut(hive).remove(&key.to_lowercase());
            Ok(())
        }
        fn delete_registry_key_if_empty(&mut self, hive: Hive, key: &str) -> io::Result<bool> {
            let lower = key.to_lowercase();
            let empty = matches!(self.hive(hive).get(&lower), Some((_, None)))
                && !self.has_subkeys(hive, key);
            if empty {
                self.hive_mut(hive).remove(&lower);
            }
            Ok(empty)
        }
    }

    /// Every file under `root`, with its contents.
    fn tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    out.insert(rel, None);
                    stack.push(path);
                } else {
                    out.insert(rel, Some(fs::read(&path).unwrap()));
                }
            }
        }
        out
    }

    fn config() -> Config {
        Config::embedded().unwrap()
    }

    fn json(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    const ORIGIN: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";

    #[test]
    fn windows_install_registers_every_browser_and_uninstall_restores_the_state() {
        let temp = TempDir::new();
        let exe = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let local = temp.0.join("LocalAppData");
        let layout = Layout::Windows {
            exe: exe.clone(),
            local_app_data: local.clone(),
        };
        let plan = plan(&config(), &layout).unwrap();
        let mut host = FakeHost::default();

        // Things that were there before and must survive.
        fs::create_dir_all(local.join("Wirecatch")).unwrap();
        fs::write(local.join("Wirecatch").join("downloads.db"), b"keep").unwrap();
        host.set_registry_value(
            Hive::CurrentUser,
            r"Software\Google\Chrome\PreferenceMACs",
            "keep",
        )
        .unwrap();
        host.set_registry_value(
            Hive::CurrentUser,
            r"Software\Microsoft\Edge\BLBeacon",
            "keep",
        )
        .unwrap();
        // An empty browser key that was already there must not be removed.
        host.registry.insert(
            r"software\mozilla".into(),
            (r"Software\Mozilla".into(), None),
        );
        let files_before = tree(&temp.0);
        let registry_before = host.snapshot();

        let report = install(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);

        let dir = local.join("Wirecatch").join("NativeMessagingHosts");
        let chromium = dir.join("app.wirecatch.bridge.chromium.json");
        let firefox = dir.join("app.wirecatch.bridge.firefox.json");
        let chromium_manifest = json(&chromium);
        assert_eq!(chromium_manifest["name"], "app.wirecatch.bridge");
        assert_eq!(chromium_manifest["type"], "stdio");
        assert_eq!(chromium_manifest["path"], exe.to_str().unwrap());
        assert_eq!(
            chromium_manifest["allowed_origins"],
            serde_json::json!([ORIGIN])
        );
        assert!(chromium_manifest.get("allowed_extensions").is_none());
        let firefox_manifest = json(&firefox);
        assert_eq!(firefox_manifest["name"], "app.wirecatch.bridge");
        assert_eq!(firefox_manifest["path"], exe.to_str().unwrap());
        assert_eq!(
            firefox_manifest["allowed_extensions"],
            serde_json::json!(["extension@wirecatch.app"])
        );
        assert!(firefox_manifest.get("allowed_origins").is_none());

        let chromium_path = chromium.to_str().unwrap();
        let expected = [
            (
                r"Software\Google\Chrome\NativeMessagingHosts\app.wirecatch.bridge",
                chromium_path,
            ),
            (
                r"Software\Chromium\NativeMessagingHosts\app.wirecatch.bridge",
                chromium_path,
            ),
            (
                r"Software\Microsoft\Edge\NativeMessagingHosts\app.wirecatch.bridge",
                chromium_path,
            ),
            (
                r"Software\Mozilla\NativeMessagingHosts\app.wirecatch.bridge",
                firefox.to_str().unwrap(),
            ),
        ];
        for (key, value) in expected {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, key)
                    .unwrap()
                    .as_deref(),
                Some(value),
                "{key}"
            );
        }
        // Every browser with a Windows version is covered by the key it reads.
        let readers = |key: &str| -> Vec<&str> {
            plan.keys
                .iter()
                .find(|k| k.key.starts_with(key))
                .map(|k| k.browsers.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            readers(r"Software\Google\Chrome\"),
            [
                "Google Chrome",
                "Brave",
                "Vivaldi",
                "Opera",
                "Opera GX",
                "Arc",
                "Thorium"
            ]
        );
        assert_eq!(
            readers(r"Software\Chromium\"),
            ["Chromium", "ungoogled-chromium"]
        );
        assert_eq!(readers(r"Software\Microsoft\Edge\"), ["Microsoft Edge"]);
        assert_eq!(
            readers(r"Software\Mozilla\"),
            ["Mozilla Firefox", "LibreWolf", "Waterfox", "Zen", "Floorp"]
        );
        let covered: Vec<&str> = plan.keys.iter().flat_map(|k| k.browsers.clone()).collect();
        for browser in BROWSERS {
            assert_eq!(
                covered.contains(&browser.name),
                browser.windows_root.is_some(),
                "{}",
                browser.name
            );
        }
        // Only keys a browser reads: none of Brave's or Vivaldi's own.
        assert_eq!(plan.keys.len(), expected.len());
        for retired in [
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.wirecatch.bridge",
            r"Software\Vivaldi\NativeMessagingHosts\app.wirecatch.bridge",
        ] {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, retired).unwrap(),
                None,
                "{retired}"
            );
        }

        // Installing again changes nothing.
        let files_installed = tree(&temp.0);
        let registry_installed = host.snapshot();
        assert_eq!(install(&plan, &mut host).failures, 0);
        assert_eq!(tree(&temp.0), files_installed);
        assert_eq!(host.snapshot(), registry_installed);

        let report = uninstall(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(tree(&temp.0), files_before);
        let hosts_keys: Vec<&str> = plan.keys.iter().map(|k| k.hosts_key.as_str()).collect();
        assert_only_empty_parents_added(&registry_before, &host.snapshot(), |key| {
            hosts_keys
                .iter()
                .any(|hosts| hosts.starts_with(&format!(r"{key}\")))
        });

        // Uninstalling again is harmless.
        let registry_after = host.snapshot();
        assert_eq!(uninstall(&plan, &mut host).failures, 0);
        assert_eq!(tree(&temp.0), files_before);
        assert_eq!(host.snapshot(), registry_after);
    }

    #[test]
    fn keys_earlier_versions_wrote_for_brave_and_vivaldi_are_removed_when_they_are_ours() {
        let temp = TempDir::new();
        let local = temp.0.join("LocalAppData");
        let layout = Layout::Windows {
            exe: temp.0.join("Wirecatch").join("wirecatch-bridge.exe"),
            local_app_data: local.clone(),
        };
        let plan = plan(&config(), &layout).unwrap();
        let manifest_path = local
            .join("Wirecatch")
            .join("NativeMessagingHosts")
            .join("app.wirecatch.bridge.chromium.json");
        let ours = manifest_path.to_str().unwrap();
        let brave =
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.wirecatch.bridge";
        let vivaldi = r"Software\Vivaldi\NativeMessagingHosts\app.wirecatch.bridge";
        let mut host = FakeHost::default();
        // What an earlier version left: its own manifest (an older allow list)
        // and a Brave key pointing to it; a Vivaldi key someone else points elsewhere.
        fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        fs::write(&manifest_path, br#"{"allowed_origins": []}"#).unwrap();
        host.set_registry_value(Hive::CurrentUser, brave, ours)
            .unwrap();
        host.set_registry_value(Hive::CurrentUser, vivaldi, r"C:\Other\host.json")
            .unwrap();
        assert_eq!(
            plan.retired_keys
                .iter()
                .map(|k| k.key.as_str())
                .collect::<Vec<_>>(),
            [brave, vivaldi]
        );

        // Install rewrites the manifest, so the Brave key is recognized as ours and
        // removed with its empty hosts key; the Vivaldi key is left alone.
        let report = install(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(host.registry_value(Hive::CurrentUser, brave).unwrap(), None);
        assert!(
            !host
                .snapshot()
                .contains_key(r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts")
        );
        assert_eq!(
            host.registry_value(Hive::CurrentUser, vivaldi)
                .unwrap()
                .as_deref(),
            Some(r"C:\Other\host.json")
        );
        assert!(plan.keys.iter().all(|k| k.key != brave && k.key != vivaldi));

        // Uninstall removes one that came back, and still leaves the other.
        host.set_registry_value(Hive::CurrentUser, brave, ours)
            .unwrap();
        let report = uninstall(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(host.registry_value(Hive::CurrentUser, brave).unwrap(), None);
        assert!(
            host.registry_value(Hive::CurrentUser, vivaldi)
                .unwrap()
                .is_some()
        );
    }

    /// `after` holds everything `before` did, unchanged, plus only empty entries
    /// that `is_parent` accepts as parents of a hosts folder or key. Nothing at or
    /// below a `NativeMessagingHosts` level is left.
    fn assert_only_empty_parents_added<K: Ord + std::fmt::Debug, V: PartialEq + std::fmt::Debug>(
        before: &BTreeMap<K, Option<V>>,
        after: &BTreeMap<K, Option<V>>,
        is_parent: impl Fn(&K) -> bool,
    ) {
        for (name, value) in before {
            assert_eq!(after.get(name), Some(value), "{name:?} changed or vanished");
        }
        for (name, value) in after {
            if before.contains_key(name) {
                continue;
            }
            assert!(value.is_none(), "{name:?} was left with content");
            assert!(
                is_parent(name),
                "{name:?} is not a parent of a hosts folder"
            );
            assert!(
                !format!("{name:?}")
                    .to_lowercase()
                    .contains("nativemessaginghosts")
                    && !format!("{name:?}").contains("native-messaging-hosts"),
                "{name:?} was left behind"
            );
        }
    }

    #[test]
    fn linux_install_writes_every_manifest_and_uninstall_restores_the_state() {
        let temp = TempDir::new();
        let home = temp.0.join("home");
        let exe = temp
            .0
            .join("opt")
            .join("wirecatch")
            .join("wirecatch-bridge");
        let layout = Layout::linux(exe.clone(), home.clone(), None);
        let plan = plan(&config(), &layout).unwrap();
        let mut host = FakeHost::default();

        // A browser profile and a Firefox profile that must survive.
        fs::create_dir_all(home.join(".config").join("google-chrome").join("Default")).unwrap();
        fs::write(
            home.join(".config")
                .join("google-chrome")
                .join("Default")
                .join("Preferences"),
            b"{}",
        )
        .unwrap();
        fs::create_dir_all(home.join(".mozilla").join("firefox")).unwrap();
        fs::write(
            home.join(".mozilla").join("firefox").join("profiles.ini"),
            b"keep",
        )
        .unwrap();
        // An empty browser folder that was already there must not be removed.
        fs::create_dir_all(home.join(".config").join("vivaldi")).unwrap();
        let before = tree(&temp.0);

        let report = install(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(plan.keys.is_empty(), "no registry on Linux");

        let config_dir = home.join(".config");
        let chromium_dirs: [&[&str]; 8] = [
            &["google-chrome"],
            // Chromium and ungoogled-chromium.
            &["chromium"],
            &["microsoft-edge"],
            &["BraveSoftware", "Brave-Browser"],
            &["vivaldi"],
            &["opera"],
            &["thorium"],
            &["yandex-browser"],
        ];
        for parts in chromium_dirs {
            let mut dir = config_dir.clone();
            dir.extend(parts);
            let manifest = json(
                &dir.join("NativeMessagingHosts")
                    .join("app.wirecatch.bridge.json"),
            );
            assert_eq!(manifest["name"], "app.wirecatch.bridge");
            assert_eq!(manifest["type"], "stdio");
            assert_eq!(manifest["path"], exe.to_str().unwrap());
            assert_eq!(manifest["allowed_origins"], serde_json::json!([ORIGIN]));
        }
        let firefox = json(
            &home
                .join(".mozilla")
                .join("native-messaging-hosts")
                .join("app.wirecatch.bridge.json"),
        );
        assert_eq!(firefox["path"], exe.to_str().unwrap());
        assert_eq!(
            firefox["allowed_extensions"],
            serde_json::json!(["extension@wirecatch.app"])
        );
        // One file per folder: 8 Chromium folders and Firefox's, which LibreWolf,
        // Waterfox, Zen and Floorp read too. No Flatpak or Snap folder is written.
        assert_eq!(plan.files.len(), chromium_dirs.len() + 1);
        let firefox_file = plan
            .files
            .iter()
            .find(|f| f.path.starts_with(home.join(".mozilla")))
            .unwrap();
        assert_eq!(
            firefox_file.browsers,
            ["Mozilla Firefox", "LibreWolf", "Waterfox", "Zen", "Floorp"]
        );
        assert!(
            plan.files
                .iter()
                .all(|f| !f.path.starts_with(home.join(".var"))
                    && !f.path.starts_with(home.join("snap"))
                    && !f.path.starts_with(home.join(".librewolf"))
                    && !f.path.starts_with(home.join(".waterfox")))
        );
        let linux_browsers: Vec<&str> =
            plan.files.iter().flat_map(|f| f.browsers.clone()).collect();
        for browser in BROWSERS {
            assert_eq!(
                linux_browsers.contains(&browser.name),
                browser.linux.is_some(),
                "{}",
                browser.name
            );
        }
        for name in ["Thorium", "Yandex Browser", "ungoogled-chromium"] {
            assert!(
                report.lines.iter().any(|l| l.contains(name)),
                "{name}: {:?}",
                report.lines
            );
        }

        let report = uninstall(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        let hosts_dirs: Vec<PathBuf> = plan
            .files
            .iter()
            .map(|f| f.hosts_dir.strip_prefix(&temp.0).unwrap().to_path_buf())
            .collect();
        assert_only_empty_parents_added(&before, &tree(&temp.0), |dir| {
            hosts_dirs
                .iter()
                .any(|hosts| hosts.starts_with(dir) && hosts != dir)
        });
    }

    #[test]
    fn linux_honors_xdg_config_home() {
        let layout = Layout::linux(
            PathBuf::from("/opt/wirecatch/wirecatch-bridge"),
            PathBuf::from("/home/u"),
            Some(PathBuf::from("/home/u/cfg")),
        );
        let plan = plan(&config(), &layout);
        // Paths are only absolute on Unix; the layout logic is the same.
        if cfg!(unix) {
            let plan = plan.unwrap();
            assert_eq!(
                plan.files[0].path,
                Path::new(
                    "/home/u/cfg/google-chrome/NativeMessagingHosts/app.wirecatch.bridge.json"
                )
            );
            assert_eq!(
                plan.files[6].path,
                Path::new("/home/u/.mozilla/native-messaging-hosts/app.wirecatch.bridge.json")
            );
        } else {
            assert!(plan.is_err(), "a Unix path is not absolute on Windows");
        }
    }

    #[test]
    fn uninstall_leaves_registrations_from_another_copy_alone() {
        // Two copies of the bridge for one user share LOCALAPPDATA, so they write
        // the same manifest paths and the same registry values; only the
        // manifest contents (the executable path) differ.
        let temp = TempDir::new();
        let local = temp.0.join("LocalAppData");
        let mine = plan(
            &config(),
            &Layout::Windows {
                exe: temp.0.join("a").join("wirecatch-bridge.exe"),
                local_app_data: local.clone(),
            },
        )
        .unwrap();
        let theirs = plan(
            &config(),
            &Layout::Windows {
                exe: temp.0.join("b").join("wirecatch-bridge.exe"),
                local_app_data: local,
            },
        )
        .unwrap();
        for (a, b) in mine.keys.iter().zip(&theirs.keys) {
            assert_eq!((&a.key, &a.value), (&b.key, &b.value));
        }

        let mut host = FakeHost::default();
        assert_eq!(install(&theirs, &mut host).failures, 0);
        let files = tree(&temp.0);
        let registry = host.snapshot();

        let report = uninstall(&mine, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(
            host.snapshot(),
            registry,
            "their registry keys were removed"
        );
        assert_eq!(tree(&temp.0), files, "their manifests were removed");

        // Their own uninstall still removes everything.
        assert_eq!(uninstall(&theirs, &mut host).failures, 0);
        for entry in &theirs.keys {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, &entry.key).unwrap(),
                None
            );
        }
    }

    #[test]
    fn uninstall_removes_a_key_whose_manifest_is_already_gone() {
        let temp = TempDir::new();
        let plan = plan(
            &config(),
            &Layout::Windows {
                exe: temp.0.join("a").join("wirecatch-bridge.exe"),
                local_app_data: temp.0.join("LocalAppData"),
            },
        )
        .unwrap();
        let mut host = FakeHost::default();
        assert_eq!(install(&plan, &mut host).failures, 0);
        for file in &plan.files {
            fs::remove_file(&file.path).unwrap();
        }
        assert_eq!(uninstall(&plan, &mut host).failures, 0);
        for entry in &plan.keys {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, &entry.key).unwrap(),
                None
            );
        }
    }

    #[test]
    fn a_relative_executable_path_is_refused() {
        let layout = Layout::Windows {
            exe: PathBuf::from("wirecatch-bridge.exe"),
            local_app_data: std::env::temp_dir(),
        };
        assert!(plan(&config(), &layout).is_err());
    }

    /// Exercises the real HKCU code under a throwaway key, then removes it.
    #[cfg(windows)]
    #[test]
    fn real_registry_round_trip() {
        let root = format!(r"Software\WirecatchBridgeTest-{}", std::process::id());
        let key = format!(r"{root}\NativeMessagingHosts\app.wirecatch.bridge");
        let mut host = RealHost;
        assert_eq!(host.registry_value(Hive::CurrentUser, &key).unwrap(), None);
        host.set_registry_value(Hive::CurrentUser, &key, r"C:\path\manifest.json")
            .unwrap();
        let read_back = host.registry_value(Hive::CurrentUser, &key);
        // A key with a subkey is not empty, so it stays.
        let kept = host.delete_registry_key_if_empty(Hive::CurrentUser, &root);
        let deleted = host.delete_registry_key(Hive::CurrentUser, &key);
        let hosts_pruned = host.delete_registry_key_if_empty(
            Hive::CurrentUser,
            &format!(r"{root}\NativeMessagingHosts"),
        );
        let root_pruned = host.delete_registry_key_if_empty(Hive::CurrentUser, &root);
        let missing = host.delete_registry_key_if_empty(Hive::CurrentUser, &root);
        assert_eq!(
            read_back.unwrap().as_deref(),
            Some(r"C:\path\manifest.json")
        );
        assert!(!kept.unwrap());
        deleted.unwrap();
        assert!(hosts_pruned.unwrap());
        assert!(root_pruned.unwrap());
        assert!(!missing.unwrap());
        assert_eq!(host.registry_value(Hive::CurrentUser, &key).unwrap(), None);
    }

    /// A per-machine install: the bridge in Program Files, registered for every user, and the same user plan the app runs on each start.
    fn machine_and_user(temp: &TempDir, exe: &Path) -> (Plan, Plan) {
        let machine = plan(
            &config(),
            &Layout::WindowsMachine {
                exe: exe.to_path_buf(),
            },
        )
        .unwrap();
        let user = plan(
            &config(),
            &Layout::Windows {
                exe: exe.to_path_buf(),
                local_app_data: temp.0.join("LocalAppData"),
            },
        )
        .unwrap();
        (machine, user)
    }

    #[test]
    fn a_machine_registration_lives_next_to_the_bridge_and_under_local_machine() {
        let temp = TempDir::new();
        let exe = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let (machine, user) = machine_and_user(&temp, &exe);
        let hosts = exe.parent().unwrap().join("NativeMessagingHosts");
        assert!(
            machine
                .files
                .iter()
                .all(|f| f.path.parent() == Some(hosts.as_path()))
        );
        assert!(machine.keys.iter().all(|k| k.hive == Hive::LocalMachine));
        let names = |p: &Plan| p.keys.iter().map(|k| k.key.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&machine),
            names(&user),
            "the same browsers, in the other hive"
        );

        let mut host = FakeHost::default();
        let report = install(&machine, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(covered_by_machine(&machine, &host));
        assert!(
            host.registry.is_empty(),
            "nothing is written for the current user"
        );

        let report = uninstall(&machine, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(
            host.machine.values().all(|(_, value)| value.is_none()),
            "every key the install set is gone"
        );
        assert!(!hosts.exists(), "the manifests and their folder are gone");
    }

    // Under a per-machine install the app used to register the bridge for each user too. An uninstall
    // only cleans up the account that runs it, so every other user kept a registration pointing at
    // removed files.
    #[test]
    fn a_user_install_steps_aside_for_the_machine_registration() {
        let temp = TempDir::new();
        let exe = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let (machine, user) = machine_and_user(&temp, &exe);
        let mut host = FakeHost::default();
        // This user registered on an earlier start.
        assert_eq!(install(&user, &mut host).failures, 0);
        assert_eq!(install(&machine, &mut host).failures, 0);

        let report = install_for_user(&user, Some(&machine), &mut host);

        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(
            user.keys.iter().all(|k| host
                .registry_value(Hive::CurrentUser, &k.key)
                .unwrap()
                .is_none()),
            "the user registration is removed: {:?}",
            host.snapshot()
        );
        assert!(user.files.iter().all(|f| !f.path.exists()));
        assert!(
            covered_by_machine(&machine, &host),
            "the machine registration stays"
        );
    }

    // Browsers read the account's registration before the machine's. One left by an earlier per-user
    // install, or by a portable copy that was deleted, named a bridge that is gone, and the app no
    // longer rewrote it, so the extension could not start any bridge for that account.
    #[test]
    fn a_user_install_removes_registrations_of_bridges_that_are_gone() {
        let temp = TempDir::new();
        let exe = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let (machine, user) = machine_and_user(&temp, &exe);
        let deleted = temp.0.join("Deleted").join("wirecatch-bridge.exe");
        let (_, deleted_user) = machine_and_user(&temp, &deleted);
        let mut host = FakeHost::default();
        assert_eq!(install(&deleted_user, &mut host).failures, 0);
        assert_eq!(install(&machine, &mut host).failures, 0);

        let report = install_for_user(&user, Some(&machine), &mut host);

        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(
            user.keys.iter().all(|k| host
                .registry_value(Hive::CurrentUser, &k.key)
                .unwrap()
                .is_none()),
            "the dead registration is removed: {:?}",
            host.snapshot()
        );
        assert!(user.files.iter().all(|f| !f.path.exists()));
        assert!(covered_by_machine(&machine, &host));
    }

    #[test]
    fn a_user_install_leaves_a_registration_of_another_copy_that_exists() {
        let temp = TempDir::new();
        let exe = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let (machine, user) = machine_and_user(&temp, &exe);
        let portable = temp.0.join("Portable").join("wirecatch-bridge.exe");
        files::write(&portable, b"a bridge").unwrap();
        let (_, portable_user) = machine_and_user(&temp, &portable);
        let mut host = FakeHost::default();
        assert_eq!(install(&portable_user, &mut host).failures, 0);
        assert_eq!(install(&machine, &mut host).failures, 0);

        let report = install_for_user(&user, Some(&machine), &mut host);

        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(portable_user.keys.iter().all(|k| {
            host.registry_value(Hive::CurrentUser, &k.key)
                .unwrap()
                .as_deref()
                == Some(k.value.as_str())
        }));
        assert!(portable_user.files.iter().all(|f| f.path.exists()));
    }

    #[test]
    fn a_user_install_registers_when_no_machine_registration_covers_this_bridge() {
        let temp = TempDir::new();
        let installed = temp
            .0
            .join("Program Files")
            .join("Wirecatch")
            .join("wirecatch-bridge.exe");
        let portable = temp.0.join("Portable").join("wirecatch-bridge.exe");
        let (machine, _) = machine_and_user(&temp, &installed);
        let (portable_machine, portable_user) = machine_and_user(&temp, &portable);
        let mut host = FakeHost::default();
        assert_eq!(install(&machine, &mut host).failures, 0);

        // A portable copy elsewhere is not covered by the installed copy.
        assert!(!covered_by_machine(&portable_machine, &host));
        let report = install_for_user(&portable_user, Some(&portable_machine), &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(portable_user.keys.iter().all(|k| {
            host.registry_value(Hive::CurrentUser, &k.key)
                .unwrap()
                .as_deref()
                == Some(k.value.as_str())
        }));
        // Without any machine plan (Linux), a user install is a plain install.
        let alone = install_for_user(&portable_user, None, &mut host);
        assert_eq!(alone.failures, 0, "{:?}", alone.lines);
    }
}
