//! The main window: the title bar, the navigation of views and queues, the download list and the
//! status bar (D36, D91).

mod list;
mod sidebar;
mod status_bar;

use std::collections::HashSet;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
    WindowBounds, WindowOptions, actions, div, px, size,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary};

use crate::library::{Library, SettingsChanged};
use crate::{add_dialog, links, theme};

actions!(main_window, [NewDownload, PasteLinks]);

/// The key context the main window's shortcuts work in; a text field's own keys come first.
const KEY_CONTEXT: &str = "MainWindow";

/// Said when Ctrl+V finds no link to add.
const NO_LINK_PASTED: &str =
    "There is no link in what you pasted. Copy the full address, starting with https://.";

/// The main window's shortcuts. Call once at start.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewDownload, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-v", PasteLinks, Some(KEY_CONTEXT)),
    ]);
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

    /// Finished and cancelled downloads start folded: they are done with.
    fn starts_folded(self) -> bool {
        matches!(self, Self::Finished | Self::Cancelled)
    }
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

pub struct MainWindow {
    library: Entity<Library>,
    view: View,
    selected: HashSet<DownloadId>,
    folded: HashSet<Group>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    /// Opens the main window on `library`.
    ///
    /// # Errors
    ///
    /// The system would not open a window.
    pub fn open(library: Entity<Library>, cx: &mut App) -> gpui_kit::Result<()> {
        let bounds = gpui_kit::Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(600.))),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Self::new(library, window, cx))
        })?;
        Ok(())
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
        let redraw = cx.observe(&library, |_, _, cx| cx.notify());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            library,
            view: View::All,
            selected: HashSet::new(),
            folded: Group::ALL
                .into_iter()
                .filter(|g| g.starts_folded())
                .collect(),
            focus,
            _subscriptions: vec![restyle, follow_system, redraw],
        }
    }

    fn show(&mut self, view: View, cx: &mut Context<'_, Self>) {
        self.view = view;
        self.selected.clear();
        cx.notify();
    }

    /// Opens the Add dialog, on the queue shown when a queue is.
    fn new_download(&mut self, urls: &[String], window: &mut Window, cx: &mut Context<'_, Self>) {
        let queue = match self.view {
            View::Queue(queue) => Some(queue),
            View::All | View::Group(_) => None,
        };
        add_dialog::open(self.library.clone(), urls, queue, window, cx);
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
        let theme = cx.theme();
        v_flex()
            .id("main-window")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewDownload, window, cx| {
                this.new_download(&[], window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteLinks, window, cx| this.paste_links(window, cx)))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(title_bar(cx))
            .child(
                h_flex().flex_1().min_h_0().child(self.sidebar(cx)).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.header(cx))
                        .child(self.list(window, cx)),
                ),
            )
            .child(self.status_bar(cx))
    }
}

/// The title bar: the app's mark and name, with the window's buttons.
fn title_bar(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    TitleBar::new().child(
        h_flex()
            .gap_2()
            .child(div().size(px(8.)).rounded_full().bg(theme.primary))
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(wirecatch_shell::host::CHANNEL.name()),
            ),
    )
}
