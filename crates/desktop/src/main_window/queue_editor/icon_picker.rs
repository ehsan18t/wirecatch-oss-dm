//! The queue icon picker: a button beside the queue name that opens a searchable popover over
//! gpui-kit's bundled Lucide catalog.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, App, AppContext as _, Context, Div, ElementId, Entity, EventEmitter, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, point, px,
};

use crate::closing::may_have_closed;
use crate::ui::{Role, Typed as _, icon, queue_tile, tokens};

/// Popover panel width, fitting 8 icon cells plus scrollbar clearance.
const PANEL_WIDTH: f32 = 340.;
const GRID_HEIGHT: f32 = 240.;
const CELL_SIZE: f32 = 36.;
/// Cap on rendered search matches so broad queries stay responsive.
const MAX_SHOWN: usize = 160;

/// Emitted when the person picks or resets the queue's icon.
pub(super) struct IconPicked;

pub(super) struct IconPicker {
    selected: Option<String>,
    open: bool,
    search: Entity<InputState>,
    scroll: ScrollHandle,
    _typing: Subscription,
}

impl EventEmitter<IconPicked> for IconPicker {}

impl IconPicker {
    pub(super) fn new(
        selected: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search icons"));
        let typing =
            cx.subscribe(
                &search,
                |picker: &mut Self, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        picker.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => picker.pick_first_match(cx),
                    _ => {}
                },
            );
        Self {
            selected,
            open: false,
            search,
            scroll: ScrollHandle::new(),
            _typing: typing,
        }
    }

    pub(super) fn selected(&self) -> Option<String> {
        self.selected.clone()
    }

    #[cfg(test)]
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn close(&mut self, cx: &mut Context<'_, Self>) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        cx.notify();
        true
    }

    pub(super) fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.search
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        cx.notify();
    }

    pub(super) fn pick(&mut self, slug: Option<String>, cx: &mut Context<'_, Self>) {
        let current = resolved_selection(self.selected.as_deref());
        let next = icon::queue_icon(slug.as_deref());
        self.open = false;
        // Clicking the already-active icon (including stored "list-ordered") leaves the form clean,
        // while explicitly choosing default over an unrecognized slug clears it.
        if current != Some(next) {
            self.selected = (next != icon::DEFAULT_QUEUE_ICON).then_some(slug).flatten();
            cx.emit(IconPicked);
        }
        cx.notify();
    }

    fn pick_first_match(&mut self, cx: &mut Context<'_, Self>) {
        let query = self.search.read(cx).value();
        if query.trim().is_empty() {
            return;
        }
        let result = icon::search(&query, None, 1);
        if let Some(first) = result.entries.first() {
            self.pick(Some(first.slug.clone()), cx);
        }
    }
}

impl Render for IconPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let chosen = icon::queue_icon(self.selected.as_deref());
        let trigger_label = self
            .selected
            .as_deref()
            .and_then(icon::entry_by_slug)
            .filter(|entry| entry.icon != icon::DEFAULT_QUEUE_ICON)
            .map_or_else(
                || "Choose icon".to_owned(),
                |entry| format!("Choose icon: {}", entry.label),
            );
        let me = cx.entity().downgrade();
        let on_change = me.clone();
        let trigger = Button::new("queue-icon-trigger")
            .outline()
            .large()
            .icon(Icon::new(chosen).with_size(tokens::ICON))
            .accessibility_label(trigger_label.clone())
            .tooltip(trigger_label);

        let search_focus = self.search.read(cx).focus_handle(cx);
        Popover::new("queue-icon-picker")
            .anchor(Anchor::TopLeft)
            .open(self.open)
            .track_focus(&search_focus)
            .on_open_change(move |open, window, cx| {
                let open = *open;
                may_have_closed(on_change.update(cx, |picker, cx| {
                    picker.set_open(open, window, cx);
                }));
            })
            .trigger(trigger)
            .content(move |_, _, cx| popover_body(&me, cx))
    }
}

/// The bundled icon `selected` names, or `None` when a stored slug is not in this build's catalog.
fn resolved_selection(selected: Option<&str>) -> Option<gpui_kit::assets::IconName> {
    match selected {
        None => Some(icon::DEFAULT_QUEUE_ICON),
        Some(slug) => icon::by_slug(slug),
    }
}

fn popover_body(picker: &WeakEntity<IconPicker>, cx: &App) -> Div {
    let Some(entity) = picker.upgrade() else {
        return v_flex();
    };
    let state = entity.read(cx);
    let query = state.search.read(cx).value();
    let trimmed = query.trim();
    let result = icon::search(trimmed, state.selected.as_deref(), MAX_SHOWN);
    let active = resolved_selection(state.selected.as_deref());
    let has_custom = active != Some(icon::DEFAULT_QUEUE_ICON);
    let summary = if !result.searched {
        format!("Popular icons · {} in catalog", icon::catalog_count())
    } else if result.total == 0 {
        format!("No icons match \u{201c}{trimmed}\u{201d}")
    } else if result.total > result.entries.len() {
        format!(
            "Showing {} of {} matches",
            result.entries.len(),
            result.total
        )
    } else if result.total == 1 {
        "1 match".to_owned()
    } else {
        format!("{} matches", result.total)
    };
    let reset_target = picker.clone();
    v_flex()
        .w(px(PANEL_WIDTH))
        .gap_2p5()
        .child(
            Input::new(&state.search)
                .small()
                .cleanable(true)
                .prefix(Icon::new(IconName::Search).with_size(tokens::ICON_SMALL)),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_role(Role::Meta, cx)
                        .child(summary),
                )
                .when(has_custom, |row| {
                    row.child(
                        Button::new("queue-icon-reset")
                            .xsmall()
                            .ghost()
                            .label("Use default")
                            .on_click(move |_, _, cx| {
                                may_have_closed(reset_target.update(cx, |picker, cx| {
                                    picker.pick(None, cx);
                                }));
                            }),
                    )
                }),
        )
        .child(icon_grid(result.entries, active, &state.scroll, picker, cx))
}

fn icon_grid(
    entries: Vec<&'static icon::IconEntry>,
    active: Option<gpui_kit::assets::IconName>,
    scroll: &ScrollHandle,
    picker: &WeakEntity<IconPicker>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let primary = theme.primary;
    let tint = queue_tile::tinted(theme.popover, primary);
    let muted = theme.muted_foreground;
    div()
        .relative()
        .h(px(GRID_HEIGHT))
        .child(
            div()
                .id("queue-icon-grid")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .pr_2()
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .children(entries.into_iter().map(|entry| {
                            let selected = Some(entry.icon) == active;
                            let target = picker.clone();
                            let slug = entry.slug.clone();
                            let label = entry.label.clone();
                            let id =
                                ElementId::Name(format!("queue-icon-choice-{}", entry.slug).into());
                            Button::new(id)
                                .ghost()
                                .selected(selected)
                                .toggled(selected)
                                .w(px(CELL_SIZE))
                                .h(px(CELL_SIZE))
                                .p_0()
                                .icon(Icon::new(entry.icon).with_size(tokens::ICON))
                                .accessibility_label(label.clone())
                                .tooltip(label)
                                .when(selected, |button| {
                                    button
                                        .bg(tint)
                                        .border_1()
                                        .border_color(primary)
                                        .text_color(primary)
                                })
                                .when(!selected, |button| button.text_color(muted))
                                .on_click(move |_, _, cx| {
                                    may_have_closed(target.update(cx, |picker, cx| {
                                        picker.pick(Some(slug.clone()), cx);
                                    }));
                                })
                        })),
                ),
        )
        .vertical_scrollbar(scroll)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui_kit::test]
    fn picking_and_resetting_an_icon_updates_selection_and_closes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (picker, cx) = cx.add_window_view(|window, cx| IconPicker::new(None, window, cx));
        assert_eq!(picker.read_with(cx, |p, _| p.selected()), None);

        picker.update_in(cx, |p, window, cx| p.set_open(true, window, cx));
        assert!(picker.read_with(cx, |p, _| p.is_open()));

        picker.update(cx, |p, cx| p.pick(Some("film".to_owned()), cx));
        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("film")
        );
        assert!(!picker.read_with(cx, |p, _| p.is_open()));

        picker.update(cx, |p, cx| p.pick(None, cx));
        assert_eq!(picker.read_with(cx, |p, _| p.selected()), None);
    }

    #[gpui_kit::test]
    fn typing_and_pressing_enter_picks_the_top_matching_icon(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (picker, cx) = cx.add_window_view(|window, cx| IconPicker::new(None, window, cx));
        picker.update_in(cx, |p, window, cx| p.set_open(true, window, cx));
        cx.run_until_parked();

        cx.simulate_keystrokes("f i l m");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();

        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("film")
        );
        assert!(!picker.read_with(cx, |p, _| p.is_open()));
    }

    #[gpui_kit::test]
    fn clicking_already_selected_default_slug_leaves_selection_unchanged(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (picker, cx) = cx.add_window_view(|window, cx| {
            IconPicker::new(Some("list-ordered".to_owned()), window, cx)
        });
        picker.update(cx, |p, cx| p.pick(None, cx));
        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("list-ordered")
        );
    }

    #[gpui_kit::test]
    fn pressing_enter_with_empty_or_whitespace_query_keeps_featured_icon(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (picker, cx) =
            cx.add_window_view(|window, cx| IconPicker::new(Some("film".to_owned()), window, cx));
        picker.update_in(cx, |p, window, cx| p.set_open(true, window, cx));
        cx.run_until_parked();

        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("film")
        );
        assert!(picker.read_with(cx, |p, _| p.is_open()));

        cx.simulate_keystrokes("space space enter");
        cx.run_until_parked();
        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("film")
        );
        assert!(picker.read_with(cx, |p, _| p.is_open()));
    }

    #[gpui_kit::test]
    fn choosing_default_clears_an_unrecognized_slug(cx: &mut TestAppContext) {
        assert_eq!(resolved_selection(Some("future-icon")), None);
        assert_eq!(resolved_selection(None), Some(icon::DEFAULT_QUEUE_ICON));
        assert_eq!(
            resolved_selection(Some("list-ordered")),
            Some(icon::DEFAULT_QUEUE_ICON)
        );

        cx.update(gpui_kit::component::init);
        let (picker, cx) = cx.add_window_view(|window, cx| {
            IconPicker::new(Some("future-icon".to_owned()), window, cx)
        });
        assert_eq!(
            picker.read_with(cx, |p, _| p.selected()).as_deref(),
            Some("future-icon")
        );

        picker.update(cx, |p, cx| p.pick(Some("list-ordered".to_owned()), cx));
        assert_eq!(picker.read_with(cx, |p, _| p.selected()), None);
    }
}
