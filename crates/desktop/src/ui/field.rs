//! A form control with its label above and its note below.

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;

use super::{Role, Typed as _};
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div,
};

#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    control: AnyElement,
    note: Option<SharedString>,
    wrong: bool,
}

impl Field {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            control: control.into_any_element(),
            note: None,
            wrong: false,
        }
    }

    pub fn note(mut self, note: impl Into<SharedString>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Shows the note in the danger colour, as what is wrong with the value.
    pub fn wrong(mut self, wrong: bool) -> Self {
        self.wrong = wrong;
        self
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let note_color = if self.wrong {
            theme.danger
        } else {
            theme.muted_foreground
        };
        v_flex()
            .gap_1p5()
            .child(div().text_role(Role::Strong, cx).child(self.label))
            .child(self.control)
            .when_some(self.note, |field, note| {
                field.child(
                    div()
                        .text_role(Role::Meta, cx)
                        .text_color(note_color)
                        .child(note),
                )
            })
    }
}
