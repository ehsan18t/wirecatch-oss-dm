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

use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::setting::{SelectIndex, SettingField, SettingPage, Settings};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, StyleRefinement, Styled as _, Subscription, Task, WeakEntity,
    Window, div, px,
};
use wirecatch_protocol::app::{AppSettings, AppSettingsPatch};
use wirecatch_shell::api;

use super::{MainWindow, sidebar};
use crate::backend::act;
use crate::library::Library;
use crate::ui::{Role, Typed as _};

impl MainWindow {
    /// The settings pages with their own navigation, which ends in the way back to the downloads
    /// where the main navigation has Settings.
    pub(super) fn settings(&self, cx: &mut Context<'_, Self>) -> Div {
        let pages = screen(
            &self.library,
            &cx.entity().downgrade(),
            self.settings_on,
            cx,
        );
        let switch = self.screen_switch(cx);
        div().relative().flex_1().min_h_0().child(pages).child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .w(px(sidebar::WIDTH))
                .bg(cx.theme().sidebar)
                .child(switch),
        )
    }
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
    About,
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
            Self::Speed => "Speed and queues",
            Self::Network => "Network",
            Self::About => "About",
        }
    }
}

/// Every page, in the order D85 gives.
pub(super) const PAGES: [Page; 8] = [
    Page::General,
    Page::Files,
    Page::Notifications,
    Page::Browser,
    Page::Media,
    Page::Speed,
    Page::Network,
    Page::About,
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
        Page::About => about::page(library),
    }
}

/// Every page, headed like the downloads' page.
fn screen(
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    open_on: OpenOn,
    cx: &App,
) -> Settings {
    let header = StyleRefinement::default()
        .px_5()
        .py_3()
        .text_role(Role::PageTitle, cx);
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
        .with_group_variant(GroupBoxVariant::Outline)
        .pages(PAGES.map(|shown| page(shown, library, main, cx).header_style(&header)))
}

/// Sends `patch` to the manager; a refusal shows in the window in front.
fn change(patch: AppSettingsPatch, cx: &mut App) {
    if let Some(window) = cx.active_window() {
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
            Self::Lines => typed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            Self::Extensions => typed
                .split([',', ';', ' ', '\n'])
                .map(|typed| typed.trim().trim_start_matches('.').to_lowercase())
                .filter(|extension| !extension.is_empty())
                .collect(),
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
) -> AnyElement {
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
    Textarea::new(&input).into_any_element()
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
