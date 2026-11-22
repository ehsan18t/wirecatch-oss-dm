//! A settings row that shows or hides the rows under it in the same card: its title, a line
//! naming what it holds, and a chevron (D287).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, px,
};

use super::tokens;

type Toggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The card's padding, which the row's hover fills and its rule crosses.
const CARD_PADDING: f32 = 16.;
/// How far the hover reaches into the card's padding around the text.
const HOVER_REACH: f32 = 8.;

#[derive(IntoElement)]
pub struct Disclosure {
    id: ElementId,
    title: SharedString,
    holds: SharedString,
    open: bool,
    on_toggle: Option<Toggle>,
}

impl Disclosure {
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        holds: impl Into<SharedString>,
        open: bool,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            holds: holds.into(),
            open,
            on_toggle: None,
        }
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Disclosure {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let row = Button::new(self.id)
            .ghost()
            .small()
            .w_full()
            .h_auto()
            .px(px(HOVER_REACH))
            .py(px(HOVER_REACH))
            .toggled(self.open)
            .accessibility_label(self.title.clone())
            // The button centres what it holds; one row that fills it puts the chevron at the end.
            .child(
                h_flex()
                    .flex_1()
                    .justify_between()
                    .child(
                        v_flex()
                            .items_start()
                            .text_sm()
                            .child(self.title)
                            .child(div().text_color(theme.muted_foreground).child(self.holds)),
                    )
                    .child(
                        Icon::new(if self.open {
                            Lucide::ChevronDown
                        } else {
                            Lucide::ChevronRight
                        })
                        .with_size(tokens::ICON)
                        .text_color(theme.muted_foreground),
                    ),
            )
            .when_some(self.on_toggle, |row, toggle| row.on_click(toggle));
        v_flex()
            .my(px(-HOVER_REACH))
            .child(div().mx(px(-HOVER_REACH)).child(row))
            .when(self.open, |this| {
                this.child(
                    div()
                        .my(px(HOVER_REACH))
                        .mx(px(-CARD_PADDING))
                        .h_px()
                        .bg(theme.border),
                )
            })
    }
}
