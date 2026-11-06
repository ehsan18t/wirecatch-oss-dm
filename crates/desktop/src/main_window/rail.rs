//! The navigation folded to an icon rail, and the switch between the docked sidebar, the rail,
//! and the drawer that a narrow window opens over the list (D87.2, D91).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::{Group, MainWindow, Screen, View, queue_header, sidebar};
use crate::ui::{QueueTile, tokens, tooltip};

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
    /// Folded by the title bar's button, at any width.
    Rail,
}

impl Nav {
    /// Whether the navigation shows as a rail in a window `width` wide.
    fn shows_rail(self, width: Pixels) -> bool {
        match self {
            Self::Auto => width < px(NARROW),
            Self::Rail => true,
        }
    }

    /// What the title bar's button makes of the navigation in a window `width` wide, or `None`
    /// where it opens and closes the drawer instead.
    fn toggled(self, width: Pixels) -> Option<Self> {
        let rail = self.shows_rail(width);
        if rail && width < px(NARROW) {
            return None;
        }
        Some(if rail { Self::Auto } else { Self::Rail })
    }
}

impl MainWindow {
    pub(super) fn shows_rail(&self, width: Pixels) -> bool {
        self.nav.shows_rail(width)
    }

    /// The title bar's navigation button: folds the sidebar to a rail and back; in a narrow
    /// window it opens the full navigation as a drawer over the list.
    pub(super) fn toggle_nav(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        match self.nav.toggled(window.viewport_size().width) {
            Some(nav) => {
                self.nav = nav;
                self.page.drawer = false;
            }
            None => self.page.drawer = !self.page.drawer,
        }
        cx.notify();
    }

    /// The rail: an icon for each view, each queue's initials, and the screen switch.
    pub(super) fn rail(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let (view_items, queue_items) = self.rail_items(cx);
        let foot = [Screen::Settings, Screen::Shortcuts, Screen::About]
            .map(|screen| self.rail_foot_item(screen, cx));
        let theme = cx.theme();
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
                v_flex()
                    .py_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .w_full()
                    .items_center()
                    .children(foot),
            )
    }

    /// Settings, Keyboard shortcuts or About at the rail's foot.
    fn rail_foot_item(&self, screen: Screen, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme();
        let (id, label, icon) = sidebar::foot_page(screen);
        let current = self.page.screen == screen;
        let dot = screen == Screen::About && sidebar::update_waiting(cx);
        rail_button(id, label, cx)
            .relative()
            .when(current, |item| {
                item.bg(theme.sidebar_accent).text_color(theme.primary)
            })
            .child(Icon::new(icon).with_size(tokens::ICON))
            .when(dot, |item| {
                item.child(
                    div()
                        .absolute()
                        .top(px(7.))
                        .right(px(7.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(theme.primary),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.open_screen(screen, window, cx)))
            .into_any_element()
    }

    /// The rail's buttons: one per view, then one per queue.
    fn rail_items(&self, cx: &mut Context<'_, Self>) -> (Vec<AnyElement>, Vec<AnyElement>) {
        let library = self.library.read(cx);
        let tally = library.downloads.tally();
        let mut views = vec![(
            "rail-all".to_owned(),
            Icon::new(Lucide::Layers),
            format!("All downloads, {}", tally.count),
            View::All,
        )];
        views.extend(Group::ALL.map(|group| {
            (
                format!("rail-{group:?}"),
                sidebar::group_icon(group),
                format!("{}, {}", group.label(), tally.in_group(group)),
                View::Group(group),
            )
        }));
        let view_items: Vec<AnyElement> = views
            .into_iter()
            .map(|(id, icon, label, view)| {
                let icon = icon.with_size(tokens::ICON).into_any_element();
                self.rail_item(id, icon, label, view, cx)
            })
            .collect();
        let queue_items = self.rail_queue_items(cx);
        (view_items, queue_items)
    }

    /// The rail's queue buttons: initials, tinted while the queue downloads, with the badge of its rules.
    fn rail_queue_items(&self, cx: &mut Context<'_, Self>) -> Vec<AnyElement> {
        let library = self.library.read(cx);
        let queues: Vec<RailQueue> = library
            .queues
            .iter()
            .map(|queue| {
                let rules = queue_header::rules_on(queue);
                let rules_said = match rules {
                    Some(true) => ", adds downloads by its rules",
                    Some(false) => ", rules off",
                    None => "",
                };
                RailQueue {
                    id: format!("rail-queue-{}", queue.id.0),
                    letters: initials(&queue.settings.name),
                    label: format!(
                        "{}: {}{rules_said}",
                        queue.settings.name,
                        self.describe(queue, cx)
                    ),
                    view: View::Queue(queue.id),
                    rules,
                    running: library
                        .downloads
                        .tally_of(queue.id)
                        .in_group(Group::Downloading)
                        > 0,
                }
            })
            .collect();
        queues
            .into_iter()
            .map(|queue| {
                let theme = cx.theme();
                let current = matches!(self.page.screen, Screen::Downloads | Screen::QueuePage)
                    && self.view == queue.view;
                let behind = if current {
                    theme.sidebar_accent
                } else {
                    theme.sidebar
                };
                let hover =
                    (!current).then(|| (SharedString::from(queue.id.clone()), theme.list_hover));
                let tile = QueueTile::letters(queue.letters, behind)
                    .rules(queue.rules)
                    .when(queue.running, |tile| tile.tone(theme.primary))
                    .when_some(hover, |tile, (group, color)| {
                        tile.hover_behind(group, color)
                    })
                    .into_any_element();
                self.rail_item(queue.id, tile, queue.label, queue.view, cx)
            })
            .collect()
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
        let current = self.page.screen == Screen::Downloads && self.view == view;
        rail_button(id.clone(), label, cx)
            .group(SharedString::from(id))
            .when(current, |item| {
                item.bg(theme.sidebar_accent).text_color(theme.primary)
            })
            .child(content)
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
            .into_any_element()
    }
}

/// A queue's rail button: its initials, tinted while it downloads, and the badge of its rules.
struct RailQueue {
    id: String,
    letters: String,
    label: String,
    view: View,
    rules: Option<bool>,
    running: bool,
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
        .tooltip(tooltip::plain(label))
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
    fn unfolding_leaves_the_fold_to_the_width_again() {
        let (wide, narrow) = (px(NARROW + 300.), px(NARROW - 200.));
        let folded = Nav::Auto
            .toggled(wide)
            .expect("a wide window folds the sidebar");
        assert!(folded.shows_rail(wide));
        let unfolded = folded.toggled(wide).expect("and unfolds it");
        assert!(!unfolded.shows_rail(wide));
        assert!(
            unfolded.shows_rail(narrow),
            "a narrow window folds it again"
        );
        assert_eq!(
            unfolded.toggled(narrow),
            None,
            "where the button opens the drawer"
        );
    }

    #[test]
    fn a_queue_shows_its_initials() {
        assert_eq!(initials("Main"), "MA");
        assert_eq!(initials("Night videos"), "NV");
        assert_eq!(initials("  "), "");
    }
}
