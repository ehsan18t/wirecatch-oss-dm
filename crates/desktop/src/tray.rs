//! The tray icon (D104): its menu (Open, New download, the downloads folder, Pause all, Resume
//! all, Queues, Speed limit, Proxy, Quit), a left click that opens the window, and a tooltip with
//! the live speed or why the download list could not open.

use gpui_kit::{App, Entity, Global};
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wirecatch_shell::activity::Activity;
use wirecatch_shell::tray::{Shell, TrayAction, perform, problem_tooltip};
use wirecatch_shell::tray_proxy::{self, ProxyItem};
use wirecatch_shell::tray_queues::{self, QueueItem};
use wirecatch_shell::tray_speed::{self, SpeedItem};

use crate::backend::Backend;
use crate::library::{Asked, Library, Loaded};
use crate::{lifecycle, links, main_window};

const ICON_PNG: &[u8] = include_bytes!("../assets/icons/32x32.png");

/// The icon in the tray, with what it shows now, so it is changed only when that changes.
struct Tray {
    icon: TrayIcon,
    tooltip: String,
    choices: Choices,
}

/// The menu's submenus, empty until the manager has opened.
#[derive(Default, PartialEq, Eq)]
struct Choices {
    queues: Vec<QueueItem>,
    speeds: Vec<SpeedItem>,
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
        choices: Choices::default(),
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
    let menu = menu(&Choices::default()).map_err(|e| e.to_string())?;
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

/// The tray's menu, with `choices` in its submenus.
fn menu(choices: &Choices) -> tray_icon::menu::Result<Menu> {
    let item = |a: TrayAction| MenuItem::with_id(a.id(), a.label(), true, None);
    let checks = |items: Vec<(&str, &str, bool)>| -> Vec<CheckMenuItem> {
        items
            .into_iter()
            .map(|(id, label, checked)| CheckMenuItem::with_id(id, label, true, checked, None))
            .collect()
    };
    let submenu = |title: &str, items: &[CheckMenuItem]| {
        let items: Vec<&dyn IsMenuItem> = items.iter().map(|c| c as &dyn IsMenuItem).collect();
        Submenu::with_items(title, !items.is_empty(), &items)
    };
    let queues = checks(
        choices
            .queues
            .iter()
            .map(|q| (q.id.as_str(), q.label.as_str(), q.checked))
            .collect(),
    );
    let speeds = checks(
        choices
            .speeds
            .iter()
            .map(|s| (s.id.as_str(), s.label.as_str(), s.checked))
            .collect(),
    );
    let proxies = checks(
        choices
            .proxies
            .iter()
            .map(|p| (p.id.as_str(), p.label.as_str(), p.checked))
            .collect(),
    );
    Menu::with_items(&[
        &item(TrayAction::Open),
        &item(TrayAction::NewDownload),
        &item(TrayAction::OpenFolder),
        &PredefinedMenuItem::separator(),
        &item(TrayAction::PauseAll),
        &item(TrayAction::ResumeAll),
        &submenu("Queues", &queues)?,
        &PredefinedMenuItem::separator(),
        &submenu("Speed limit", &speeds)?,
        &submenu("Proxy", &proxies)?,
        &PredefinedMenuItem::separator(),
        &item(TrayAction::Quit),
    ])
}

/// Shows the library's speed, or why it could not open, and its queues, speed limit and proxies.
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
    let choices = library
        .settings
        .as_ref()
        .map(|settings| Choices {
            queues: tray_queues::items(&library.queues),
            speeds: tray_speed::items(settings.global_speed_limit),
            proxies: tray_proxy::items(&settings.proxy, &library.proxies),
        })
        .unwrap_or_default();
    let Some(tray) = cx.try_global::<Tray>() else {
        return;
    };
    let (new_tooltip, new_choices) = (tray.tooltip != tooltip, tray.choices != choices);
    if !new_tooltip && !new_choices {
        return;
    }
    let tray = cx.global_mut::<Tray>();
    if new_tooltip && let Err(e) = tray.icon.set_tooltip(Some(&tooltip)) {
        tracing::debug!("could not change the tray tooltip: {e}");
    }
    if new_choices {
        match menu(&choices) {
            Ok(menu) => tray.icon.set_menu(Some(Box::new(menu))),
            Err(e) => tracing::debug!("could not change the tray menu: {e}"),
        }
    }
    tray.tooltip = tooltip;
    tray.choices = choices;
}

struct AppShell<'a>(&'a mut App);

impl Shell for AppShell<'_> {
    fn open_window(&mut self) {
        main_window::bring_up(&Asked::Window, self.0);
    }

    fn quit(&mut self) {
        lifecycle::quit(self.0);
    }

    fn new_download(&mut self) {
        let copied = self
            .0
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        main_window::bring_up(&Asked::Links(links::extract(&copied)), self.0);
    }

    fn open_folder(&mut self, folder: &std::path::Path) {
        // Opening a file "with the system" runs it: only a folder is opened. Checked off the
        // main thread, as a folder on an unreachable share holds the check for seconds.
        let folder = folder.to_path_buf();
        let checked = self
            .0
            .background_executor()
            .spawn(async move { folder.is_dir().then_some(folder) });
        self.0
            .spawn(async move |cx| match checked.await {
                Some(folder) => cx.update(|cx| cx.open_with_system(&folder)),
                None => {
                    tracing::warn!("the default download folder is not a folder on this computer");
                }
            })
            .detach();
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
    if let Some(limit) = tray_speed::limit_of(&id) {
        if let Some(manager) = &manager
            && let Err(e) = tray_speed::switch(manager, limit)
        {
            tracing::warn!("the tray could not change the speed limit: {e}");
        }
        return;
    }
    if let Some(queue) = tray_queues::queue_of(&id) {
        if let Some(manager) = &manager
            && let Err(e) = tray_queues::toggle(manager, queue)
        {
            tracing::warn!("the tray could not pause or resume a queue: {e}");
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
