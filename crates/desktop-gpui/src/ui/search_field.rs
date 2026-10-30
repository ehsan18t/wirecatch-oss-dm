//! A search box: gpui-kit's input with the search icon, a clear button and its shortcut.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::{Action, Entity, IntoElement, Window};

use super::tokens;
use gpui_kit::prelude::FluentBuilder as _;

/// `focus` is the action that moves to the box; its key, where one is bound in `context`, shows
/// at the end.
pub fn search_field(
    state: &Entity<InputState>,
    focus: &dyn Action,
    context: &str,
    window: &Window,
) -> impl IntoElement {
    let shortcut = Kbd::binding_for_action(focus, Some(context), window);
    Input::new(state)
        .small()
        .cleanable(true)
        .prefix(Icon::new(IconName::Search).with_size(tokens::ICON_SMALL))
        .when_some(shortcut, |input, kbd| input.suffix(kbd.outline()))
}
