//! A setting that is off until switched on: its label and help, what applies while it is off,
//! and once on, the fields that set it.

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Styled as _,
    Window, div,
};

use super::{Role, Typed as _};

type Toggled = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct SwitchRow {
    id: ElementId,
    label: SharedString,
    help: Option<SharedString>,
    /// What applies while it is off, such as the global value it follows.
    off: Option<SharedString>,
    checked: bool,
    ruled: bool,
    on_toggle: Option<Toggled>,
    fields: Option<AnyElement>,
}

impl SwitchRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            help: None,
            off: None,
            checked,
            ruled: false,
            on_toggle: None,
            fields: None,
        }
    }

    pub fn help(mut self, help: impl Into<SharedString>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn off(mut self, off: Option<impl Into<SharedString>>) -> Self {
        self.off = off.map(Into::into);
        self
    }

    /// Draws a line above it, between it and the row before.
    pub fn ruled(mut self, ruled: bool) -> Self {
        self.ruled = ruled;
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }

    /// Shown under the row while it is on.
    pub fn fields(mut self, fields: impl IntoElement) -> Self {
        self.fields = Some(fields.into_any_element());
        self
    }
}

impl RenderOnce for SwitchRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let border = cx.theme().border;
        let switch = Switch::new(self.id)
            .small()
            .checked(self.checked)
            .when_some(self.on_toggle, |switch, handler| switch.on_click(handler));
        v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .when(self.ruled, |row| row.border_t_1().border_color(border))
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_role(Role::Strong, cx).child(self.label))
                            .when_some(self.help, |text, help| {
                                text.child(div().text_role(Role::Meta, cx).child(help))
                            }),
                    )
                    .when(!self.checked, |row| {
                        row.when_some(self.off, |row, off| {
                            row.child(div().pt_0p5().text_role(Role::Meta, cx).child(off))
                        })
                    })
                    .child(switch),
            )
            .when(self.checked, |row| {
                row.when_some(self.fields, ParentElement::child)
            })
    }
}

pub fn group(title: &'static str, rows: Vec<AnyElement>, cx: &App) -> gpui_kit::Div {
    v_flex()
        .gap_2()
        .child(div().text_role(Role::Label, cx).child(title))
        .child(panel(cx).children(rows))
}

/// A bordered box the rows of one subject sit in.
pub fn panel(cx: &App) -> gpui_kit::Div {
    let theme = cx.theme();
    v_flex()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
}
