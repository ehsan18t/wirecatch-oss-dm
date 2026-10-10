//! `wirecatch-bridge install` and `uninstall`: register the bridge as a native
//! messaging host for the current user in every supported browser.
//!
//! Everything is per user; nothing needs administrator rights.
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
use serde::Serialize;

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
    /// Windows: the key under HKCU that holds `NativeMessagingHosts`.
    windows_root: &'static str,
    /// Linux: where the browser's folder lives.
    linux_base: LinuxBase,
    /// Linux: the browser's folder below that base, as path components.
    linux_dir: &'static [&'static str],
    /// Linux: the name of the hosts folder inside it.
    linux_hosts: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxBase {
    /// `$XDG_CONFIG_HOME`, or `~/.config`.
    ConfigHome,
    /// The home folder.
    Home,
}

/// The browsers `install` registers with.
///
/// Opera on Windows reads Google Chrome's registry key, so its entry is the same
/// key as Chrome's and is written once.
const BROWSERS: &[Browser] = &[
    Browser {
        name: "Google Chrome",
        engine: Engine::Chromium,
        windows_root: r"Software\Google\Chrome",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["google-chrome"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Chromium",
        engine: Engine::Chromium,
        windows_root: r"Software\Chromium",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["chromium"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Microsoft Edge",
        engine: Engine::Chromium,
        windows_root: r"Software\Microsoft\Edge",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["microsoft-edge"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Brave",
        engine: Engine::Chromium,
        windows_root: r"Software\BraveSoftware\Brave-Browser",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["BraveSoftware", "Brave-Browser"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Vivaldi",
        engine: Engine::Chromium,
        windows_root: r"Software\Vivaldi",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["vivaldi"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Opera",
        engine: Engine::Chromium,
        windows_root: r"Software\Google\Chrome",
        linux_base: LinuxBase::ConfigHome,
        linux_dir: &["opera"],
        linux_hosts: "NativeMessagingHosts",
    },
    Browser {
        name: "Mozilla Firefox",
        engine: Engine::Firefox,
        windows_root: r"Software\Mozilla",
        linux_base: LinuxBase::Home,
        linux_dir: &[".mozilla"],
        linux_hosts: "native-messaging-hosts",
    },
];

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

    fn exe(&self) -> &Path {
        match self {
            Self::Windows { exe, .. } | Self::Linux { exe, .. } => exe,
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

/// A registry key under HKCU whose default value is a manifest path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryEntry {
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
    };
    match layout {
        Layout::Windows { local_app_data, .. } => {
            let dir = local_app_data
                .join("Wirecatch")
                .join("NativeMessagingHosts");
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
                    hosts_dir: dir.clone(),
                });
                for browser in BROWSERS.iter().filter(|b| b.engine == engine) {
                    if let Some(file) = plan.files.last_mut() {
                        file.browsers.push(browser.name);
                    }
                    let hosts_key = format!(r"{}\NativeMessagingHosts", browser.windows_root);
                    let key = format!(r"{hosts_key}\{host}");
                    if let Some(entry) = plan.keys.iter_mut().find(|e| e.key == key) {
                        entry.browsers.push(browser.name);
                        continue;
                    }
                    plan.keys.push(RegistryEntry {
                        key,
                        value: value.clone(),
                        manifest_contents: contents.clone(),
                        browsers: vec![browser.name],
                        hosts_key,
                    });
                }
            }
        }
        Layout::Linux {
            home, config_home, ..
        } => {
            for browser in BROWSERS {
                let base = match browser.linux_base {
                    LinuxBase::ConfigHome => config_home,
                    LinuxBase::Home => home,
                };
                let mut dir = base.clone();
                dir.extend(browser.linux_dir);
                dir.push(browser.linux_hosts);
                plan.files.push(ManifestFile {
                    path: dir.join(format!("{host}.json")),
                    contents: manifest(config, exe_text, browser.engine)?,
                    browsers: vec![browser.name],
                    hosts_dir: dir,
                });
            }
        }
    }
    Ok(plan)
}

/// File system and registry access, behind a trait so tests can run against a
/// temporary folder and an in-memory registry.
pub(crate) trait Host {
    /// The file's contents, or `None` if it does not exist.
    fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;
    /// Writes the file, creating missing folders, replacing it atomically.
    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// Removes the file.
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    /// Removes the folder if it exists and is empty. Returns whether it did.
    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool>;
    /// The default value of an HKCU key, or `None` if the key or value is missing.
    fn registry_value(&self, key: &str) -> io::Result<Option<String>>;
    /// Creates the HKCU key (and missing parents) and sets its default value.
    fn set_registry_value(&mut self, key: &str, value: &str) -> io::Result<()>;
    /// Deletes the HKCU key, which must have no subkeys. Missing is fine.
    fn delete_registry_key(&mut self, key: &str) -> io::Result<()>;
    /// Deletes the HKCU key if it exists with no subkeys and no values. Returns
    /// whether it did.
    fn delete_registry_key_if_empty(&mut self, key: &str) -> io::Result<bool>;
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

    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
        files::write(path, contents)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool> {
        files::remove_dir_if_empty(path)
    }

    fn registry_value(&self, key: &str) -> io::Result<Option<String>> {
        registry::value(key)
    }

    fn set_registry_value(&mut self, key: &str, value: &str) -> io::Result<()> {
        registry::set_value(key, value)
    }

    fn delete_registry_key(&mut self, key: &str) -> io::Result<()> {
        registry::delete_key(key)
    }

    fn delete_registry_key_if_empty(&mut self, key: &str) -> io::Result<bool> {
        registry::delete_key_if_empty(key)
    }
}

#[cfg(not(windows))]
mod registry {
    //! There is no registry outside Windows; the Linux plan never uses it.

    use std::io;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no registry on this system",
        )
    }

    pub(crate) fn value(_key: &str) -> io::Result<Option<String>> {
        Err(unsupported())
    }
    pub(crate) fn set_value(_key: &str, _value: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key(_key: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key_if_empty(_key: &str) -> io::Result<bool> {
        Err(unsupported())
    }
}

#[cfg(windows)]
mod registry {
    //! The per-user registry (HKEY_CURRENT_USER) through the Win32 API.

    use std::io;
    use std::ptr;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegGetValueW, RegOpenKeyExW,
        RegQueryInfoKeyW, RegSetValueExW,
    };

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

    pub(crate) fn value(key: &str) -> io::Result<Option<String>> {
        let key = wide(key);
        let mut buf: Vec<u16> = vec![0; 260];
        // The value can change size between calls; retry a few times.
        for _ in 0..4 {
            let mut size = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
            // SAFETY: `key` is NUL-terminated; `buf` is writable for `size` bytes.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
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

    pub(crate) fn set_value(key: &str, value: &str) -> io::Result<()> {
        let key = wide(key);
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: `key` is NUL-terminated; on success `handle` is a key we own.
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
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

    pub(crate) fn delete_key(key: &str) -> io::Result<()> {
        let key = wide(key);
        // SAFETY: `key` is NUL-terminated.
        match unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            other => Err(io::Error::from_raw_os_error(other as i32)),
        }
    }

    pub(crate) fn delete_key_if_empty(key: &str) -> io::Result<bool> {
        let wide_key = wide(key);
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: `wide_key` is NUL-terminated; on success `handle` is ours.
        match unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
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
        delete_key(key)?;
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
        match host.set_registry_value(&entry.key, &entry.value) {
            Ok(()) => report.ok(format!(
                r"Registered HKEY_CURRENT_USER\{} ({})",
                entry.key,
                entry.browsers.join(", ")
            )),
            Err(e) => report.fail(format!(
                r"Could not register HKEY_CURRENT_USER\{}: {e}",
                entry.key
            )),
        }
    }
    report
}

/// Removes what [`install`] wrote from this copy of the bridge, leaving anything
/// that another copy wrote or that changed since alone.
pub(crate) fn uninstall(plan: &Plan, host: &mut dyn Host) -> Report {
    let mut report = Report::default();
    // Keys first, while the manifests they point to can still be compared.
    for entry in &plan.keys {
        let name = format!(r"HKEY_CURRENT_USER\{}", entry.key);
        let value = match host.registry_value(&entry.key) {
            Ok(None) => continue,
            Ok(Some(value)) => value,
            Err(e) => {
                report.fail(format!("Could not read {name}: {e}"));
                continue;
            }
        };
        if value != entry.value {
            report.ok(format!(
                "Left {name} in place: it points to {value}, which this copy did not write"
            ));
            continue;
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
                continue;
            }
            Err(e) => {
                report.fail(format!("Could not read {}: {e}", entry.value));
                continue;
            }
        }
        match host.delete_registry_key(&entry.key) {
            Ok(()) => {
                report.ok(format!("Removed {name}"));
                if let Err(e) = host.delete_registry_key_if_empty(&entry.hosts_key) {
                    report.fail(format!(
                        r"Could not tidy HKEY_CURRENT_USER\{}: {e}",
                        entry.hosts_key
                    ));
                }
            }
            Err(e) => report.fail(format!("Could not remove {name}: {e}")),
        }
    }
    for file in &plan.files {
        let shown = file.path.display();
        match host.read_file(&file.path) {
            Ok(None) => {}
            Ok(Some(bytes)) if bytes == file.contents.as_bytes() => {
                match host.remove_file(&file.path) {
                    Ok(()) => {
                        report.ok(format!("Removed {shown}"));
                        if let Err(e) = host.remove_dir_if_empty(&file.hosts_dir) {
                            report
                                .fail(format!("Could not tidy {}: {e}", file.hosts_dir.display()));
                        }
                    }
                    Err(e) => report.fail(format!("Could not remove {shown}: {e}")),
                }
            }
            Ok(Some(_)) => report.ok(format!(
                "Left {shown} in place: it was changed or written by another copy of Wirecatch"
            )),
            Err(e) => report.fail(format!("Could not read {shown}: {e}")),
        }
    }
    report
}

/// Which command to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// `wirecatch-bridge install`
    Install,
    /// `wirecatch-bridge uninstall`
    Uninstall,
}

/// Runs `install` or `uninstall` for the current user and prints what happened.
#[allow(clippy::print_stdout)] // a terminal command, not native messaging mode
pub(crate) fn run(command: Command) -> ExitCode {
    let plan = Config::embedded()
        .and_then(|config| Ok((config, Layout::current()?)))
        .and_then(|(config, layout)| plan(&config, &layout));
    let plan = match plan {
        Ok(plan) => plan,
        Err(e) => {
            crate::log!("{e:#}");
            return ExitCode::FAILURE;
        }
    };
    let report = match command {
        Command::Install => install(&plan, &mut RealHost),
        Command::Uninstall => uninstall(&plan, &mut RealHost),
    };
    for line in &report.lines {
        println!("{line}");
    }
    if report.failures > 0 {
        ExitCode::FAILURE
    } else {
        match command {
            Command::Install => println!("The Wirecatch browser bridge is registered."),
            Command::Uninstall => println!("The Wirecatch browser bridge is no longer registered."),
        }
        ExitCode::SUCCESS
    }
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
        /// Lowercased key path to (original spelling, default value).
        registry: BTreeMap<String, (String, Option<String>)>,
    }

    impl FakeHost {
        fn has_subkeys(&self, key: &str) -> bool {
            let prefix = format!(r"{}\", key.to_lowercase());
            self.registry.keys().any(|k| k.starts_with(&prefix))
        }

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
        fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
            files::write(path, contents)
        }
        fn remove_file(&mut self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }
        fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool> {
            files::remove_dir_if_empty(path)
        }
        fn registry_value(&self, key: &str) -> io::Result<Option<String>> {
            Ok(self
                .registry
                .get(&key.to_lowercase())
                .and_then(|(_, value)| value.clone()))
        }
        fn set_registry_value(&mut self, key: &str, value: &str) -> io::Result<()> {
            let parts: Vec<&str> = key.split('\\').collect();
            for depth in 1..parts.len() {
                let parent = parts[..depth].join("\\");
                self.registry
                    .entry(parent.to_lowercase())
                    .or_insert((parent, None));
            }
            self.registry.insert(
                key.to_lowercase(),
                (key.to_string(), Some(value.to_string())),
            );
            Ok(())
        }
        fn delete_registry_key(&mut self, key: &str) -> io::Result<()> {
            if self.has_subkeys(key) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            self.registry.remove(&key.to_lowercase());
            Ok(())
        }
        fn delete_registry_key_if_empty(&mut self, key: &str) -> io::Result<bool> {
            let lower = key.to_lowercase();
            let empty =
                matches!(self.registry.get(&lower), Some((_, None))) && !self.has_subkeys(key);
            if empty {
                self.registry.remove(&lower);
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
        host.set_registry_value(r"Software\Google\Chrome\PreferenceMACs", "keep")
            .unwrap();
        host.set_registry_value(r"Software\Microsoft\Edge\BLBeacon", "keep")
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
                r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\app.wirecatch.bridge",
                chromium_path,
            ),
            (
                r"Software\Vivaldi\NativeMessagingHosts\app.wirecatch.bridge",
                chromium_path,
            ),
            (
                r"Software\Mozilla\NativeMessagingHosts\app.wirecatch.bridge",
                firefox.to_str().unwrap(),
            ),
        ];
        for (key, value) in expected {
            assert_eq!(
                host.registry_value(key).unwrap().as_deref(),
                Some(value),
                "{key}"
            );
        }
        // Every browser is covered (Opera shares Chrome's key).
        let covered: Vec<&str> = plan.keys.iter().flat_map(|k| k.browsers.clone()).collect();
        for browser in BROWSERS {
            assert!(covered.contains(&browser.name), "{}", browser.name);
        }
        assert_eq!(plan.keys.len(), expected.len());

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
        let chromium_dirs: [&[&str]; 6] = [
            &["google-chrome"],
            &["chromium"],
            &["microsoft-edge"],
            &["BraveSoftware", "Brave-Browser"],
            &["vivaldi"],
            &["opera"],
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
        assert_eq!(plan.files.len(), BROWSERS.len());

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
            assert_eq!(host.registry_value(&entry.key).unwrap(), None);
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
            assert_eq!(host.registry_value(&entry.key).unwrap(), None);
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
        assert_eq!(host.registry_value(&key).unwrap(), None);
        host.set_registry_value(&key, r"C:\path\manifest.json")
            .unwrap();
        let read_back = host.registry_value(&key);
        // A key with a subkey is not empty, so it stays.
        let kept = host.delete_registry_key_if_empty(&root);
        let deleted = host.delete_registry_key(&key);
        let hosts_pruned =
            host.delete_registry_key_if_empty(&format!(r"{root}\NativeMessagingHosts"));
        let root_pruned = host.delete_registry_key_if_empty(&root);
        let missing = host.delete_registry_key_if_empty(&root);
        assert_eq!(
            read_back.unwrap().as_deref(),
            Some(r"C:\path\manifest.json")
        );
        assert!(!kept.unwrap());
        deleted.unwrap();
        assert!(hosts_pruned.unwrap());
        assert!(root_pruned.unwrap());
        assert!(!missing.unwrap());
        assert_eq!(host.registry_value(&key).unwrap(), None);
    }
}
