//! A round checkbox that picks a row, or every row under a heading.

use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

const SIZE: f32 = 16.;

/// How much of what a circle stands for is picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Picked {
    None,
    Some,
    All,
}

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct SelectCircle {
    id: ElementId,
    picked: Picked,
    on_click: Option<OnClick>,
}

impl SelectCircle {
    pub fn new(id: impl Into<ElementId>, picked: Picked) -> Self {
        Self {
            id: id.into(),
            picked,
            on_click: None,
        }
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for SelectCircle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mark = match self.picked {
            Picked::None => None,
            Picked::Some => Some(IconName::Minus),
            Picked::All => Some(IconName::Check),
        };
        div()
            .id(self.id)
            .size(px(SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border(px(1.5))
            .cursor_pointer()
            .map(|circle| match self.picked {
                Picked::None => circle
                    .border_color(theme.muted_foreground.opacity(0.7))
                    .hover(|circle| circle.border_color(theme.foreground)),
                Picked::Some => circle.border_color(theme.primary).text_color(theme.primary),
                Picked::All => circle
                    .border_color(theme.primary)
                    .bg(theme.primary)
                    .text_color(theme.primary_foreground),
            })
            .children(mark.map(|mark| Icon::new(mark).with_size(px(SIZE - 6.))))
            .when_some(self.on_click, |circle, on_click| {
                circle.on_click(move |event, window, cx| {
                    // The row under it would take the click too.
                    cx.stop_propagation();
                    on_click(event, window, cx);
                })
            })
    }
}
