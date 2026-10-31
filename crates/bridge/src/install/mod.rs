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
//! [`BROWSERS`](browsers::BROWSERS)), so one manifest or key often serves several browsers. A
//! location is only written when a browser really reads it.
//!
//! Sandboxed Linux browsers: a Flatpak or Snap browser reads manifests inside its
//! sandbox, but the bridge they name is installed outside it, where the sandbox
//! cannot start it, so no manifest is written there. Firefox as a Snap (Ubuntu's
//! default) and as a Flatpak can still reach the bridge through the
//! `WebExtensions` portal (`xdg-desktop-portal`), which reads the host's own
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

mod browsers;
mod host;
mod plan;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;

use self::host::{Host, RealHost};
use self::plan::{Layout, ManifestFile, Plan, RegistryEntry, plan};
use crate::config::Config;

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

/// Writes every manifest and registry key in `plan` that does not already hold what it should.
/// The app runs this on every start, so an unchanged registration is left untouched.
pub(crate) fn install(plan: &Plan, host: &mut dyn Host) -> Report {
    let mut report = Report::default();
    for file in &plan.files {
        let browsers = file.browsers.join(", ");
        if host.read_file(&file.path).ok().flatten().as_deref() == Some(file.contents.as_bytes()) {
            report.ok(format!(
                "Already in place: {} ({browsers})",
                file.path.display()
            ));
            continue;
        }
        match host.write_file(&file.path, file.contents.as_bytes()) {
            Ok(()) => report.ok(format!("Wrote {} ({browsers})", file.path.display())),
            Err(e) => report.fail(format!("Could not write {}: {e}", file.path.display())),
        }
    }
    for entry in &plan.keys {
        let name = format!(r"{}\{}", entry.hive.name(), entry.key);
        let browsers = entry.browsers.join(", ");
        if host
            .registry_value(entry.hive, &entry.key)
            .ok()
            .flatten()
            .is_some_and(|value| value == entry.value)
        {
            report.ok(format!("Already registered: {name} ({browsers})"));
            continue;
        }
        match host.set_registry_value(entry.hive, &entry.key, &entry.value) {
            Ok(()) => report.ok(format!("Registered {name} ({browsers})")),
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

/// Runs `install` or `uninstall` and writes what happened to stdout: a terminal
/// command, not native messaging mode, so stdout belongs to the person.
pub(crate) fn run(command: Command, scope: Scope) -> ExitCode {
    let report = match planned_report(command, scope) {
        Ok(report) => report,
        Err(e) => {
            tracing::error!("{e:#}");
            return ExitCode::FAILURE;
        }
    };
    let succeeded = report.failures == 0;
    let summary = succeeded.then_some(match (command, scope) {
        (Command::Install, Scope::User) => "The Wirecatch browser bridge is registered.",
        (Command::Install, Scope::Machine) => {
            "The Wirecatch browser bridge is registered for every user of this computer."
        }
        (Command::Uninstall, _) => "The Wirecatch browser bridge is no longer registered.",
    });
    // The registration is done either way, so a report nobody can read (stdout
    // closed) is logged and does not change the exit code.
    if let Err(e) = print_report(&report.lines, summary) {
        tracing::warn!("could not write the report: {e}");
    }
    if succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Writes the report's lines and then `summary` to stdout, one per line.
fn print_report(lines: &[String], summary: Option<&str>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    for line in lines.iter().map(String::as_str).chain(summary) {
        writeln!(out, "{line}")?;
    }
    out.flush()
}

fn planned_report(command: Command, scope: Scope) -> anyhow::Result<Report> {
    let config = Config::embedded();
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
    use std::fs;

    use super::browsers::BROWSERS;
    use super::host::files;
    use super::plan::Hive;
    use super::*;
    use std::collections::BTreeMap;
    use wirecatch_product::{
        BRIDGE_EXECUTABLE, CHROMIUM_EXTENSION_ORIGIN, Channel, DATABASE_FILE, FIREFOX_EXTENSION_ID,
        NATIVE_HOST_NAME,
    };
    use wirecatch_test_support::{PrivateDir, private_dir};

    /// The bridge as the Windows installer ships it, whatever system the test runs on.
    fn windows_bridge() -> String {
        format!("{BRIDGE_EXECUTABLE}.exe")
    }

    /// The key under `parent` where Windows browsers look for the bridge.
    fn hosts_key(parent: &str) -> String {
        format!(r"{parent}\NativeMessagingHosts\{NATIVE_HOST_NAME}")
    }

    /// The app's data folder, which holds the per-user manifests on Windows.
    fn data_folder(local: &Path) -> PathBuf {
        local.join(Channel::Release.windows_data_folder())
    }

    /// A temporary folder for one test, removed on drop.
    fn temp_dir() -> PrivateDir {
        private_dir("bridge-install").unwrap()
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
        /// Files and registry values written.
        writes: usize,
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
            self.writes += 1;
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
            self.writes += 1;
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
        Config::embedded()
    }

    fn json(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    const ORIGIN: &str = CHROMIUM_EXTENSION_ORIGIN;

    /// Registry values and files from before Wirecatch, which install and uninstall
    /// must leave alone.
    fn seed_windows_state(local: &Path, host: &mut FakeHost) {
        fs::create_dir_all(data_folder(local)).unwrap();
        fs::write(data_folder(local).join(DATABASE_FILE), b"keep").unwrap();
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
    }

    /// Checks the two manifests a Windows install writes into `dir`, and returns
    /// the Chromium one's path and the Firefox one's.
    fn assert_windows_manifests(dir: &Path, exe: &Path) -> (PathBuf, PathBuf) {
        let chromium = dir.join(format!("{NATIVE_HOST_NAME}.chromium.json"));
        let firefox = dir.join(format!("{NATIVE_HOST_NAME}.firefox.json"));
        let chromium_manifest = json(&chromium);
        assert_eq!(chromium_manifest["name"], NATIVE_HOST_NAME);
        assert_eq!(chromium_manifest["type"], "stdio");
        assert_eq!(chromium_manifest["path"], exe.to_str().unwrap());
        assert_eq!(
            chromium_manifest["allowed_origins"],
            serde_json::json!([ORIGIN])
        );
        assert!(chromium_manifest.get("allowed_extensions").is_none());
        let firefox_manifest = json(&firefox);
        assert_eq!(firefox_manifest["name"], NATIVE_HOST_NAME);
        assert_eq!(firefox_manifest["path"], exe.to_str().unwrap());
        assert_eq!(
            firefox_manifest["allowed_extensions"],
            serde_json::json!([FIREFOX_EXTENSION_ID])
        );
        assert!(firefox_manifest.get("allowed_origins").is_none());
        (chromium, firefox)
    }

    /// Checks that exactly the keys browsers read point to the manifests, and that
    /// each Windows browser is covered by the key it reads.
    fn assert_windows_keys(plan: &Plan, host: &FakeHost, chromium: &Path, firefox: &Path) {
        let chromium_path = chromium.to_str().unwrap();
        let expected = [
            (hosts_key(r"Software\Google\Chrome"), chromium_path),
            (hosts_key(r"Software\Chromium"), chromium_path),
            (hosts_key(r"Software\Microsoft\Edge"), chromium_path),
            (hosts_key(r"Software\Mozilla"), firefox.to_str().unwrap()),
        ];
        for (key, value) in &expected {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, key)
                    .unwrap()
                    .as_deref(),
                Some(*value),
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
            hosts_key(r"Software\BraveSoftware\Brave-Browser"),
            hosts_key(r"Software\Vivaldi"),
        ] {
            assert_eq!(
                host.registry_value(Hive::CurrentUser, &retired).unwrap(),
                None,
                "{retired}"
            );
        }
    }

    #[test]
    fn windows_install_registers_every_browser_and_uninstall_restores_the_state() {
        let temp = temp_dir();
        let exe = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
        let local = temp.path().join("LocalAppData");
        let layout = Layout::Windows {
            exe: exe.clone(),
            local_app_data: local.clone(),
        };
        let plan = plan(&config(), &layout).unwrap();
        let mut host = FakeHost::default();
        seed_windows_state(&local, &mut host);
        let files_before = tree(temp.path());
        let registry_before = host.snapshot();

        let report = install(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        let dir = data_folder(&local).join("NativeMessagingHosts");
        let (chromium, firefox) = assert_windows_manifests(&dir, &exe);
        assert_windows_keys(&plan, &host, &chromium, &firefox);

        // Installing again changes nothing, and writes nothing.
        let files_installed = tree(temp.path());
        let registry_installed = host.snapshot();
        let writes = host.writes;
        assert_eq!(install(&plan, &mut host).failures, 0);
        assert_eq!(tree(temp.path()), files_installed);
        assert_eq!(host.snapshot(), registry_installed);
        assert_eq!(
            host.writes, writes,
            "an unchanged registration is written again"
        );

        let report = uninstall(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(tree(temp.path()), files_before);
        let hosts_keys: Vec<&str> = plan.keys.iter().map(|k| k.hosts_key.as_str()).collect();
        assert_only_empty_parents_added(&registry_before, &host.snapshot(), |key| {
            hosts_keys
                .iter()
                .any(|hosts| hosts.starts_with(&format!(r"{key}\")))
        });

        // Uninstalling again is harmless.
        let registry_after = host.snapshot();
        assert_eq!(uninstall(&plan, &mut host).failures, 0);
        assert_eq!(tree(temp.path()), files_before);
        assert_eq!(host.snapshot(), registry_after);
    }

    #[test]
    fn keys_earlier_versions_wrote_for_brave_and_vivaldi_are_removed_when_they_are_ours() {
        let temp = temp_dir();
        let local = temp.path().join("LocalAppData");
        let layout = Layout::Windows {
            exe: temp.path().join("Wirecatch").join(windows_bridge()),
            local_app_data: local.clone(),
        };
        let plan = plan(&config(), &layout).unwrap();
        let manifest_path = data_folder(&local)
            .join("NativeMessagingHosts")
            .join(format!("{NATIVE_HOST_NAME}.chromium.json"));
        let ours = manifest_path.to_str().unwrap();
        let brave = hosts_key(r"Software\BraveSoftware\Brave-Browser");
        let brave = brave.as_str();
        let vivaldi = hosts_key(r"Software\Vivaldi");
        let vivaldi = vivaldi.as_str();
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

    /// The Chromium folders under `~/.config` that get a manifest on Linux.
    const LINUX_CHROMIUM_DIRS: [&[&str]; 8] = [
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

    /// Browser profiles and folders from before Wirecatch, which install and
    /// uninstall must leave alone.
    fn seed_linux_profiles(home: &Path) {
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
    }

    /// Checks the manifest in every Chromium folder and Firefox's.
    fn assert_linux_manifests(home: &Path, exe: &Path) {
        let config_dir = home.join(".config");
        for parts in LINUX_CHROMIUM_DIRS {
            let mut dir = config_dir.clone();
            dir.extend(parts);
            let manifest = json(
                &dir.join("NativeMessagingHosts")
                    .join(format!("{NATIVE_HOST_NAME}.json")),
            );
            assert_eq!(manifest["name"], NATIVE_HOST_NAME);
            assert_eq!(manifest["type"], "stdio");
            assert_eq!(manifest["path"], exe.to_str().unwrap());
            assert_eq!(manifest["allowed_origins"], serde_json::json!([ORIGIN]));
        }
        let firefox = json(
            &home
                .join(".mozilla")
                .join("native-messaging-hosts")
                .join(format!("{NATIVE_HOST_NAME}.json")),
        );
        assert_eq!(firefox["path"], exe.to_str().unwrap());
        assert_eq!(
            firefox["allowed_extensions"],
            serde_json::json!([FIREFOX_EXTENSION_ID])
        );
    }

    /// Checks that the plan writes one file per folder a browser reads, none
    /// inside a sandbox, and covers every browser with a Linux version.
    fn assert_linux_coverage(plan: &Plan, home: &Path, report: &Report) {
        // One file per folder: 8 Chromium folders and Firefox's, which LibreWolf,
        // Waterfox, Zen and Floorp read too. No Flatpak or Snap folder is written.
        assert_eq!(plan.files.len(), LINUX_CHROMIUM_DIRS.len() + 1);
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
    }

    #[test]
    fn linux_install_writes_every_manifest_and_uninstall_restores_the_state() {
        let temp = temp_dir();
        let home = temp.path().join("home");
        let exe = temp
            .path()
            .join("opt")
            .join("wirecatch")
            .join(BRIDGE_EXECUTABLE);
        let layout = Layout::linux(exe.clone(), home.clone(), None);
        let plan = plan(&config(), &layout).unwrap();
        let mut host = FakeHost::default();
        seed_linux_profiles(&home);
        let before = tree(temp.path());

        let report = install(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert!(plan.keys.is_empty(), "no registry on Linux");
        assert_linux_manifests(&home, &exe);
        assert_linux_coverage(&plan, &home, &report);

        let report = uninstall(&plan, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        let hosts_dirs: Vec<PathBuf> = plan
            .files
            .iter()
            .map(|f| f.hosts_dir.strip_prefix(temp.path()).unwrap().to_path_buf())
            .collect();
        assert_only_empty_parents_added(&before, &tree(temp.path()), |dir| {
            hosts_dirs
                .iter()
                .any(|hosts| hosts.starts_with(dir) && hosts != dir)
        });
    }

    #[test]
    fn linux_honors_xdg_config_home() {
        let layout = Layout::linux(
            Path::new("/opt/wirecatch").join(BRIDGE_EXECUTABLE),
            PathBuf::from("/home/u"),
            Some(PathBuf::from("/home/u/cfg")),
        );
        let plan = plan(&config(), &layout);
        // Paths are only absolute on Unix; the layout logic is the same.
        if cfg!(unix) {
            let plan = plan.unwrap();
            assert_eq!(
                plan.files[0].path,
                Path::new(&format!(
                    "/home/u/cfg/google-chrome/NativeMessagingHosts/{NATIVE_HOST_NAME}.json"
                ))
            );
            let firefox = Path::new("/home/u/.mozilla/native-messaging-hosts")
                .join(format!("{NATIVE_HOST_NAME}.json"));
            assert!(
                plan.files.iter().any(|file| file.path == firefox),
                "Firefox ignores XDG_CONFIG_HOME"
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
        let temp = temp_dir();
        let local = temp.path().join("LocalAppData");
        let mine = plan(
            &config(),
            &Layout::Windows {
                exe: temp.path().join("a").join(windows_bridge()),
                local_app_data: local.clone(),
            },
        )
        .unwrap();
        let theirs = plan(
            &config(),
            &Layout::Windows {
                exe: temp.path().join("b").join(windows_bridge()),
                local_app_data: local,
            },
        )
        .unwrap();
        for (a, b) in mine.keys.iter().zip(&theirs.keys) {
            assert_eq!((&a.key, &a.value), (&b.key, &b.value));
        }

        let mut host = FakeHost::default();
        assert_eq!(install(&theirs, &mut host).failures, 0);
        let files = tree(temp.path());
        let registry = host.snapshot();

        let report = uninstall(&mine, &mut host);
        assert_eq!(report.failures, 0, "{:?}", report.lines);
        assert_eq!(
            host.snapshot(),
            registry,
            "their registry keys were removed"
        );
        assert_eq!(tree(temp.path()), files, "their manifests were removed");

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
        let temp = temp_dir();
        let plan = plan(
            &config(),
            &Layout::Windows {
                exe: temp.path().join("a").join(windows_bridge()),
                local_app_data: temp.path().join("LocalAppData"),
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
            exe: PathBuf::from(windows_bridge()),
            local_app_data: std::env::temp_dir(),
        };
        assert!(plan(&config(), &layout).is_err());
    }

    /// Exercises the real HKCU code under a throwaway key, then removes it.
    #[cfg(windows)]
    #[test]
    fn real_registry_round_trip() {
        let root = format!(r"Software\WirecatchBridgeTest-{}", std::process::id());
        let key = hosts_key(&root);
        let mut host = RealHost;
        assert_eq!(host.registry_value(Hive::CurrentUser, &key).unwrap(), None);
        host.set_registry_value(Hive::CurrentUser, &key, r"C:\path\manifest.json")
            .unwrap();
        let read_back = host.registry_value(Hive::CurrentUser, &key);
        // A key with a subkey is not empty, so it stays.
        let kept = host.delete_registry_key_if_empty(Hive::CurrentUser, &root);
        let refused = host.delete_registry_key(Hive::CurrentUser, &root);
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
        assert!(refused.is_err(), "a key with subkeys is never deleted");
        deleted.unwrap();
        assert!(hosts_pruned.unwrap());
        assert!(root_pruned.unwrap());
        assert!(!missing.unwrap());
        assert_eq!(host.registry_value(Hive::CurrentUser, &key).unwrap(), None);
    }

    /// A per-machine install: the bridge in Program Files, registered for every user, and the same user plan the app runs on each start.
    fn machine_and_user(temp: &PrivateDir, exe: &Path) -> (Plan, Plan) {
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
                local_app_data: temp.path().join("LocalAppData"),
            },
        )
        .unwrap();
        (machine, user)
    }

    #[test]
    fn a_machine_registration_lives_next_to_the_bridge_and_under_local_machine() {
        let temp = temp_dir();
        let exe = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
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
        let temp = temp_dir();
        let exe = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
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
        let temp = temp_dir();
        let exe = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
        let (machine, user) = machine_and_user(&temp, &exe);
        let deleted = temp.path().join("Deleted").join(windows_bridge());
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
        let temp = temp_dir();
        let exe = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
        let (machine, user) = machine_and_user(&temp, &exe);
        let portable = temp.path().join("Portable").join(windows_bridge());
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
        let temp = temp_dir();
        let installed = temp
            .path()
            .join("Program Files")
            .join("Wirecatch")
            .join(windows_bridge());
        let portable = temp.path().join("Portable").join(windows_bridge());
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
