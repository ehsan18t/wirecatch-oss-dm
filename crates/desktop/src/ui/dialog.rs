//! Every dialog of the app opens through here, so all of them move alike.

use gpui_kit::component::dialog::{AlertDialog, Dialog, DialogEntrance};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, px,
};

use crate::ui::{Role, Typed as _};

/// The default slides down from the window top for 250 ms; this settles in place.
const ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;
/// gpui-kit's padding on each side of a dialog, which the header's rule reaches past.
const DIALOG_PADDING: f32 = 16.;

/// A dialog's title over a rule across the whole dialog, with a fixed gap before its content.
#[derive(IntoElement)]
pub struct Header {
    title: SharedString,
}

impl Header {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl RenderOnce for Header {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .mx(px(-DIALOG_PADDING))
            .px(px(DIALOG_PADDING))
            .pb_3()
            .mb_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().text_role(Role::PageTitle, cx).child(self.title))
    }
}

pub fn open(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        build(dialog.entrance(ENTRANCE), window, cx)
    });
}

/// Gives the keyboard to `field` once the dialog just opened has taken it for itself.
pub fn focus_after_opening<F: gpui_kit::Focusable + 'static>(
    field: &gpui_kit::Entity<F>,
    window: &mut Window,
    cx: &mut App,
) {
    let field = field.clone();
    window.defer(cx, move |window, cx| {
        field.read(cx).focus_handle(cx).focus(window, cx);
    });
}

pub fn open_alert(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(AlertDialog, &mut Window, &mut App) -> AlertDialog + 'static,
) {
    window.open_alert_dialog(cx, move |alert, window, cx| {
        build(alert.entrance(ENTRANCE), window, cx)
    });
}

/// A form's one-line field, holding `value`.
pub fn text_field(
    value: impl Into<String>,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        state.set_value(value, window, cx);
        state
    })
}
