//! The queue icon picker: a clickable icon button beside the queue name that opens a searchable
//! popover over gpui-kit's bundled Lucide catalogue.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, App, AppContext as _, Context, Div, ElementId, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};

use crate::closing::may_have_closed;
use crate::ui::{Role, Typed as _, icon, queue_tile, tokens, tooltip};

/// Popover panel width, fitting 8 icon cells plus scrollbar padding.
const PANEL_WIDTH: f32 = 340.;
/// Scrollable icon grid height.
const GRID_HEIGHT: f32 = 240.;
/// Square cell size for each icon choice.
const CELL_SIZE: f32 = 36.;
/// Cap on rendered search matches so broad queries stay responsive.
const MAX_SHOWN: usize = 160;

/// State for picking a queue's optional bundled Lucide icon.
pub(super) struct IconPicker {
    selected: Option<String>,
    open: bool,
    search: Entity<InputState>,
    _typing: Subscription,
}

impl IconPicker {
    pub(super) fn new(
        selected: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search icons..."));
        let typing = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            selected,
            open: false,
            search,
            _typing: typing,
        }
    }

    pub(super) fn selected(&self) -> Option<String> {
        self.selected.clone()
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn close(&mut self, cx: &mut Context<'_, Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.search
                .update(cx, |state, cx| state.set_value("", window, cx));
            let search = self.search.clone();
            window.defer(cx, move |window, cx| {
                search.update(cx, |state, cx| state.focus(window, cx));
            });
        }
        cx.notify();
    }

    fn pick(&mut self, slug: Option<String>, cx: &mut Context<'_, Self>) {
        self.selected = slug;
        self.open = false;
        cx.notify();
    }
}

impl Render for IconPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let chosen = icon::queue_icon(self.selected.as_deref());
        let me = cx.entity().downgrade();
        let on_change = me.clone();
        let trigger = Button::new("queue-icon-trigger")
            .outline()
            .large()
            .icon(Icon::new(chosen).with_size(tokens::ICON))
            .tooltip("Choose icon");

        Popover::new("queue-icon-picker")
            .anchor(Anchor::TopLeft)
            .open(self.open)
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

fn popover_body(picker: &WeakEntity<IconPicker>, cx: &App) -> Div {
    let Some(entity) = picker.upgrade() else {
        return v_flex();
    };
    let state = entity.read(cx);
    let query = state.search.read(cx).value().to_string();
    let trimmed = query.trim();
    let (matches, total) = icon::search(trimmed, MAX_SHOWN);
    let has_custom = state.selected.is_some();
    let active = icon::queue_icon(state.selected.as_deref());
    let summary = if trimmed.is_empty() {
        format!("Popular icons · {} in catalogue", icon::catalog_count())
    } else if total == 0 {
        format!("No icons match \u{201c}{trimmed}\u{201d}")
    } else if total > matches.len() {
        format!("Showing {} of {total} matches", matches.len())
    } else if total == 1 {
        "1 match".to_owned()
    } else {
        format!("{total} matches")
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
        .child(icon_grid(matches, active, picker, cx))
}

fn icon_grid(
    entries: Vec<&'static icon::IconEntry>,
    active: gpui_kit::assets::IconName,
    picker: &WeakEntity<IconPicker>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let primary = theme.primary;
    let tint = queue_tile::tinted(theme.popover, primary);
    let hover_bg = theme.list_hover;
    let muted = theme.muted_foreground;
    let radius = theme.radius;
    div()
        .id("queue-icon-grid")
        .h(px(GRID_HEIGHT))
        .overflow_y_scroll()
        .child(
            h_flex()
                .flex_wrap()
                .gap_1()
                .children(entries.into_iter().map(|entry| {
                    let selected = entry.icon == active;
                    let target = picker.clone();
                    let chosen_slug =
                        (entry.icon != icon::DEFAULT_QUEUE_ICON).then(|| entry.slug.clone());
                    let label = entry.label.clone();
                    let id = ElementId::Name(format!("queue-icon-choice-{}", entry.slug).into());
                    div()
                        .id(id)
                        .size(px(CELL_SIZE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius)
                        .cursor_pointer()
                        .when(selected, |cell| {
                            cell.bg(tint)
                                .border_1()
                                .border_color(primary)
                                .text_color(primary)
                        })
                        .when(!selected, |cell| {
                            cell.text_color(muted).hover(move |cell| cell.bg(hover_bg))
                        })
                        .tooltip(tooltip::plain(label))
                        .on_click(move |_, _, cx| {
                            may_have_closed(target.update(cx, |picker, cx| {
                                picker.pick(chosen_slug.clone(), cx);
                            }));
                        })
                        .child(Icon::new(entry.icon).with_size(tokens::ICON))
                })),
        )
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
}
