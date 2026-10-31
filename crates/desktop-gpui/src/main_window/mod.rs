//! The main window: the title bar, the navigation of views and queues, the download list and the
//! status bar (D36, D91).

mod applies;
mod backup_dialog;
mod command_bar;
mod confirm;
mod details;
mod drag;
mod empty;
mod fixes;
mod keymap;
mod link_trial;
mod list;
mod notices_bar;
mod order;
mod palette;
mod proxy_dialog;
mod queue_dialog;
mod queue_header;
mod queue_proxy;
mod queue_rules;
mod rail;
mod refresh_dialog;
mod row_menu;
mod selection;
mod settings;
mod sidebar;
mod sort;
mod title_bar;

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{ActiveTheme as _, TitleBar, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, ExternalPaths, FocusHandle,
    Focusable as _, Global, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
    actions, div, px, size,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary};
use wirecatch_protocol::desktop::Reveal;

use self::selection::Step;
use crate::library::{Asked, Library, SettingsChanged};
use crate::ui::{Role, Typed as _, layout};
use crate::{add_dialog, lifecycle, links, theme};

actions!(
    main_window,
    [
        NewDownload,
        PasteLinks,
        Dismiss,
        FocusSearch,
        ToggleSettings,
        CursorUp,
        CursorDown,
        ExtendUp,
        ExtendDown,
        CursorPageUp,
        CursorPageDown,
        CursorFirst,
        CursorLast,
        SelectAll,
        PauseOrResume,
        OpenSelected,
        ShowDetails,
        RemoveSelected,
        SearchToList,
        OpenMenu,
        NudgeUp,
        NudgeDown,
        OpenPalette,
        ShowShortcuts,
        NextRegion,
        PreviousRegion
    ]
);

/// The key context the main window's shortcuts work in; a text field's own keys come first.
const KEY_CONTEXT: &str = "MainWindow";

/// The search field's context, where Down leaves the field for the list.
const SEARCH_CONTEXT: &str = "MainSearch";

const ALL_PLACEHOLDER: &str = "Search downloads or paste links";

/// Said when Ctrl+V finds no link to add.
const NO_LINK_PASTED: &str =
    "There is no link in what you pasted. Copy the full address, starting with https://.";

/// The main window's shortcuts. Call once at start.
pub fn bind_keys(cx: &mut App) {
    keymap::bind(cx);
}

/// Where a download's row sits in the list, in the order the groups show.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Group {
    Attention,
    Downloading,
    UpNext,
    Paused,
    Finished,
    Cancelled,
}

impl Group {
    pub const ALL: [Self; 6] = [
        Self::Attention,
        Self::Downloading,
        Self::UpNext,
        Self::Paused,
        Self::Finished,
        Self::Cancelled,
    ];

    pub fn of(status: DownloadStatus) -> Self {
        match status {
            DownloadStatus::Failed => Self::Attention,
            DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing => Self::Downloading,
            DownloadStatus::Queued => Self::UpNext,
            DownloadStatus::Paused => Self::Paused,
            DownloadStatus::Completed => Self::Finished,
            DownloadStatus::Cancelled => Self::Cancelled,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Attention => "Needs attention",
            Self::Downloading => "Downloading",
            Self::UpNext => "Up next",
            Self::Paused => "Paused",
            Self::Finished => "Finished",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Work that is over: it has left its queue's line.
    fn is_over(self) -> bool {
        matches!(self, Self::Finished | Self::Cancelled)
    }

    /// Finished and cancelled downloads start folded: they are done with.
    fn starts_folded(self) -> bool {
        matches!(self, Self::Finished | Self::Cancelled)
    }
}

/// What fills the window beside the navigation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Downloads,
    Settings,
}

/// What the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    All,
    Group(Group),
    Queue(QueueId),
}

impl View {
    fn shows(self, row: &DownloadSummary) -> bool {
        match self {
            Self::All => true,
            Self::Group(group) => Group::of(row.status) == group,
            Self::Queue(queue) => row.queue == queue,
        }
    }
}

/// The main window, once opened; it may have been closed since.
#[derive(Clone)]
struct Shown {
    window: AnyWindowHandle,
    view: WeakEntity<MainWindow>,
}

/// What lives as long as the app, with or without a window: the library, which no window holds
/// at a start in the background, and the main window once opened.
struct Running {
    library: Entity<Library>,
    shown: Option<Shown>,
}

impl Global for Running {}

/// Keeps `library` for the life of the app and brings the main window up whenever it is asked.
pub fn run(library: Entity<Library>, cx: &mut App) {
    cx.subscribe(&library, |_, asked: &Asked, cx| bring_up(asked, cx))
        .detach();
    cx.set_global(Running {
        library,
        shown: None,
    });
}

/// Brings the main window up for `asked`, opening it when it is not open: at a start in the
/// background, or after it was closed.
pub fn bring_up(asked: &Asked, cx: &mut App) {
    if lifecycle::start_again_later(asked, cx) {
        return;
    }
    let Some(running) = cx.try_global::<Running>() else {
        return;
    };
    let (library, shown) = (running.library.clone(), running.shown.clone());
    if shown.is_some_and(|shown| answer_in(&shown, asked, cx)) {
        return;
    }
    match MainWindow::open(library, cx) {
        Ok(shown) => {
            cx.global_mut::<Running>().shown = Some(shown.clone());
            answer_in(&shown, asked, cx);
        }
        Err(e) => {
            tracing::error!("could not open the main window: {e}");
            crate::say_before_exit(&format!(
                "{} could not open its window ({e}) and has closed. Start it again, and if this keeps happening, restart your computer.",
                wirecatch_shell::host::CHANNEL.name()
            ));
            lifecycle::quit(cx);
        }
    }
}

/// Whether the main window is open, whether or not it is in front.
pub fn is_open(cx: &mut App) -> bool {
    let Some(shown) = cx.try_global::<Running>().and_then(|r| r.shown.clone()) else {
        return false;
    };
    shown.window.update(cx, |_, _, _| ()).is_ok()
}

/// The library the main window shows, once the app runs.
fn library(cx: &App) -> Option<Entity<Library>> {
    cx.try_global::<Running>()
        .map(|running| running.library.clone())
}

/// Brings the main window up and runs `work` in it.
pub fn bring_up_with(cx: &mut App, work: impl FnOnce(&mut Window, &mut App)) {
    bring_up(&Asked::Window, cx);
    if let Some(shown) = cx
        .try_global::<Running>()
        .and_then(|running| running.shown.clone())
    {
        _ = shown.window.update(cx, |_, window, cx| work(window, cx));
    }
}

/// Whether the window in `shown` was still open to answer `asked`.
fn answer_in(shown: &Shown, asked: &Asked, cx: &mut App) -> bool {
    shown
        .window
        .update(cx, |_, window, cx| {
            shown
                .view
                .update(cx, |main, cx| main.answer(asked, window, cx))
                .is_ok()
        })
        .unwrap_or(false)
}

pub struct MainWindow {
    library: Entity<Library>,
    screen: Screen,
    view: View,
    selected: HashSet<DownloadId>,
    /// Where a Shift range starts.
    anchor: Option<DownloadId>,
    /// The row the arrow keys move from.
    cursor: Option<DownloadId>,
    /// The selection was built on purpose (Ctrl, Shift, Ctrl+A), so it shows its bar.
    picked: bool,
    sort: sort::Sort,
    frozen: std::cell::RefCell<sort::Frozen>,
    nav: rail::Nav,
    /// The full navigation is open over the list, from the rail of a narrow window.
    drawer: bool,
    /// The Add dialog's form, while it is open.
    add_form: Option<WeakEntity<add_dialog::AddForm>>,
    /// The row menu the keyboard opened, while it shows.
    keyboard_menu: Option<(Entity<PopupMenu>, Subscription)>,
    /// Where the cursor's row was last drawn, for the keyboard's menu.
    cursor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    folded: HashSet<Group>,
    /// The title bar's search, which narrows the list to names and links holding it.
    search: Entity<InputState>,
    /// The queue under the pointer, which shows its menu.
    hovered_queue: Option<QueueId>,
    /// Every queue shows in the navigation, not only the first few.
    all_queues: bool,
    /// The row under the pointer, which shows its buttons.
    hovered: Option<DownloadId>,
    scroll: UniformListScrollHandle,
    settings_on: settings::OpenOn,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    /// Opens the main window on `library`.
    ///
    /// # Errors
    ///
    /// The system would not open a window.
    fn open(library: Entity<Library>, cx: &mut App) -> gpui_kit::Result<Shown> {
        let bounds = gpui_kit::Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(600.))),
            ..TitleBar::window_options()
        };
        let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Self::new(library, window, cx))
        })?;
        Ok(Shown {
            window,
            view: view.downgrade(),
        })
    }

    /// Answers `asked`: brings this window to the front, with the Add dialog holding the links,
    /// on the browser settings, or on the downloads a notification is about.
    fn answer(&mut self, asked: &Asked, window: &mut Window, cx: &mut Context<'_, Self>) {
        window.activate_window();
        match asked {
            Asked::Window => {}
            Asked::Links(links) => self.new_download(links, window, cx),
            Asked::BrowserSettings => self.open_settings(settings::Page::Browser, cx),
            Asked::Reveal(reveal) => self.reveal(reveal, window, cx),
        }
    }

    fn open_settings(&mut self, page: settings::Page, cx: &mut Context<'_, Self>) {
        self.settings_on = settings::OpenOn {
            opening: self.settings_on.opening + 1,
            page: Some(page),
        };
        self.screen = Screen::Settings;
        cx.notify();
    }

    /// Shows `reveal`'s downloads selected in their queue, the first in view, or in every download
    /// when the queue is gone (D98); or the settings page it names.
    fn reveal(&mut self, reveal: &Reveal, window: &mut Window, cx: &mut Context<'_, Self>) {
        if reveal.browser_settings {
            return self.open_settings(settings::Page::Browser, cx);
        }
        if reveal.proxy_settings {
            return self.open_settings(settings::Page::Network, cx);
        }
        self.screen = Screen::Downloads;
        let library = self.library.read(cx);
        let view = reveal
            .queue
            .filter(|queue| library.queue(*queue).is_some())
            .map_or(View::All, View::Queue);
        let groups: Vec<Group> = library
            .downloads
            .iter()
            .filter(|row| reveal.ids.contains(&row.id))
            .map(|row| Group::of(row.status))
            .collect();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.show(view, window, cx);
        for group in groups {
            self.folded.remove(&group);
        }
        self.selected = reveal.ids.iter().copied().collect();
        self.anchor = reveal.ids.first().copied();
        self.cursor = self.anchor;
        self.picked = reveal.ids.len() > 1;
        if let Some(line) = reveal
            .ids
            .first()
            .and_then(|first| self.line_of(*first, cx))
        {
            self.scroll.scroll_to_item(line, ScrollStrategy::Center);
        }
    }

    fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let restyle = cx.subscribe(&library, |_, library, _: &SettingsChanged, cx| {
            if let Some(settings) = &library.read(cx).settings {
                theme::apply(settings.theme, settings.accent, cx);
            }
        });
        let follow_system = cx.observe_window_appearance(window, |this, _, cx| {
            if let Some(settings) = &this.library.read(cx).settings {
                theme::apply(settings.theme, settings.accent, cx);
            }
        });
        let redraw = cx.observe_in(&library, window, |this, library, window, cx| {
            if !library.read(cx).startup_notices.is_empty() {
                this.tell_startup_notices(window, cx);
            }
            cx.notify();
        });
        if !library.read(cx).startup_notices.is_empty() {
            cx.defer_in(window, |this, window, cx| {
                this.tell_startup_notices(window, cx);
            });
        }
        let closing = library.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            let close_to_tray = closing
                .read_with(cx, |library, _| {
                    library.settings.as_ref().map(|s| s.close_to_tray)
                })
                .ok()
                .flatten();
            lifecycle::window_closing(close_to_tray, cx);
            true
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ALL_PLACEHOLDER)
                .clean_on_escape()
        });
        let searched = cx.subscribe_in(
            &search,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.download_typed(window, cx),
                _ => {}
            },
        );
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            library,
            screen: Screen::Downloads,
            view: View::All,
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            picked: false,
            keyboard_menu: None,
            add_form: None,
            nav: rail::Nav::default(),
            drawer: false,
            sort: sort::Sort::default(),
            frozen: std::cell::RefCell::default(),
            cursor_bounds: Rc::default(),
            folded: Group::ALL
                .into_iter()
                .filter(|g| g.starts_folded())
                .collect(),
            hovered: None,
            scroll: UniformListScrollHandle::new(),
            settings_on: settings::OpenOn::default(),
            hovered_queue: None,
            all_queues: false,
            focus,
            search,
            _subscriptions: vec![restyle, follow_system, redraw, searched],
        }
    }

    fn show(&mut self, view: View, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.view = view;
        self.drawer = false;
        self.clear_selection();
        let placeholder = match view {
            View::All => ALL_PLACEHOLDER.to_owned(),
            View::Group(_) | View::Queue(_) => {
                format!("Search {} or paste links", self.title(cx))
            }
        };
        self.search.update(cx, |search, cx| {
            search.set_placeholder(placeholder, window, cx);
        });
        cx.notify();
    }

    pub(super) fn toggle_settings(&mut self, cx: &mut Context<'_, Self>) {
        self.screen = match self.screen {
            Screen::Downloads => Screen::Settings,
            Screen::Settings => Screen::Downloads,
        };
        cx.notify();
    }

    /// The full navigation over the list, beside the rail; it closes on a choice or a click
    /// beside it.
    fn drawer(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .absolute()
            .top(gpui_kit::component::TITLE_BAR_HEIGHT)
            .bottom_0()
            .left(px(rail::WIDTH))
            .right_0()
            .child(
                div()
                    .id("drawer-scrim")
                    .absolute()
                    .size_full()
                    .bg(theme.overlay)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .h_full()
                    .shadow_lg()
                    .child(self.sidebar(cx)),
            )
    }

    /// F6 and Shift+F6: the keyboard moves between the search and the list.
    fn next_region(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.screen = Screen::Downloads;
        if self.search.read(cx).focus_handle(cx).is_focused(window) {
            self.focus.focus(window, cx);
            if self.cursor.is_none() {
                self.step(Step::First, false, cx);
            }
        } else {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        }
        cx.notify();
    }

    /// Escape outside a field clears the selection; a dialog closes on its own Escape first.
    fn dismiss(&mut self, cx: &mut Context<'_, Self>) {
        self.clear_selection();
        cx.notify();
    }

    /// Opens the Add dialog, on the queue shown when a queue is; links while it is open join it.
    fn new_download(&mut self, urls: &[String], window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(open) = self.add_form.as_ref().and_then(WeakEntity::upgrade) {
            if !urls.is_empty() {
                open.update(cx, |form, cx| form.add_links(urls, window, cx));
            }
            return;
        }
        let queue = match self.view {
            View::Queue(queue) => Some(queue),
            View::All | View::Group(_) => None,
        };
        self.add_form = Some(add_dialog::open(
            self.library.clone(),
            urls,
            queue,
            window,
            cx,
        ));
    }

    /// Opens the queue editor on `queue`, or on a new queue.
    fn edit_queue(
        &mut self,
        queue: Option<QueueId>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        queue_dialog::open(
            self.library.clone(),
            queue,
            cx.entity().downgrade(),
            window,
            cx,
        );
    }

    /// Ctrl+V outside a text field: the links in what was copied go to the Add dialog.
    fn paste_links(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        let urls = links::extract(&copied);
        if urls.is_empty() {
            window.push_notification(NO_LINK_PASTED, cx);
        } else {
            self.new_download(&urls, window, cx);
        }
    }

    /// The list's keys, while the downloads show.
    fn list_keys(
        window: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<'_, Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        window
            .on_action(cx.listener(|this, _: &NudgeUp, window, cx| this.nudge(true, window, cx)))
            .on_action(cx.listener(|this, _: &NudgeDown, window, cx| this.nudge(false, window, cx)))
            .on_action(cx.listener(|this, _: &OpenMenu, window, cx| {
                this.open_keyboard_menu(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SearchToList, window, cx| {
                this.focus.focus(window, cx);
                this.step(Step::Down, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorUp, _, cx| this.step(Step::Up, false, cx)))
            .on_action(cx.listener(|this, _: &CursorDown, _, cx| this.step(Step::Down, false, cx)))
            .on_action(cx.listener(|this, _: &ExtendUp, _, cx| this.step(Step::Up, true, cx)))
            .on_action(cx.listener(|this, _: &ExtendDown, _, cx| this.step(Step::Down, true, cx)))
            .on_action(cx.listener(|this, _: &CursorPageUp, _, cx| {
                this.step(Step::PageUp, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorPageDown, _, cx| {
                this.step(Step::PageDown, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorFirst, _, cx| {
                this.step(Step::First, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorLast, _, cx| this.step(Step::Last, false, cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &PauseOrResume, window, cx| {
                this.pause_or_resume(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                this.open_selected(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowDetails, window, cx| {
                this.details_of_cursor(window, cx);
            }))
            .on_action(cx.listener(|this, _: &RemoveSelected, window, cx| {
                this.remove_selected(window, cx);
            }))
    }

    fn title(&self, cx: &App) -> SharedString {
        match self.view {
            View::All => "All downloads".into(),
            View::Group(group) => group.label().into(),
            View::Queue(id) => self.library.read(cx).queue(id).map_or_else(
                || "Queue".into(),
                |queue| queue.settings.name.clone().into(),
            ),
        }
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let rail = self.shows_rail(window.viewport_size().width);
        let background = cx.theme().background;
        v_flex()
            .id("main-window")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewDownload, window, cx| {
                this.new_download(&[], window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteLinks, window, cx| this.paste_links(window, cx)))
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| this.dismiss(cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.screen = Screen::Downloads;
                this.search
                    .update(cx, |search, cx| search.focus(window, cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleSettings, _, cx| this.toggle_settings(cx)))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_paths(paths, window, cx);
            }))
            .on_action(cx.listener(|_, _: &ShowShortcuts, window, cx| keymap::open(window, cx)))
            .on_action(cx.listener(|this, _: &NextRegion, window, cx| this.next_region(window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousRegion, window, cx| this.next_region(window, cx)),
            )
            .when(self.screen == Screen::Downloads, |window| {
                Self::list_keys(window, cx)
            })
            .size_full()
            .bg(background)
            .text_role(Role::Body, cx)
            .child(self.title_bar(cx))
            .child(match self.screen {
                Screen::Downloads => layout::body(
                    if rail {
                        self.rail(cx).into_any_element()
                    } else {
                        self.sidebar(cx).into_any_element()
                    },
                    self.command_bar(cx).into_any_element(),
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .children(self.storage_banner(cx))
                        .children(self.queue_header(cx))
                        .children(self.selection_bar(cx))
                        .child(self.list(cx)),
                ),
                Screen::Settings => self.settings(cx),
            })
            .when(
                rail && self.drawer && self.screen == Screen::Downloads,
                |window| window.child(self.drawer(cx)),
            )
            .when_some(self.keyboard_menu.as_ref(), |window, (menu, _)| {
                window.child(row_menu::keyboard_menu(menu, self.keyboard_menu_at()))
            })
    }
}
