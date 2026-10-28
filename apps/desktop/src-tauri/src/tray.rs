//! The tray icon: its menu (Open Wirecatch, Pause all, Resume all, Quit), a left click that
//! opens the window, and a tooltip with the live speed.

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _, Runtime};
use wirecatch_core::Manager;

use crate::api;
use crate::host::Core;
use crate::tray_proxy::{self, ProxyItem};
use crate::{lifecycle, window};

/// Id of the one tray icon.
pub const TRAY_ID: &str = "main";

/// Something the tray menu can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show the window, creating it if it was closed to the tray.
    Open,
    /// Pause every download.
    PauseAll,
    /// Resume every paused download.
    ResumeAll,
    /// Stop every download so it continues next time, then exit.
    Quit,
}

impl TrayAction {
    /// Every action, in menu order.
    pub const ALL: [TrayAction; 4] = [Self::Open, Self::PauseAll, Self::ResumeAll, Self::Quit];

    /// The menu item id.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
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
    fn open_window(&self);
    /// Starts quitting: the manager shuts down, then the process exits.
    fn quit(&self);
}

/// Runs a tray action. `manager` is `None` while the manager is still opening (or could not
/// open); pausing and resuming then explain that.
///
/// # Errors
///
/// Pause all or Resume all while the manager is still opening, or once it has shut down.
pub fn perform(
    action: TrayAction,
    manager: Option<&Manager>,
    shell: &impl Shell,
) -> Result<(), String> {
    let ready =
        || manager.ok_or_else(|| "Wirecatch is still starting. Try again in a moment.".to_owned());
    match action {
        TrayAction::Open => shell.open_window(),
        TrayAction::PauseAll => api::pause_all(ready()?).map_err(|e| e.message)?,
        TrayAction::ResumeAll => api::resume_all(ready()?).map_err(|e| e.message)?,
        TrayAction::Quit => shell.quit(),
    }
    Ok(())
}

struct AppShell<'a, R: Runtime>(&'a AppHandle<R>);

impl<R: Runtime> Shell for AppShell<'_, R> {
    fn open_window(&self) {
        window::show_main_window(self.0);
    }

    fn quit(&self) {
        lifecycle::quit(self.0);
    }
}

/// Runs the tray menu item with this id.
pub fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let core = app.try_state::<Core>();
    let manager = core.as_ref().and_then(|c| c.get());
    if let Some(proxy) = tray_proxy::proxy_of(id) {
        if let Some(manager) = manager
            && let Err(e) = tray_proxy::switch(manager, proxy)
        {
            tracing::warn!("the tray could not switch the proxy: {e}");
        }
        return;
    }
    let Some(action) = TrayAction::from_id(id) else {
        return;
    };
    if let Err(e) = perform(action, manager, &AppShell(app)) {
        tracing::warn!("tray action {} failed: {e}", action.id());
    }
}

/// Puts the icon in the tray.
///
/// # Errors
///
/// The menu or the icon could not be made: some Linux desktops have no tray at all.
/// The tray's menu, with `proxies` in its Proxy submenu (none until the manager has opened).
fn menu<R: Runtime>(app: &AppHandle<R>, proxies: &[ProxyItem]) -> tauri::Result<Menu<R>> {
    let item = |a: TrayAction| MenuItem::with_id(app, a.id(), a.label(), true, None::<&str>);
    let open = item(TrayAction::Open)?;
    let pause = item(TrayAction::PauseAll)?;
    let resume = item(TrayAction::ResumeAll)?;
    let quit = item(TrayAction::Quit)?;
    let line = PredefinedMenuItem::separator(app)?;
    let line2 = PredefinedMenuItem::separator(app)?;
    let line3 = PredefinedMenuItem::separator(app)?;
    let choices = proxies
        .iter()
        .map(|p| CheckMenuItem::with_id(app, &p.id, &p.label, true, p.checked, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let choices: Vec<&dyn IsMenuItem<R>> =
        choices.iter().map(|c| c as &dyn IsMenuItem<R>).collect();
    let proxy = Submenu::with_items(app, "Proxy", !proxies.is_empty(), &choices)?;
    Menu::with_items(
        app,
        &[&open, &line, &pause, &resume, &line2, &proxy, &line3, &quit],
    )
}

/// Shows the app's proxy and the saved profiles in the Proxy submenu.
pub fn set_proxies<R: Runtime>(app: &AppHandle<R>, proxies: &[ProxyItem]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Err(e) = menu(app, proxies).and_then(|m| tray.set_menu(Some(m))) {
        tracing::debug!("could not update the tray menu: {e}");
    }
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = menu(app, &[])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(crate::activity::tooltip(0, 0))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
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

/// Changes the tooltip.
pub fn set_tooltip<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID)
        && let Err(e) = tray.set_tooltip(Some(text))
    {
        tracing::debug!("could not update the tray tooltip: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct FakeShell {
        calls: RefCell<Vec<&'static str>>,
    }

    impl Shell for FakeShell {
        fn open_window(&self) {
            self.calls.borrow_mut().push("open");
        }
        fn quit(&self) {
            self.calls.borrow_mut().push("quit");
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
            [open.as_str(), "Pause all", "Resume all", "Quit"]
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
        let shell = FakeShell::default();
        perform(TrayAction::Open, None, &shell).unwrap();
        perform(TrayAction::Quit, None, &shell).unwrap();
        assert_eq!(*shell.calls.borrow(), ["open", "quit"]);
    }

    #[test]
    fn pausing_before_the_manager_opens_explains_why_it_cannot() {
        let shell = FakeShell::default();
        let err = perform(TrayAction::PauseAll, None, &shell).unwrap_err();
        assert!(err.contains("still starting"));
        assert!(shell.calls.borrow().is_empty());
    }
}
