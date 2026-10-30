//! The tray icon (D104): its menu (Open, Pause all, Resume all, Proxy, Quit), a left click that
//! opens the window, and a tooltip with the live speed or why the download list could not open.

use gpui_kit::{App, Entity, Global};
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wirecatch_shell::activity::Activity;
use wirecatch_shell::tray::{Shell, TrayAction, perform, problem_tooltip};
use wirecatch_shell::tray_proxy::{self, ProxyItem};

use crate::backend::Backend;
use crate::library::{Asked, Library, Loaded};
use crate::{lifecycle, main_window};

const ICON_PNG: &[u8] = include_bytes!("../../../apps/desktop/src-tauri/icons/32x32.png");

/// The icon in the tray, with what it shows now, so it is changed only when that changes.
struct Tray {
    icon: TrayIcon,
    tooltip: String,
    proxies: Vec<ProxyItem>,
}

impl Global for Tray {}

/// What was clicked, sent from the tray's own thread or window procedure.
enum Clicked {
    Icon,
    Menu(String),
}

/// Puts the icon in the tray, following `library`. Without one, closing the window quits.
pub fn start(library: &Entity<Library>, cx: &mut App) {
    let icon = match build() {
        Ok(icon) => icon,
        Err(e) => {
            tracing::warn!("no tray icon, so closing the window quits: {e}");
            return;
        }
    };
    let (clicks, mut clicked) = tokio::sync::mpsc::unbounded_channel();
    let menu_clicks = clicks.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        _ = menu_clicks.send(Clicked::Menu(event.id.0));
    }));
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            _ = clicks.send(Clicked::Icon);
        }
    }));
    cx.set_global(Tray {
        icon,
        tooltip: String::new(),
        proxies: Vec::new(),
    });
    cx.spawn(async move |cx| {
        while let Some(click) = clicked.recv().await {
            cx.update(|cx| on_click(click, cx));
        }
    })
    .detach();
    cx.observe(library, |library, cx| follow(&library, cx))
        .detach();
    follow(library, cx);
}

/// Whether the icon is in the tray.
pub fn exists(cx: &App) -> bool {
    cx.has_global::<Tray>()
}

/// Takes the icon out of the tray.
pub fn remove(cx: &mut App) {
    if exists(cx) {
        cx.remove_global::<Tray>();
    }
}

fn build() -> Result<TrayIcon, String> {
    let menu = menu(&[]).map_err(|e| e.to_string())?;
    TrayIconBuilder::new()
        .with_icon(icon()?)
        .with_tooltip(wirecatch_shell::activity::tooltip(0, 0))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())
}

fn icon() -> Result<Icon, String> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(ICON_PNG))
        .read_info()
        .map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or("the tray icon is too large")?;
    let mut rgba = vec![0; size];
    let frame = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return Err("the tray icon is not 8-bit RGBA".to_owned());
    }
    rgba.truncate(frame.buffer_size());
    Icon::from_rgba(rgba, frame.width, frame.height).map_err(|e| e.to_string())
}

/// The tray's menu, with `proxies` in its Proxy submenu (none until the manager has opened).
fn menu(proxies: &[ProxyItem]) -> tray_icon::menu::Result<Menu> {
    let item = |a: TrayAction| MenuItem::with_id(a.id(), a.label(), true, None);
    let choices: Vec<CheckMenuItem> = proxies
        .iter()
        .map(|p| CheckMenuItem::with_id(&p.id, &p.label, true, p.checked, None))
        .collect();
    let choices: Vec<&dyn IsMenuItem> = choices.iter().map(|c| c as &dyn IsMenuItem).collect();
    let proxy = Submenu::with_items("Proxy", !proxies.is_empty(), &choices)?;
    Menu::with_items(&[
        &item(TrayAction::Open),
        &PredefinedMenuItem::separator(),
        &item(TrayAction::PauseAll),
        &item(TrayAction::ResumeAll),
        &PredefinedMenuItem::separator(),
        &proxy,
        &PredefinedMenuItem::separator(),
        &item(TrayAction::Quit),
    ])
}

/// Shows the library's speed, or why it could not open, and its proxies.
fn follow(library: &Entity<Library>, cx: &mut App) {
    let library = library.read(cx);
    let tooltip = match &library.loaded {
        Loaded::Failed(reason) => problem_tooltip(&reason.message),
        Loaded::Loading | Loaded::Ready => {
            let mut activity = Activity::default();
            activity.load(&library.downloads);
            activity.tooltip()
        }
    };
    let proxies = library
        .settings
        .as_ref()
        .map(|settings| tray_proxy::items(&settings.proxy, &library.proxies))
        .unwrap_or_default();
    let Some(tray) = cx.try_global::<Tray>() else {
        return;
    };
    let (new_tooltip, new_proxies) = (tray.tooltip != tooltip, tray.proxies != proxies);
    if !new_tooltip && !new_proxies {
        return;
    }
    let tray = cx.global_mut::<Tray>();
    if new_tooltip && let Err(e) = tray.icon.set_tooltip(Some(&tooltip)) {
        tracing::debug!("could not change the tray tooltip: {e}");
    }
    if new_proxies {
        match menu(&proxies) {
            Ok(menu) => tray.icon.set_menu(Some(Box::new(menu))),
            Err(e) => tracing::debug!("could not change the tray menu: {e}"),
        }
    }
    tray.tooltip = tooltip;
    tray.proxies = proxies;
}

struct AppShell<'a>(&'a mut App);

impl Shell for AppShell<'_> {
    fn open_window(&mut self) {
        main_window::bring_up(&Asked::Window, self.0);
    }

    fn quit(&mut self) {
        lifecycle::quit(self.0);
    }
}

fn on_click(click: Clicked, cx: &mut App) {
    let id = match click {
        Clicked::Icon => return main_window::bring_up(&Asked::Window, cx),
        Clicked::Menu(id) => id,
    };
    let (manager, runtime) = Backend::now(cx);
    // Pausing and resuming start and stop downloads, which run on the manager's runtime.
    let _inside = runtime.enter();
    if let Some(proxy) = tray_proxy::proxy_of(&id) {
        if let Some(manager) = &manager
            && let Err(e) = tray_proxy::switch(manager, proxy)
        {
            tracing::warn!("the tray could not switch the proxy: {e}");
        }
        return;
    }
    let Some(action) = TrayAction::from_id(&id) else {
        return;
    };
    if let Err(e) = perform(action, manager.as_ref(), &mut AppShell(cx)) {
        tracing::warn!("tray action {} failed: {e}", action.id());
    }
}
