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

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::assets::IconName;
    use gpui_kit::component::Icon;
    use gpui_kit::component::menu::PopupMenuItem;

    use super::danger_item;

    #[gpui_kit::test]
    fn a_danger_item_is_found_by_its_label(cx: &mut TestAppContext) {
        let item = cx.update(|cx| {
            gpui_kit::component::init(cx);
            danger_item("Remove", Icon::new(IconName::Trash), cx)
        });
        // The menu names only a plain item, from its label, for screen readers and queries.
        assert!(matches!(
            &item,
            PopupMenuItem::Item { label, icon: Some(_), .. } if label.as_ref() == "Remove"
        ));
    }
}
