//! A rounded square with the picture of what a queue is doing, at the start of its row, with a
//! badge in its corner when the queue adds downloads by its rules.

use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, px,
};

use super::tokens;

const SIZE: f32 = 30.;
const BADGE: f32 = 16.;

#[derive(IntoElement)]
pub struct QueueTile {
    icon: Icon,
    tone: Option<Hsla>,
    /// Its rules: on, off, or none.
    rules: Option<bool>,
    /// The row's colour, cut around the badge; a hover colour for the row's group.
    behind: Hsla,
    hover: Option<(SharedString, Hsla)>,
}

impl QueueTile {
    pub fn new(icon: impl Into<Icon>, behind: Hsla) -> Self {
        Self {
            icon: icon.into(),
            tone: None,
            rules: None,
            behind,
            hover: None,
        }
    }

    /// Tints the tile, as the colour of the queue's state.
    pub fn tone(mut self, tone: Hsla) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn rules(mut self, rules: Option<bool>) -> Self {
        self.rules = rules;
        self
    }

    /// The badge's cut follows the row's hover colour, for a row in `group`.
    pub fn hover_behind(mut self, group: impl Into<SharedString>, color: Hsla) -> Self {
        self.hover = Some((group.into(), color));
        self
    }
}

impl RenderOnce for QueueTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let badge = self.rules.map(|on| {
            let (icon, bg, fg) = if on {
                (
                    gpui_kit::assets::IconName::Zap,
                    theme.primary,
                    theme.primary_foreground,
                )
            } else {
                (
                    gpui_kit::assets::IconName::ZapOff,
                    theme.secondary_active,
                    theme.muted_foreground,
                )
            };
            div()
                .absolute()
                .right(px(-5.))
                .bottom(px(-5.))
                .size(px(BADGE))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(bg)
                .text_color(fg)
                .border_2()
                .border_color(self.behind)
                .when_some(self.hover.clone(), |badge, (group, color)| {
                    badge.group_hover(group, move |style| style.border_color(color))
                })
                .child(Icon::new(icon).with_size(px(BADGE - 7.)))
        });
        let tone = self.tone;
        div()
            .relative()
            .size(px(SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .bg(tone.map_or(theme.secondary, |tone| tone.opacity(0.15)))
            .text_color(tone.unwrap_or(theme.muted_foreground))
            .child(self.icon.with_size(tokens::ICON))
            .children(badge)
    }
}
