//! What the tray icon's menu does, apart from drawing it: the actions, and the tooltip when the
//! download list could not open.

use wirecatch_core::Manager;
use wirecatch_protocol::wire::{ErrorCode, ProtocolError};

use crate::api::{self, CommandResult};

/// Something the tray menu can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show the window, creating it if it was closed to the tray.
    Open,
    /// Open the Add dialog, with the links on the clipboard.
    NewDownload,
    /// Open the default download folder.
    OpenFolder,
    /// Pause every download.
    PauseAll,
    /// Resume every paused download.
    ResumeAll,
    /// Stop every download so it continues next time, then exit.
    Quit,
}

impl TrayAction {
    /// Every action, in menu order.
    pub const ALL: [TrayAction; 6] = [
        Self::Open,
        Self::NewDownload,
        Self::OpenFolder,
        Self::PauseAll,
        Self::ResumeAll,
        Self::Quit,
    ];

    /// The menu item id.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::NewDownload => "new_download",
            Self::OpenFolder => "open_folder",
            Self::PauseAll => "pause_all",
            Self::ResumeAll => "resume_all",
            Self::Quit => "quit",
        }
    }

    /// The menu item text, naming the running app by its channel's name.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Open => format!("Open {}", crate::host::CHANNEL.name()),
            Self::NewDownload => "New download...".to_owned(),
            Self::OpenFolder => "Open downloads folder".to_owned(),
            Self::PauseAll => "Pause all".to_owned(),
            Self::ResumeAll => "Resume all".to_owned(),
            Self::Quit => "Quit".to_owned(),
        }
    }

    /// The action of a menu item id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// What tray actions need from the app around them.
pub trait Shell {
    /// Shows the main window.
    fn open_window(&mut self);
    /// Shows the main window with the Add dialog open.
    fn new_download(&mut self);
    /// Opens `folder` in the file manager, once it is found to be a folder.
    fn open_folder(&mut self, folder: &std::path::Path);
    /// Starts quitting: the manager shuts down, then the process exits.
    fn quit(&mut self);
}

/// Runs a tray action. `manager` is `None` while the manager is still opening (or could not
/// open); pausing and resuming then explain that.
///
/// # Errors
///
/// Pause all, Resume all or Open downloads folder while the manager is still opening, or once it
/// has shut down; the default folder names no folder on this machine.
pub fn perform(
    action: TrayAction,
    manager: Option<&Manager>,
    shell: &mut impl Shell,
) -> CommandResult<()> {
    let ready = || {
        manager.ok_or_else(|| ProtocolError {
            code: ErrorCode::AppUnavailable,
            message: "Wirecatch is still starting. Try again in a moment.".to_owned(),
        })
    };
    match action {
        TrayAction::Open => shell.open_window(),
        TrayAction::NewDownload => shell.new_download(),
        TrayAction::OpenFolder => {
            let folder = ready()?.default_folder().map_err(|e| e.to_protocol())?;
            shell.open_folder(&folder);
        }
        TrayAction::PauseAll => api::pause_all(ready()?)?,
        TrayAction::ResumeAll => api::resume_all(ready()?)?,
        TrayAction::Quit => shell.quit(),
    }
    Ok(())
}

/// Longest tooltip every platform shows in full (Windows cuts tray tooltips at 127 characters).
pub const MAX_TOOLTIP_CHARS: usize = 127;

/// The tooltip when something keeps Wirecatch from working, cut to fit.
#[must_use]
pub fn problem_tooltip(reason: &str) -> String {
    let text = format!("{} could not start: {reason}", crate::host::CHANNEL.name());
    if text.chars().count() <= MAX_TOOLTIP_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_TOOLTIP_CHARS - 3).collect();
    cut.push_str("...");
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeShell {
        calls: Vec<&'static str>,
    }

    impl Shell for FakeShell {
        fn open_window(&mut self) {
            self.calls.push("open");
        }
        fn quit(&mut self) {
            self.calls.push("quit");
        }
        fn new_download(&mut self) {
            self.calls.push("new download");
        }
        fn open_folder(&mut self, _: &std::path::Path) {
            self.calls.push("folder");
        }
    }

    #[test]
    fn menu_ids_round_trip_and_read_plainly() {
        for a in TrayAction::ALL {
            assert_eq!(TrayAction::from_id(a.id()), Some(a));
            assert!(!a.label().contains('\u{2014}'));
        }
        assert_eq!(TrayAction::from_id("something else"), None);
        // The running app names itself: "Open Wirecatch Dev" in a debug build.
        let open = format!("Open {}", crate::host::CHANNEL.name());
        assert_eq!(
            TrayAction::ALL.map(TrayAction::label),
            [
                open.as_str(),
                "New download...",
                "Open downloads folder",
                "Pause all",
                "Resume all",
                "Quit"
            ]
        );
    }

    #[test]
    fn a_problem_fits_in_the_tooltip() {
        assert_eq!(
            problem_tooltip("the disk is full."),
            format!(
                "{} could not start: the disk is full.",
                crate::host::CHANNEL.name()
            )
        );
        let long = problem_tooltip(&"x".repeat(500));
        assert_eq!(long.chars().count(), MAX_TOOLTIP_CHARS);
        assert!(long.ends_with("..."));
    }

    #[test]
    fn open_and_quit_go_to_the_shell() {
        let mut shell = FakeShell::default();
        perform(TrayAction::Open, None, &mut shell).unwrap();
        perform(TrayAction::NewDownload, None, &mut shell).unwrap();
        perform(TrayAction::Quit, None, &mut shell).unwrap();
        assert_eq!(shell.calls, ["open", "new download", "quit"]);
    }

    #[test]
    fn pausing_before_the_manager_opens_explains_why_it_cannot() {
        let mut shell = FakeShell::default();
        let err = perform(TrayAction::PauseAll, None, &mut shell).unwrap_err();
        assert_eq!(err.code, ErrorCode::AppUnavailable);
        assert!(err.message.contains("still starting"));
        assert_eq!(shell.calls, [] as [&str; 0]);
    }
}
