//! A search box: gpui-kit's input with the search icon and a clear button.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::{Entity, IntoElement};

use super::tokens;

pub fn search_field(state: &Entity<InputState>) -> impl IntoElement {
    Input::new(state)
        .small()
        .cleanable(true)
        .prefix(Icon::new(IconName::Search).with_size(tokens::ICON_SMALL))
}
