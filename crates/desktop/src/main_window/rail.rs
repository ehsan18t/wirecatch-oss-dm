//! The navigation folded to an icon rail, and the switch between the docked sidebar, the rail,
//! and the drawer that a narrow window opens over the list (D87.2, D91).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::{Group, MainWindow, Screen, View, sidebar};
use crate::ui::{Role, Typed as _, tokens};

/// The rail's width: an icon and its padding.
pub(super) const WIDTH: f32 = 52.;
/// Below this window width the navigation starts as a rail, and opens as a drawer.
const NARROW: f32 = 1100.;

/// How the navigation shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Nav {
    /// Docked when the window is wide, a rail when it is narrow.
    #[default]
    Auto,
    Docked,
    Rail,
}

impl MainWindow {
    /// Whether the navigation shows as a rail in a window `width` wide.
    pub(super) fn shows_rail(&self, width: Pixels) -> bool {
        match self.nav {
            Nav::Auto => width < px(NARROW),
            Nav::Docked => false,
            Nav::Rail => true,
        }
    }

    /// The title bar's navigation button: folds the sidebar to a rail and back; in a narrow
    /// window it opens the full navigation as a drawer over the list.
    pub(super) fn toggle_nav(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let narrow = window.viewport_size().width < px(NARROW);
        if narrow && self.shows_rail(window.viewport_size().width) {
            self.drawer = !self.drawer;
        } else {
            self.nav = if self.shows_rail(window.viewport_size().width) {
                Nav::Docked
            } else {
                Nav::Rail
            };
            self.drawer = false;
        }
        cx.notify();
    }

    /// The rail: an icon for each view, each queue's initials, and the screen switch.
    pub(super) fn rail(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let (view_items, queue_items) = self.rail_items(cx);
        let theme = cx.theme();
        let (switch_icon, switch_label) = match self.screen {
            Screen::Downloads => (Icon::new(IconName::Settings), "Settings"),
            Screen::Settings => (Icon::new(Lucide::Download), "Downloads"),
        };
        v_flex()
            .id("rail")
            .w(px(WIDTH))
            .h_full()
            .flex_shrink_0()
            .items_center()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                v_flex()
                    .id("rail-views")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .gap_1()
                    .items_center()
                    .children(view_items)
                    .child(div().h(px(12.)))
                    .children(queue_items),
            )
            .child(
                div()
                    .py_2()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        rail_button("rail-switch", switch_label, cx)
                            .child(switch_icon.with_size(tokens::ICON))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_settings(cx))),
                    ),
            )
    }

    /// The rail's buttons: one per view, then one per queue.
    fn rail_items(&self, cx: &mut Context<'_, Self>) -> (Vec<AnyElement>, Vec<AnyElement>) {
        let library = self.library.read(cx);
        let count = |view: View| library.downloads.iter().filter(|r| view.shows(r)).count();
        let mut views = vec![(
            "rail-all".to_owned(),
            Icon::new(Lucide::Layers),
            format!("All downloads, {}", count(View::All)),
            View::All,
        )];
        views.extend(Group::ALL.map(|group| {
            (
                format!("rail-{group:?}"),
                sidebar::group_icon(group),
                format!("{}, {}", group.label(), count(View::Group(group))),
                View::Group(group),
            )
        }));
        let queues: Vec<(String, String, String, View)> = library
            .queues
            .iter()
            .map(|queue| {
                (
                    format!("rail-queue-{}", queue.id.0),
                    initials(&queue.settings.name),
                    format!("{}: {}", queue.settings.name, self.describe(queue, cx)),
                    View::Queue(queue.id),
                )
            })
            .collect();
        let view_items: Vec<AnyElement> = views
            .into_iter()
            .map(|(id, icon, label, view)| {
                let icon = icon.with_size(tokens::ICON).into_any_element();
                self.rail_item(id, icon, label, view, cx)
            })
            .collect();
        let queue_items: Vec<AnyElement> = queues
            .into_iter()
            .map(|(id, letters, label, view)| {
                let tile = div()
                    .text_role(Role::Meta, cx)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(letters)
                    .into_any_element();
                self.rail_item(id, tile, label, view, cx)
            })
            .collect();
        (view_items, queue_items)
    }

    fn rail_item(
        &self,
        id: String,
        content: AnyElement,
        label: String,
        view: View,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let current = self.screen == Screen::Downloads && self.view == view;
        rail_button(id, label, cx)
            .when(current, |item| {
                item.bg(theme.sidebar_accent).text_color(theme.primary)
            })
            .child(content)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.screen = Screen::Downloads;
                this.drawer = false;
                this.show(view, window, cx);
            }))
            .into_any_element()
    }
}

/// A square rail button with its name in a tooltip beside it.
fn rail_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    cx: &Context<'_, MainWindow>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let theme = cx.theme();
    let label: SharedString = label.into();
    div()
        .id(ElementId::Name(id.into()))
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.radius)
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|item| item.bg(theme.list_hover).text_color(theme.foreground))
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
}

/// A queue's initials for the rail: the first letters of its first two words, or its first two
/// letters.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters: String = match words.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        [only] => only.chars().take(2).collect(),
        [] => String::new(),
    };
    letters.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_shows_its_initials() {
        assert_eq!(initials("Main"), "MA");
        assert_eq!(initials("Night videos"), "NV");
        assert_eq!(initials("  "), "");
    }
}
