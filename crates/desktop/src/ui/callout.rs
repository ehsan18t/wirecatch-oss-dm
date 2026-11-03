//! A tinted box that says what went wrong, with the buttons that fix it, or what is about to
//! happen.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;

use super::{Role, Typed as _};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Danger,
    Info,
}

#[derive(IntoElement)]
pub struct Callout {
    tone: Tone,
    title: Option<SharedString>,
    message: SharedString,
    actions: Vec<AnyElement>,
    actions_beside: bool,
}

impl Callout {
    pub fn danger(message: impl Into<SharedString>) -> Self {
        Self::of(Tone::Danger, message)
    }

    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::of(Tone::Info, message)
    }

    fn of(tone: Tone, message: impl Into<SharedString>) -> Self {
        Self {
            tone,
            title: None,
            message: message.into(),
            actions: Vec::new(),
            actions_beside: false,
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

    /// Puts the buttons on the right of the words instead of under them.
    pub fn actions_beside(mut self) -> Self {
        self.actions_beside = true;
        self
    }
}

impl RenderOnce for Callout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let titled = self.title.is_some();
        let has_actions = !self.actions.is_empty();
        let tint = match self.tone {
            Tone::Danger => theme.danger,
            Tone::Info => theme.info,
        };
        let said = v_flex()
            .gap_1()
            .when_some(self.title, |said, title| {
                said.child(
                    div()
                        .text_role(Role::Strong, cx)
                        .text_color(tint)
                        .child(title),
                )
            })
            .child(
                div()
                    .when(!titled, |message| message.text_color(tint))
                    .child(self.message),
            );
        let box_ = div()
            .p_3()
            .rounded(theme.radius)
            .bg(tint.opacity(0.12))
            .text_role(Role::Body, cx);
        let buttons = h_flex().gap_2().children(self.actions);
        if self.actions_beside {
            box_.flex()
                .items_center()
                .gap_3()
                .child(said.flex_1().min_w_0())
                .child(buttons)
        } else {
            box_.flex()
                .flex_col()
                .gap_1()
                .child(said)
                .when(has_actions, |callout| callout.child(buttons.pt_1()))
        }
    }
}
