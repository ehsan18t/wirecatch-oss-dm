//! Menu items beyond gpui-kit's own.

use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::{App, SharedString, Styled as _};

/// An item that deletes files: a plain labelled item with its icon in the danger colour (D272).
pub fn danger_item(
    label: impl Into<SharedString>,
    icon: impl Into<Icon>,
    cx: &App,
) -> PopupMenuItem {
    PopupMenuItem::new(label).icon(icon.into().text_color(cx.theme().danger))
}
