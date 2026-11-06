//! A list typed one value at a time: each value a chip with its own remove button, and a field
//! after them for the next.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::Confirm;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Window, div, px,
};

use super::{Role, Typed as _};

/// The narrowest the field after the chips gets before it wraps to a line of its own.
const FIELD_MIN_WIDTH: f32 = 140.;
const CHIP_HEIGHT: f32 = 22.;

/// How typed text becomes values.
#[derive(Clone, Copy)]
pub enum Split {
    /// A comma, semicolon or space ends a value, as Enter does; `read` gives the values of a text.
    Words(fn(&str) -> Vec<String>),
    /// Only Enter ends a value, which may hold commas and spaces.
    Lines,
}

impl Split {
    fn values(self, typed: &str) -> Vec<String> {
        match self {
            Self::Words(read) => read(typed),
            Self::Lines => {
                let value = typed.trim();
                if value.is_empty() {
                    Vec::new()
                } else {
                    vec![value.to_owned()]
                }
            }
        }
    }

    fn ends_value(self, typed: &str) -> bool {
        matches!(self, Self::Words(_)) && typed.contains([',', ';', ' ', '\n'])
    }
}

/// The values changed: one was added or removed.
pub struct Changed;

pub struct TagField {
    values: Vec<String>,
    split: Split,
    input: Entity<InputState>,
    _typing: Subscription,
}

impl EventEmitter<Changed> for TagField {}

impl TagField {
    pub fn new(
        values: &[String],
        split: Split,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let typing = cx.subscribe_in(
            &input,
            window,
            |field: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if field.split.ends_value(&field.input.read(cx).value()) {
                        field.commit(window, cx);
                    }
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => field.commit(window, cx),
                InputEvent::Focus => {}
            },
        );
        Self {
            values: values.to_vec(),
            split,
            input,
            _typing: typing,
        }
    }

    /// The values, with what is typed and not yet a chip among them.
    pub fn values(&self, cx: &App) -> Vec<String> {
        let mut values = self.values.clone();
        for value in self.split.values(&self.input.read(cx).value()) {
            if !values.contains(&value) {
                values.push(value);
            }
        }
        values
    }

    /// Gives the keyboard to the field after the chips of `field`.
    pub fn focus(field: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let input = field.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let typed = self.split.values(&self.input.read(cx).value());
        if typed.is_empty() && self.input.read(cx).value().is_empty() {
            return;
        }
        for value in typed {
            if !self.values.contains(&value) {
                self.values.push(value);
            }
        }
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.emit(Changed);
        cx.notify();
    }

    fn remove(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if index < self.values.len() {
            self.values.remove(index);
            cx.emit(Changed);
            cx.notify();
        }
    }
}

impl Render for TagField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let chips = self.values.iter().enumerate().map(|(index, value)| {
            h_flex()
                .flex_shrink_0()
                .h(px(CHIP_HEIGHT))
                .pl_2()
                .pr_0p5()
                .gap_0p5()
                .rounded(theme.radius)
                .bg(theme.secondary_active)
                .text_role(Role::Meta, cx)
                .text_color(theme.secondary_foreground)
                .child(value.clone())
                .child(
                    Button::new(("remove-value", index))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip(format!("Remove {value}"))
                        .on_click(cx.listener(move |field, _, _, cx| field.remove(index, cx))),
                )
        });
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .p_1()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.input)
            .bg(theme.background)
            .children(chips)
            .child(
                div()
                    .flex_1()
                    .min_w(px(FIELD_MIN_WIDTH))
                    // Enter ends a value; let through, a dialog's Confirm would answer the dialog.
                    .on_action(|_: &Confirm, _, _| {})
                    .child(Input::new(&self.input).appearance(false).small()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_end_at_a_separator_and_lines_only_at_enter() {
        let words = Split::Words(crate::text::typed_extensions);
        assert!(words.ends_value("mp4,"));
        assert!(words.ends_value("mp4 "));
        assert!(!words.ends_value("mp4"));
        assert_eq!(words.values(".MP4, mkv"), ["mp4", "mkv"]);
        assert!(!Split::Lines.ends_value("a, b *.iso"));
        assert_eq!(Split::Lines.values("  a, b *.iso "), ["a, b *.iso"]);
        assert_eq!(Split::Lines.values("   "), Vec::<String>::new());
    }
}
