//! A short label on a quiet ground, such as the last name of a download's folder, with the whole
//! of what it stands for on hover.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px, relative,
};

use super::{Role, Typed as _, tooltip};

const HEIGHT: f32 = 20.;
const MAX_WIDTH_FRACTION: f32 = 0.5;

#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    whole: Option<SharedString>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            whole: None,
        }
    }

    /// What the label stands for, shown on hover.
    pub fn whole(mut self, whole: impl Into<SharedString>) -> Self {
        self.whole = Some(whole.into());
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id(self.id)
            .debug_selector(|| "chip".to_owned())
            .flex_shrink_0()
            .max_w(relative(MAX_WIDTH_FRACTION))
            .h(px(HEIGHT))
            .px_1p5()
            .flex()
            .items_center()
            .rounded(theme.radius)
            .bg(theme.secondary)
            .text_role(Role::Meta, cx)
            .text_color(theme.secondary_foreground)
            .child(div().min_w_0().truncate().child(self.label))
            .when_some(self.whole, |chip, whole| {
                chip.tooltip(tooltip::plain(whole))
            })
    }
}
