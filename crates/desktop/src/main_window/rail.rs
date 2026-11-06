//! The navigation folded to an icon rail, and the switch between the docked sidebar, the rail,
//! and the drawer that a narrow window opens over the list (D91, D266, D267).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use wirecatch_protocol::app::Queue;

use super::{Group, MainWindow, Screen, View, queue_header, sidebar};
use crate::ui::{QueueTile, Role, Typed as _, tokens, tooltip};

/// A count badge's height with its cut-out ring; it widens for more digits.
const BADGE: f32 = 18.;
/// How far a count badge reaches past its item's corner.
const BADGE_OUT: f32 = 2.;
/// The most a badge counts; beyond it, it says `99+`.
const BADGE_MOST: usize = 99;

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

    /// The rail: an icon for each view, each queue's state, and the screen switch.
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
            None,
        )];
        views.extend(Group::ALL.map(|group| {
            let count = tally.in_group(group);
            (
                format!("rail-{group:?}"),
                sidebar::group_icon(group),
                format!("{}, {count}", group.label()),
                View::Group(group),
                counted(group, count),
            )
        }));
        let view_items: Vec<AnyElement> = views
            .into_iter()
            .map(|(id, icon, label, view, badge)| {
                let icon = icon.with_size(tokens::ICON).into_any_element();
                let badge = badge.map(|badge| self.count_badge(badge, &id, view, cx));
                self.rail_item(id, icon, label, view, cx)
                    .children(badge)
                    .into_any_element()
            })
            .collect();
        let queue_items = self.rail_queue_items(cx);
        (view_items, queue_items)
    }

    /// The rail's queue buttons: the tile of what each queue is doing, as the sidebar draws it,
    /// with the badge of its rules.
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
                    queue: queue.clone(),
                    label: format!(
                        "{}: {}{rules_said}",
                        queue.settings.name,
                        self.describe(queue, cx)
                    ),
                    view: View::Queue(queue.id),
                    rules,
                    running: sidebar::running_done(library.downloads.tally_of(queue.id)),
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
                let tile = sidebar::queue_tile(&queue.queue, queue.running.is_some(), behind, cx)
                    .rules(queue.rules)
                    .when_some(queue.running, QueueTile::ring)
                    .when_some(hover, |tile, (group, color)| {
                        tile.hover_behind(group, color)
                    })
                    .into_any_element();
                self.rail_item(queue.id, tile, queue.label, queue.view, cx)
                    .into_any_element()
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
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let theme = cx.theme();
        let current = self.current(view);
        rail_button(id.clone(), label, cx)
            .relative()
            .group(SharedString::from(id))
            .when(current, |item| {
                item.bg(theme.sidebar_accent).text_color(theme.primary)
            })
            .child(content)
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
    }

    fn current(&self, view: View) -> bool {
        self.page.screen == Screen::Downloads && self.view == view
    }

    /// A count at the rail item's top right corner, cut out of the colour behind it as the
    /// queue tile's rules badge is, so it keeps its edge on a hovered or open item.
    fn count_badge(&self, badge: Badge, group: &str, view: View, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let (bg, fg) = if badge.alarming {
            (theme.danger, theme.danger_foreground)
        } else {
            (theme.muted_foreground, theme.sidebar)
        };
        let current = self.current(view);
        let behind = if current {
            theme.sidebar_accent
        } else {
            theme.sidebar
        };
        let hover = theme.list_hover;
        div()
            .absolute()
            .top(px(-BADGE_OUT))
            .right(px(-BADGE_OUT))
            .min_w(px(BADGE))
            .h(px(BADGE))
            .px(px(3.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(bg)
            .border_2()
            .border_color(behind)
            .when(!current, |badge| {
                badge.group_hover(SharedString::from(group.to_owned()), move |style| {
                    style.border_color(hover)
                })
            })
            .text_role(Role::Caption, cx)
            .text_color(fg)
            .font_weight(FontWeight::BOLD)
            .child(badge_text(badge.count))
            .into_any_element()
    }
}

/// A queue's rail button: the tile of what it is doing, the badge of its rules, and while it
/// downloads a ring of how far it got.
struct RailQueue {
    id: String,
    queue: Queue,
    label: String,
    view: View,
    rules: Option<bool>,
    /// How much is done, while it downloads.
    running: Option<f32>,
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

/// A view's count on its rail icon: what needs attention, in the danger colour, and what
/// downloads. The other views say their count in their tooltip only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Badge {
    count: usize,
    alarming: bool,
}

fn counted(group: Group, count: usize) -> Option<Badge> {
    let alarming = match group {
        Group::Attention => true,
        Group::Downloading => false,
        _ => return None,
    };
    (count > 0).then_some(Badge { count, alarming })
}

fn badge_text(count: usize) -> String {
    if count > BADGE_MOST {
        format!("{BADGE_MOST}+")
    } else {
        count.to_string()
    }
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
    fn only_attention_and_downloading_show_a_count() {
        assert_eq!(
            counted(Group::Attention, 2),
            Some(Badge {
                count: 2,
                alarming: true
            })
        );
        assert_eq!(
            counted(Group::Downloading, 3),
            Some(Badge {
                count: 3,
                alarming: false
            })
        );
        assert_eq!(counted(Group::Attention, 0), None);
        assert_eq!(counted(Group::UpNext, 4), None);
        assert_eq!(counted(Group::Finished, 40), None);
    }

    #[test]
    fn a_badge_counts_to_99() {
        assert_eq!(badge_text(7), "7");
        assert_eq!(badge_text(99), "99");
        assert_eq!(badge_text(100), "99+");
    }
}
