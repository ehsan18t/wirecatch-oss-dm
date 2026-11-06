//! The settings screen (D85): gpui-kit's settings pages, each setting read from the library and
//! changed through the manager, so every window shows the change once the manager makes it.

mod about;
mod addon;
mod browser;
mod files;
mod general;
mod media;
mod network;
mod notifications;
mod speed;

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::setting::{SelectIndex, SettingField, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Subscription, Task, WeakEntity, Window, div, px,
};
use wirecatch_protocol::app::{AppSettings, AppSettingsPatch};
use wirecatch_shell::api;

use super::{MainWindow, Screen, sidebar};
use crate::backend::act;
use crate::library::Library;
use crate::text;
use crate::ui::{Role, Typed as _, tokens};

impl MainWindow {
    /// The settings pages with their own navigation, under the way back to the downloads and over
    /// the foot the main navigation has. gpui-kit's settings own that column, so both lie over it.
    pub(super) fn settings(&self, cx: &mut Context<'_, Self>) -> Div {
        let pages = screen(
            &self.library,
            &cx.entity().downgrade(),
            self.settings_on,
            cx,
        );
        let foot = self.foot(cx);
        let theme = cx.theme();
        let back = h_flex()
            .id("back-to-downloads")
            .h(px(30.))
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|item| item.bg(theme.list_hover))
            .child(Icon::new(Lucide::ArrowLeft).with_size(tokens::ICON))
            .child("Back to downloads")
            .on_click(
                cx.listener(|this, _, window, cx| this.open_screen(Screen::Downloads, window, cx)),
            );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(pages)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(sidebar::WIDTH))
                    .p_2()
                    .child(back),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w(px(sidebar::WIDTH))
                    .bg(theme.sidebar)
                    .child(foot),
            )
    }
}

/// The room the way back takes over gpui-kit's search field.
const BACK_HEIGHT: f32 = 38.;

/// One page drawn like a settings page, in the main navigation's place: gpui-kit's settings with
/// no room for their own column.
fn lone_page(id: &'static str, page: SettingPage, cx: &App) -> impl IntoElement + use<> {
    div().flex_1().min_h_0().child(
        Settings::new(id)
            .small()
            .sidebar_width(px(0.))
            .sidebar_size_range(px(0.)..px(0.))
            .with_group_variant(GroupBoxVariant::Outline)
            .page(page.header_style(&page_header(cx))),
    )
}

/// The About screen.
pub(super) fn about_screen(library: &Entity<Library>, cx: &App) -> impl IntoElement + use<> {
    lone_page("about", about::page(library), cx)
}

/// The Keyboard shortcuts screen.
pub(super) fn shortcuts_screen(cx: &App) -> impl IntoElement + use<> {
    lone_page("shortcuts", super::keymap::page(), cx)
}

fn page_header(cx: &App) -> StyleRefinement {
    StyleRefinement::default()
        .px_5()
        .py_3()
        .text_role(Role::PageTitle, cx)
}

/// How long a list field waits after the last keystroke before saving.
const SAVE_AFTER: Duration = Duration::from_millis(800);
/// The widest a custom field's status and buttons run before wrapping. The setting's own
/// column takes up to 60% of the row and never yields, so a wider field runs past the card.
const FIELD_WIDTH: f32 = 320.;

/// A settings page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    General,
    Files,
    Notifications,
    Browser,
    Media,
    Speed,
    Network,
}

impl Page {
    /// The page's name, on its heading and wherever it is offered.
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Files => "Saving files",
            Self::Notifications => "Notifications",
            Self::Browser => "Browser",
            Self::Media => "Media",
            Self::Speed => "Speed and limits",
            Self::Network => "Network",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Self::General => Lucide::SlidersHorizontal,
            Self::Files => Lucide::Folder,
            Self::Notifications => Lucide::Bell,
            Self::Browser => Lucide::Globe,
            Self::Media => Lucide::Film,
            Self::Speed => Lucide::Gauge,
            Self::Network => Lucide::Network,
        }
    }
}

/// Every page, in the order D85 gives.
pub(super) const PAGES: [Page; 7] = [
    Page::General,
    Page::Files,
    Page::Notifications,
    Page::Browser,
    Page::Media,
    Page::Speed,
    Page::Network,
];

/// The page the settings screen opens on. A new `opening` starts the screen afresh on `page`,
/// since the pages keep the one picked last.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct OpenOn {
    pub opening: usize,
    pub page: Option<Page>,
}

fn page(
    page: Page,
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    cx: &App,
) -> SettingPage {
    match page {
        Page::General => general::page(library),
        Page::Files => files::page(library),
        Page::Notifications => notifications::page(library, cx),
        Page::Browser => browser::page(library, cx),
        Page::Media => media::page(library, cx),
        Page::Speed => speed::page(library, main, cx),
        Page::Network => network::page(library, cx),
    }
    .icon(Icon::new(page.icon()))
}

/// Every page, headed like the downloads' page.
fn screen(
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    open_on: OpenOn,
    cx: &App,
) -> Settings {
    let header = page_header(cx);
    let first = open_on
        .page
        .and_then(|page| PAGES.iter().position(|known| *known == page))
        .unwrap_or_default();
    Settings::new(("settings", open_on.opening))
        .default_selected_index(SelectIndex {
            page_ix: first,
            group_ix: None,
        })
        .small()
        .sidebar_width(px(sidebar::WIDTH))
        .sidebar_size_range(px(sidebar::WIDTH)..px(sidebar::WIDTH))
        .header_style(&StyleRefinement::default().mt(px(BACK_HEIGHT)))
        .with_group_variant(GroupBoxVariant::Outline)
        .pages(PAGES.map(|shown| page(shown, library, main, cx).header_style(&header)))
}

/// Sends `patch` to the manager; a refusal shows in the window in front.
fn change(patch: AppSettingsPatch, cx: &mut App) {
    if let Some(window) = crate::backend::window_in_front(cx) {
        act(window, cx, move |manager| {
            api::change_settings(manager, patch)
        });
    } else {
        tracing::warn!("a setting changed with no window open");
    }
}

/// A custom field's status and buttons, right-aligned and wrapping within [`FIELD_WIDTH`].
fn field_row() -> Div {
    h_flex()
        .max_w(px(FIELD_WIDTH))
        .flex_wrap()
        .justify_end()
        .items_center()
        .gap_2()
}

/// A setting's current value; `fallback` until the settings have loaded.
fn current<T>(
    library: &Entity<Library>,
    cx: &App,
    read: impl Fn(&AppSettings) -> T,
    fallback: T,
) -> T {
    library.read(cx).settings.as_ref().map_or(fallback, read)
}

/// An on and off setting.
fn switch(
    library: &Entity<Library>,
    read: fn(&AppSettings) -> bool,
    patch: fn(bool) -> AppSettingsPatch,
) -> SettingField<bool> {
    let library = library.clone();
    SettingField::switch(
        move |cx| current(&library, cx, read, false),
        move |on, cx| change(patch(on), cx),
    )
}

/// A setting with a few named values; each value is offered under its name in `choices`.
fn choice<T: Copy + PartialEq + 'static>(
    library: &Entity<Library>,
    choices: &'static [(T, &'static str)],
    read: fn(&AppSettings) -> T,
    patch: fn(T) -> AppSettingsPatch,
) -> SettingField<SharedString> {
    let library = library.clone();
    let options = choices
        .iter()
        .map(|&(_, name)| (SharedString::from(name), SharedString::from(name)))
        .collect();
    SettingField::dropdown(
        options,
        move |cx| {
            let value = current(&library, cx, |settings| Some(read(settings)), None);
            choices
                .iter()
                .find(|(choice, _)| Some(*choice) == value)
                .map_or_else(SharedString::default, |&(_, name)| name.into())
        },
        move |name, cx| {
            if let Some(&(value, _)) = choices.iter().find(|(_, choice)| *choice == name.as_ref()) {
                change(patch(value), cx);
            }
        },
    )
}

/// How a list setting is typed.
#[derive(Clone, Copy)]
enum Typing {
    /// One value per line.
    Lines,
    /// File extensions on one line, separated by commas; a leading dot is dropped.
    Extensions,
}

impl Typing {
    fn shown(self, values: &[String]) -> String {
        match self {
            Self::Lines => values.join("\n"),
            Self::Extensions => values.join(", "),
        }
    }

    fn read(self, typed: &str) -> Vec<String> {
        match self {
            Self::Lines => text::filled_lines(typed),
            Self::Extensions => text::typed_extensions(typed),
        }
    }
}

/// A list setting typed as text, saved a moment after typing stops or when the field loses
/// focus, so a value half typed is not sent on every keystroke.
fn list_field(
    key: SharedString,
    saved: &[String],
    typing: Typing,
    placeholder: &'static str,
    save: impl Fn(Vec<String>, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Textarea {
    struct List {
        input: Entity<TextareaState>,
        pending: Option<Task<()>>,
        _saving: Subscription,
    }
    let shown = typing.shown(saved);
    let save = Rc::new(save);
    let list = window.use_keyed_state(key, cx, |window, cx| {
        let (min, max) = match typing {
            Typing::Lines => (2, 8),
            Typing::Extensions => (1, 3),
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(min, max)
                .placeholder(placeholder)
                .default_value(shown.clone())
        });
        // Its first layout wraps the text without counting the rows, so a long first value shows cut off until edited.
        let fitted = input.clone();
        window.on_next_frame(move |_, cx| {
            fitted.update(cx, |state, cx| state.set_auto_grow(min, max, cx));
        });
        let saving = cx.subscribe(
            &input,
            move |list: &mut List, input, event: &InputEvent, cx| {
                let save = save.clone();
                match event {
                    InputEvent::Change => {
                        let input = input.clone();
                        // Replacing the task drops the one waiting, so only the last change saves.
                        list.pending = Some(cx.spawn(async move |_, cx| {
                            cx.background_executor().timer(SAVE_AFTER).await;
                            cx.update(|cx| save(typing.read(&input.read(cx).value()), cx));
                        }));
                    }
                    InputEvent::Blur => {
                        list.pending = None;
                        save(typing.read(&input.read(cx).value()), cx);
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Focus => {}
                }
            },
        );
        List {
            input,
            pending: None,
            _saving: saving,
        }
    });
    let input = list.read(cx).input.clone();
    // A change made elsewhere shows, unless the person is typing here.
    if !input.read(cx).focus_handle(cx).is_focused(window)
        && typing.read(&input.read(cx).value()) != saved
    {
        input.update(cx, |state, cx| state.set_value(shown, window, cx));
    }
    Textarea::new(&input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lists_keep_what_holds_something() {
        assert_eq!(Typing::Lines.read(" a.example \n\n b "), ["a.example", "b"]);
        assert_eq!(
            Typing::Extensions.read(".ISO, tar.gz;  zip"),
            ["iso", "tar.gz", "zip"]
        );
        assert_eq!(
            Typing::Extensions.shown(&["iso".into(), "zip".into()]),
            "iso, zip"
        );
    }
}
