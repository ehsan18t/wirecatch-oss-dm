//! The tray icon: its menu (Open Wirecatch, Pause all, Resume all, Quit), a left click that
//! opens the window, and a tooltip with the live speed.

use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _, Runtime};

pub use wirecatch_shell::tray::{MAX_TOOLTIP_CHARS, Shell, TrayAction, perform, problem_tooltip};

use crate::host::Core;
use crate::tray_proxy::{self, ProxyItem};
use crate::{lifecycle, window};

/// Id of the one tray icon.
pub const TRAY_ID: &str = "main";

struct AppShell<'a, R: Runtime>(&'a AppHandle<R>);

impl<R: Runtime> Shell for AppShell<'_, R> {
    fn open_window(&mut self) {
        window::show_main_window(self.0);
    }

    fn quit(&mut self) {
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
    if let Err(e) = perform(action, manager, &mut AppShell(app)) {
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

/// Changes the tooltip.
pub fn set_tooltip<R: Runtime>(app: &AppHandle<R>, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID)
        && let Err(e) = tray.set_tooltip(Some(text))
    {
        tracing::debug!("could not update the tray tooltip: {e}");
    }
}
