//! The title bar of a small window that fits its content (a download window, the download
//! complete window): the stock bar's look with Minimize and Close only. GPUI's own bar always
//! offers Maximize on Windows, which such a window has no use for.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TITLE_BAR_HEIGHT, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Decorations, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, WindowControlArea,
    div, px,
};

/// The bar, with `title` on the left.
pub fn small_title_bar(title: impl IntoElement, window: &Window, cx: &App) -> AnyElement {
    let theme = cx.theme();
    // Under server-side decorations the window manager draws its own buttons.
    let own_buttons = cfg!(target_os = "windows")
        || matches!(window.window_decorations(), Decorations::Client { .. });
    h_flex()
        .id("small-title-bar")
        .flex_shrink_0()
        .h(TITLE_BAR_HEIGHT)
        .pl(px(12.))
        .border_b_1()
        .border_color(theme.title_bar_border)
        .bg(theme.title_bar)
        .child(
            h_flex()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .when(cfg!(target_os = "linux"), |bar| {
                    bar.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                })
                .child(title),
        )
        .when(own_buttons, |bar| {
            bar.child(button(
                "minimize",
                IconName::WindowMinimize,
                WindowControlArea::Min,
                cx,
            ))
            .child(button(
                "close",
                IconName::WindowClose,
                WindowControlArea::Close,
                cx,
            ))
        })
        .into_any_element()
}

fn button(
    name: &'static str,
    icon: IconName,
    area: WindowControlArea,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let closes = matches!(area, WindowControlArea::Close);
    let (fill, ink) = if closes {
        (theme.danger, theme.danger_foreground)
    } else {
        (theme.secondary_hover, theme.secondary_foreground)
    };
    div()
        .id(ElementId::Name(name.into()))
        .flex()
        .flex_shrink_0()
        .w(TITLE_BAR_HEIGHT)
        .h_full()
        .items_center()
        .justify_center()
        .text_color(theme.foreground)
        .hover(move |style| style.bg(fill).text_color(ink))
        .when(cfg!(target_os = "windows"), |button| {
            button.window_control_area(area)
        })
        .when(cfg!(target_os = "linux"), |button| {
            button
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if closes {
                        window.remove_window();
                    } else {
                        window.minimize_window();
                    }
                })
        })
        .child(Icon::new(icon).small())
}
