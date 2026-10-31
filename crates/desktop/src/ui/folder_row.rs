//! Where files are saved: the folder picked, or what applies without one, with Change and, once
//! one is picked, Use default.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div,
};

type Clicked = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct FolderRow {
    picked: Option<SharedString>,
    fallback: SharedString,
    on_change: Clicked,
    on_reset: Clicked,
}

impl FolderRow {
    /// `fallback` is shown while no folder is `picked`.
    pub fn new(
        picked: Option<impl Into<SharedString>>,
        fallback: impl Into<SharedString>,
        on_change: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_reset: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            picked: picked.map(Into::into),
            fallback: fallback.into(),
            on_change: Box::new(on_change),
            on_reset: Box::new(on_reset),
        }
    }
}

impl RenderOnce for FolderRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let picked = self.picked.is_some();
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if picked {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(self.picked.unwrap_or(self.fallback)),
            )
            .when(picked, |row| {
                row.child(
                    Button::new("use-default")
                        .small()
                        .ghost()
                        .label("Use default")
                        .on_click(self.on_reset),
                )
            })
            .child(
                Button::new("change-folder")
                    .small()
                    .outline()
                    .label("Change")
                    .on_click(self.on_change),
            )
    }
}
