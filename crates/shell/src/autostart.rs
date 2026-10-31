//! Starting with the system, kept in step with `AppSettings.start_with_system`.
//!
//! While the setting is on, the entry is written again on every start, so a copy of the app that
//! moved (a new install folder, a moved portable copy) still starts from the right place. If the
//! person turned the entry off outside Wirecatch (Task Manager's Startup tab on Windows, the
//! desktop's startup settings on Linux), Wirecatch follows that choice and turns its own setting
//! off instead of quietly turning the entry back on. Turning the setting on in Wirecatch is a
//! fresh choice and wins.

use std::io;
use std::path::{Path, PathBuf};

use wirecatch_core::Manager;
use wirecatch_protocol::app::AppSettingsPatch;

use crate::launch::BACKGROUND_FLAG;

/// Why the start-at-sign-in entry could not be brought in line with the setting.
#[derive(Debug, thiserror::Error)]
pub enum AutostartError {
    #[error("could not write the start-at-sign-in entry ({0})")]
    Write(#[source] io::Error),
    #[error("could not remove the start-at-sign-in entry ({0})")]
    Remove(#[source] io::Error),
    #[error("could not turn the setting off ({0})")]
    Setting(#[from] wirecatch_core::CoreError),
}

/// The state of the system's start-at-sign-in entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// No entry.
    Missing,
    /// An entry that runs at sign-in.
    On,
    /// An entry the person turned off outside Wirecatch.
    TurnedOff,
}

/// What to do about the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write the entry (again), turned on.
    Write,
    /// Remove the entry.
    Remove,
    /// Leave the entry and turn the setting off, to match the person's choice elsewhere.
    TurnSettingOff,
    /// Nothing to do.
    Nothing,
}

/// Decides what to do. `just_turned_on` is set when the person switched the setting on in
/// Wirecatch a moment ago, rather than at start or after some other settings change.
#[must_use]
pub fn decide(wanted: bool, state: EntryState, just_turned_on: bool) -> Action {
    match (wanted, state) {
        (true, EntryState::TurnedOff) if !just_turned_on => Action::TurnSettingOff,
        (true, _) => Action::Write,
        (false, EntryState::Missing) => Action::Nothing,
        (false, _) => Action::Remove,
    }
}

/// Whether a Windows `StartupApproved\Run` value says the entry is turned off. Task Manager
/// writes an odd first byte (for example `03`) for a disabled entry and an even one (`02`) for
/// an enabled one.
#[must_use]
pub fn approved_value_turned_off(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|b| b & 1 == 1)
}

/// Whether a Linux autostart `.desktop` entry is turned off.
pub fn desktop_entry_turned_off(text: &str) -> bool {
    text.lines().map(str::trim).any(|line| {
        let line = line.to_ascii_lowercase().replace(' ', "");
        line == "hidden=true" || line == "x-gnome-autostart-enabled=false"
    })
}

/// What changes the start-at-sign-in entry: the system's own, or a stand-in.
pub trait Launcher {
    /// Writes the entry, turned on.
    ///
    /// # Errors
    ///
    /// The system refused to write the entry (the registry or the autostart folder).
    fn enable(&self) -> io::Result<()>;
    /// Removes the entry.
    ///
    /// # Errors
    ///
    /// The system refused to remove the entry.
    fn disable(&self) -> io::Result<()>;
}

/// Carries out one start-at-sign-in step: decides from the entry's `state` and does it, through
/// `launcher` or, to follow a choice made outside Wirecatch, by turning the setting off.
///
/// # Errors
///
/// The `launcher` could not write or remove the entry, or the setting could not be saved (the
/// manager has shut down).
pub fn apply(
    launcher: &impl Launcher,
    manager: &Manager,
    state: EntryState,
    wanted: bool,
    just_turned_on: bool,
) -> Result<Action, AutostartError> {
    let action = decide(wanted, state, just_turned_on);
    match action {
        Action::Write => launcher.enable().map_err(AutostartError::Write)?,
        Action::Remove => launcher.disable().map_err(AutostartError::Remove)?,
        Action::Nothing => {}
        Action::TurnSettingOff => {
            tracing::info!("starting with the system was turned off outside Wirecatch");
            // Only this one setting, so a change made meanwhile in a window is kept.
            manager.change_settings(AppSettingsPatch {
                start_with_system: Some(false),
                ..AppSettingsPatch::default()
            })?;
        }
    }
    Ok(action)
}

/// The system's own entry named `name`, which starts `program` with `--background`.
pub struct Entry<'a> {
    pub name: &'a str,
    pub program: &'a Path,
}

impl Entry<'_> {
    /// Makes the entry match the setting (`wanted`); blocks on the registry or the disk.
    /// `just_turned_on` is set when the person switched the setting on a moment ago.
    pub fn sync(&self, manager: &Manager, wanted: bool, just_turned_on: bool) {
        let state = read_state(self.name);
        if let Err(e) = apply(self, manager, state, wanted, just_turned_on) {
            tracing::warn!("could not change starting with the system: {e}");
        }
    }
}

impl Launcher for Entry<'_> {
    fn enable(&self) -> io::Result<()> {
        write_entry(self.name, self.program)
    }

    fn disable(&self) -> io::Result<()> {
        remove_entry(self.name)
    }
}

/// The program the entry starts: the AppImage the app runs from, else the running program.
///
/// # Errors
///
/// The system could not say where the running program is.
pub fn program() -> io::Result<PathBuf> {
    match std::env::var_os(crate::appimage::APPIMAGE_VAR) {
        Some(appimage) if !appimage.is_empty() => Ok(PathBuf::from(appimage)),
        _ => std::env::current_exe(),
    }
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The `Run` value's command: the program quoted, since an unquoted path with a space in it runs
/// whatever program its first word names.
#[cfg(any(windows, test))]
fn windows_command(program: &Path) -> String {
    format!("\"{}\" {BACKGROUND_FLAG}", program.display())
}

/// Reads the state of the entry named `name`.
#[cfg(windows)]
pub fn read_state(name: &str) -> EntryState {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let present = hkcu
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|k| k.get_value::<String, _>(name))
        .is_ok();
    if !present {
        return EntryState::Missing;
    }
    let turned_off = hkcu
        .open_subkey_with_flags(APPROVED_KEY, KEY_READ)
        .and_then(|k| k.get_raw_value(name))
        .is_ok_and(|v| approved_value_turned_off(&v.bytes));
    if turned_off {
        EntryState::TurnedOff
    } else {
        EntryState::On
    }
}

#[cfg(windows)]
fn write_entry(name: &str, program: &Path) -> io::Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY};
    use winreg::{RegKey, RegValue};

    /// What Task Manager writes for an entry that is turned on.
    const APPROVED_ON: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY)?;
    run.set_value(name, &windows_command(program))?;
    // A fresh choice in Wirecatch turns back on an entry turned off in Task Manager.
    if let Ok(approved) = hkcu.open_subkey_with_flags(APPROVED_KEY, KEY_SET_VALUE) {
        approved.set_raw_value(
            name,
            &RegValue {
                vtype: REG_BINARY,
                bytes: APPROVED_ON.to_vec(),
            },
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn remove_entry(name: &str) -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key in [RUN_KEY, APPROVED_KEY] {
        let removed = hkcu
            .open_subkey_with_flags(key, KEY_SET_VALUE)
            .and_then(|k| k.delete_value(name));
        match removed {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// The desktop's autostart folder (the XDG Autostart specification).
#[cfg(not(windows))]
fn autostart_folder() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|c| !c.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(config.join("autostart"))
}

/// Reads the state of the entry named `name`.
#[cfg(not(windows))]
#[must_use]
pub fn read_state(name: &str) -> EntryState {
    autostart_folder().map_or(EntryState::Missing, |folder| state_in(&folder, name))
}

#[cfg(any(not(windows), test))]
fn state_in(folder: &Path, name: &str) -> EntryState {
    match std::fs::read_to_string(folder.join(format!("{name}.desktop"))) {
        Err(_) => EntryState::Missing,
        Ok(text) if desktop_entry_turned_off(&text) => EntryState::TurnedOff,
        Ok(_) => EntryState::On,
    }
}

#[cfg(not(windows))]
fn write_entry(name: &str, program: &Path) -> io::Result<()> {
    let folder = autostart_folder()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
    write_in(&folder, name, program)
}

#[cfg(not(windows))]
fn remove_entry(name: &str) -> io::Result<()> {
    let Some(folder) = autostart_folder() else {
        return Ok(());
    };
    match std::fs::remove_file(folder.join(format!("{name}.desktop"))) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(any(not(windows), test))]
fn write_in(folder: &Path, name: &str, program: &Path) -> io::Result<()> {
    std::fs::create_dir_all(folder)?;
    std::fs::write(
        folder.join(format!("{name}.desktop")),
        desktop_entry(name, program),
    )
}

/// The autostart `.desktop` entry that starts `program` in the background.
#[cfg(any(not(windows), test))]
fn desktop_entry(name: &str, program: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={} {BACKGROUND_FLAG}\nStartupNotify=false\nTerminal=false\n",
        exec_word(&program.to_string_lossy())
    )
}

/// `text` as one word of a `.desktop` file's `Exec` line (the Desktop Entry Specification):
/// quoted when it holds a reserved character, and `%` doubled, which the line reads as a field code.
#[cfg(any(not(windows), test))]
fn exec_word(text: &str) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let text = text.replace('%', "%%");
    if !text.chars().any(|c| RESERVED.contains(c)) {
        return text;
    }
    let mut quoted = String::from('"');
    for c in text.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_entry_quotes_the_program() {
        assert_eq!(
            windows_command(Path::new(r"C:\Program Files\Wirecatch\wirecatch.exe")),
            r#""C:\Program Files\Wirecatch\wirecatch.exe" --background"#
        );
    }

    #[test]
    fn a_program_path_stays_one_word_of_the_exec_line() {
        assert_eq!(
            exec_word("/opt/wirecatch/wirecatch"),
            "/opt/wirecatch/wirecatch"
        );
        assert_eq!(
            exec_word("/home/a b/Wirecatch $1 \"x\".AppImage"),
            r#""/home/a b/Wirecatch \$1 \"x\".AppImage""#
        );
        assert_eq!(exec_word("/opt/100%/w"), "/opt/100%%/w");
    }

    #[test]
    fn a_written_entry_reads_as_on_and_starts_in_the_background() {
        let folder = tempfile::tempdir().unwrap();
        let autostart = folder.path().join("autostart");
        assert_eq!(state_in(&autostart, "Wirecatch"), EntryState::Missing);
        write_in(
            &autostart,
            "Wirecatch",
            Path::new("/opt/wire catch/wirecatch"),
        )
        .unwrap();
        assert_eq!(state_in(&autostart, "Wirecatch"), EntryState::On);
        let text = std::fs::read_to_string(autostart.join("Wirecatch.desktop")).unwrap();
        assert!(
            text.contains("Exec=\"/opt/wire catch/wirecatch\" --background\n"),
            "{text}"
        );
    }

    #[test]
    fn the_entry_is_rewritten_on_every_start_while_wanted() {
        assert_eq!(decide(true, EntryState::On, false), Action::Write);
        assert_eq!(decide(true, EntryState::Missing, false), Action::Write);
    }

    #[test]
    fn a_choice_made_outside_wirecatch_is_followed_not_overruled() {
        assert_eq!(
            decide(true, EntryState::TurnedOff, false),
            Action::TurnSettingOff
        );
        // Switching it on again in Wirecatch is a fresh choice.
        assert_eq!(decide(true, EntryState::TurnedOff, true), Action::Write);
    }

    #[test]
    fn turning_it_off_removes_the_entry() {
        assert_eq!(decide(false, EntryState::On, false), Action::Remove);
        assert_eq!(decide(false, EntryState::TurnedOff, false), Action::Remove);
        assert_eq!(decide(false, EntryState::Missing, false), Action::Nothing);
    }

    #[test]
    fn turned_off_entries_are_recognized() {
        assert!(approved_value_turned_off(&[
            3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8
        ]));
        assert!(!approved_value_turned_off(&[
            2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]));
        assert!(!approved_value_turned_off(&[]));
        assert!(desktop_entry_turned_off("[Desktop Entry]\nHidden=true\n"));
        assert!(desktop_entry_turned_off(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false"
        ));
        assert!(!desktop_entry_turned_off(
            "[Desktop Entry]\nExec=wirecatch-app --background"
        ));
    }
}
