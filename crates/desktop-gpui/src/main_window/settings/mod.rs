//! The settings screen (D85): gpui-kit's settings pages, each setting read from the library and
//! changed through the manager, so every window shows the change once the manager makes it.

mod general;
mod network;
mod notifications;

use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{SettingField, Settings};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::{
    App, Context, Div, Entity, ParentElement as _, SharedString, StyleRefinement, Styled as _, div,
    px,
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
        let pages = screen(&self.library, cx);
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

/// Every page, in the order D85 gives, headed like the downloads' page.
fn screen(library: &Entity<Library>, cx: &App) -> Settings {
    let header = StyleRefinement::default()
        .px_5()
        .py_3()
        .text_role(Role::PageTitle, cx);
    Settings::new("settings")
        .small()
        .sidebar_width(px(sidebar::WIDTH))
        .sidebar_size_range(px(sidebar::WIDTH)..px(sidebar::WIDTH))
        .with_group_variant(GroupBoxVariant::Outline)
        .pages(
            [
                general::page(library),
                notifications::page(library, cx),
                network::page(library, cx),
            ]
            .map(|page| page.header_style(&header)),
        )
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
