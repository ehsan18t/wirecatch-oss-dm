//! A tinted box that says what went wrong, with the buttons that fix it.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div,
};

#[derive(IntoElement)]
pub struct Callout {
    title: Option<SharedString>,
    message: SharedString,
    actions: Vec<AnyElement>,
}

impl Callout {
    pub fn danger(message: impl Into<SharedString>) -> Self {
        Self {
            title: None,
            message: message.into(),
            actions: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for Callout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let titled = self.title.is_some();
        v_flex()
            .p_3()
            .gap_1()
            .rounded(theme.radius)
            .bg(theme.danger.opacity(0.12))
            .text_sm()
            .when_some(self.title, |callout, title| {
                callout.child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.danger)
                        .child(title),
                )
            })
            .child(
                div()
                    .when(!titled, |message| message.text_color(theme.danger))
                    .child(self.message),
            )
            .when(!self.actions.is_empty(), |callout| {
                callout.child(h_flex().pt_1().gap_2().children(self.actions))
            })
    }
}
