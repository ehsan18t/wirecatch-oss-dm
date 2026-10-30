//! The arrangements screens are built from. Each returns its container, so a caller adds the
//! children and any spacing of its own.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;

use super::{Role, Typed as _};
use gpui_kit::{
    AnyElement, App, Div, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, px,
};

/// Where a block draws its dividing line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    None,
    Above,
    Below,
}

fn ruled(block: Div, rule: Rule, cx: &App) -> Div {
    let border = cx.theme().border;
    match rule {
        Rule::None => block,
        Rule::Above => block.border_t_1().border_color(border),
        Rule::Below => block.border_b_1().border_color(border),
    }
}

/// A window's body: the sidebar, and beside it the header over the content.
pub fn body(sidebar: impl IntoElement, header: impl IntoElement, content: impl IntoElement) -> Div {
    h_flex().flex_1().min_h_0().child(sidebar).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .child(content),
    )
}

/// A padded block of a panel, stacked top to bottom.
pub fn section(rule: Rule, cx: &App) -> Div {
    ruled(v_flex().px_4().py_3(), rule, cx)
}

/// A padded row across a panel: its header or its row of actions.
pub fn bar(rule: Rule, cx: &App) -> Div {
    ruled(h_flex().px_4().py_3().gap_2(), rule, cx)
}

/// A muted label in a column of `label_width`, and its value beside it.
pub fn label_row(
    label_width: f32,
    label: impl IntoElement,
    value: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .gap_3()
        .child(
            h_flex()
                .w(px(label_width))
                .flex_shrink_0()
                .gap_1p5()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// The top of a page: its title, a line under it, and its actions on the right.
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    summary: SharedString,
    actions: Vec<AnyElement>,
}

impl PageHeader {
    pub fn new(title: impl Into<SharedString>, summary: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            summary: summary.into(),
            actions: Vec::new(),
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for PageHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        ruled(h_flex().px_5().py_3().gap_4(), Rule::Below, cx)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_role(Role::PageTitle, cx).child(self.title))
                    .when(!self.summary.is_empty(), |text| {
                        text.child(div().text_role(Role::Meta, cx).child(self.summary))
                    }),
            )
            .children(self.actions)
    }
}
