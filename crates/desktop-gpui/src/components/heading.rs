//! The small label over a group: Queues in the sidebar, Activity in the details.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Div, FontWeight, ParentElement as _, SharedString, Styled as _, div};

/// Callers add their own spacing.
pub fn section(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(label.into())
}
