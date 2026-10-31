//! A rounded square with the picture of a file's kind, at the start of a download's row.

use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::{
    App, Hsla, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div, px,
};
use wirecatch_protocol::app::Category;

use super::{icon, tokens};

const SIZE: f32 = 32.;

#[derive(IntoElement)]
pub struct FileTile {
    category: Category,
    tone: Option<Hsla>,
}

impl FileTile {
    pub fn new(category: Category) -> Self {
        Self {
            category,
            tone: None,
        }
    }

    /// Tints the tile, as the colour of the download's state.
    pub fn tone(mut self, tone: Hsla) -> Self {
        self.tone = Some(tone);
        self
    }
}

impl RenderOnce for FileTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tone = self.tone.unwrap_or(theme.muted_foreground);
        div()
            .size(px(SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .bg(tone.opacity(0.12))
            .text_color(tone)
            .child(Icon::new(icon::category(self.category)).with_size(tokens::ICON))
    }
}
