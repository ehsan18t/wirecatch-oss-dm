//! Starting with the system, kept in step with `AppSettings.start_with_system`.
//!
//! While the setting is on, the entry is written again on every start, so a copy of the app that
//! moved (a new install folder, a moved portable copy) still starts from the right place. If the
//! person turned the entry off outside Wirecatch (Task Manager's Startup tab on Windows, the
//! desktop's startup settings on Linux), Wirecatch follows that choice and turns its own setting
//! off instead of quietly turning the entry back on. Turning the setting on in Wirecatch is a
//! fresh choice and wins.

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

/// Reads the state of the entry named `name`.
#[cfg(windows)]
pub fn read_state(name: &str) -> EntryState {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let present = hkcu
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_READ)
        .and_then(|k| k.get_value::<String, _>(name))
        .is_ok();
    if !present {
        return EntryState::Missing;
    }
    let turned_off = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            KEY_READ,
        )
        .and_then(|k| k.get_raw_value(name))
        .is_ok_and(|v| approved_value_turned_off(&v.bytes));
    if turned_off {
        EntryState::TurnedOff
    } else {
        EntryState::On
    }
}

/// Reads the state of the entry named `name`.
#[cfg(not(windows))]
pub fn read_state(name: &str) -> EntryState {
    let Some(config) = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))
    else {
        return EntryState::Missing;
    };
    match std::fs::read_to_string(config.join("autostart").join(format!("{name}.desktop"))) {
        Err(_) => EntryState::Missing,
        Ok(text) if desktop_entry_turned_off(&text) => EntryState::TurnedOff,
        Ok(_) => EntryState::On,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
