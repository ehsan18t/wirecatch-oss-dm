//! Tooltips that give way to an open menu.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Empty, IntoElement, KeyContext, Render, SharedString,
    Window,
};

/// `gpui-component`'s key context of its `PopupMenu`, which it does not export.
const MENU_CONTEXT: &str = "PopupMenu";

/// `build`'s tooltip, or none while focus is inside a menu, whose items it would cover.
pub fn unless_in_menu(
    window: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
) -> AnyView {
    if in_menu(&window.context_stack()) {
        cx.new(|_| NoTooltip).into()
    } else {
        build(window, cx)
    }
}

/// A plain tooltip saying `label`, unless focus is inside a menu.
pub fn plain(
    label: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label: SharedString = label.into();
    move |window, cx| {
        unless_in_menu(window, cx, |window, cx| {
            Tooltip::new(label.clone()).build(window, cx)
        })
    }
}

fn in_menu(focus_path: &[KeyContext]) -> bool {
    focus_path
        .iter()
        .any(|context| context.contains(MENU_CONTEXT))
}

struct NoTooltip;

impl Render for NoTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(contexts: &[&str]) -> Vec<KeyContext> {
        contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap_or_default())
            .collect()
    }

    #[test]
    fn only_focus_inside_a_menu_silences_a_tooltip() {
        assert!(in_menu(&path(&["MainWindow", MENU_CONTEXT])));
        assert!(!in_menu(&path(&["MainWindow", "Input"])));
        assert!(!in_menu(&[]));
    }
}
