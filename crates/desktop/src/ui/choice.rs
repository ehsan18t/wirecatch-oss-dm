//! Picking one of a few options laid side by side, or any of them.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Toggle, ToggleGroup, ToggleVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, ElementId, Styled as _, Window};

/// One of `options`, the `picked` one pressed; `pick` gets the index pressed.
pub fn one_of(
    id: impl Into<ElementId>,
    options: &[&'static str],
    picked: usize,
    pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> ToggleGroup {
    group(id)
        .children(
            options
                .iter()
                .enumerate()
                .map(|(index, label)| option(index, label, index == picked, cx)),
        )
        .on_click(move |pressed, window, cx| {
            // Pressing the picked option again unpresses it; it stays picked.
            if let Some(index) = (0..pressed.len()).find(|&i| pressed[i] && i != picked) {
                pick(&index, window, cx);
            }
        })
}

/// Any of `options`, each pressed or not; `pick` gets the indexes pressed after a click.
pub fn any_of(
    id: impl Into<ElementId>,
    options: &[(&'static str, bool)],
    pick: impl Fn(&[usize], &mut Window, &mut App) + 'static,
    cx: &App,
) -> ToggleGroup {
    group(id)
        .children(
            options
                .iter()
                .enumerate()
                .map(|(index, (label, pressed))| option(index, label, *pressed, cx)),
        )
        .on_click(move |pressed, window, cx| {
            let indexes: Vec<usize> = (0..pressed.len()).filter(|&i| pressed[i]).collect();
            pick(&indexes, window, cx);
        })
}

fn group(id: impl Into<ElementId>) -> ToggleGroup {
    ToggleGroup::new(id).segmented().outline()
}

/// A pressed option takes the tint the navigation marks its current item with.
fn option(index: usize, label: &'static str, pressed: bool, cx: &App) -> Toggle {
    let theme = cx.theme();
    Toggle::new(index)
        .label(label)
        .checked(pressed)
        .when(pressed, |option| {
            option
                .bg(theme.sidebar_accent)
                .text_color(theme.sidebar_accent_foreground)
        })
}
