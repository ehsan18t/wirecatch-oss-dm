//! Which files and keys a registration is made of, worked out without touching the system.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

use super::browsers::{BROWSERS, Engine, LinuxBase, RETIRED_WINDOWS_ROOTS};
use crate::config::Config;
use wirecatch_product::{Channel, NATIVE_HOST_DESCRIPTION};

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

pub(super) fn absolute_env(name: &str) -> Option<PathBuf> {
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
pub(super) struct ChromiumManifest<'a> {
    pub(super) name: &'a str,
    pub(super) description: &'a str,
    pub(super) path: &'a str,
    #[serde(rename = "type")]
    pub(super) kind: &'a str,
    pub(super) allowed_origins: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct FirefoxManifest<'a> {
    pub(super) name: &'a str,
    pub(super) description: &'a str,
    pub(super) path: &'a str,
    #[serde(rename = "type")]
    pub(super) kind: &'a str,
    pub(super) allowed_extensions: &'a [String],
}

pub(super) fn manifest(config: &Config, exe: &str, engine: Engine) -> anyhow::Result<String> {
    let mut text = match engine {
        Engine::Chromium => serde_json::to_string_pretty(&ChromiumManifest {
            name: &config.native_host_name,
            description: NATIVE_HOST_DESCRIPTION,
            path: exe,
            kind: "stdio",
            allowed_origins: config.chromium_origins(),
        })?,
        Engine::Firefox => serde_json::to_string_pretty(&FirefoxManifest {
            name: &config.native_host_name,
            description: NATIVE_HOST_DESCRIPTION,
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
            // The release app's data folder: every build registers under the one
            // host name (D22), so every build writes the same manifests.
            let dir = local_app_data
                .join(Channel::Release.windows_data_folder())
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
pub(super) fn plan_windows(
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
