//! Every dialog of the app opens through here, so all of them move alike.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::{AlertDialog, Dialog, DialogEntrance};
use gpui_kit::{App, Window};

/// The default slides down from the window top for 250 ms; this settles in place.
const ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;

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
