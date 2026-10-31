//! The scale every screen draws with: what each piece of text is, and how big icons are. Screens
//! name a role instead of a size, so the whole app changes here.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, FontWeight, Pixels, Styled, px};

/// What a piece of text is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The name of the page a window shows.
    PageTitle,
    /// The name of a dialog's subject, such as a download's file.
    Title,
    Body,
    /// Body text that steps back: a size beside a speed, a hint.
    Secondary,
    /// A value that matters more than the text around it.
    Strong,
    /// A live figure shown large, such as a download's speed.
    Figure,
    /// What sits under or beside a value: a host, a count, a hint.
    Meta,
    /// The small label over a group.
    Label,
    /// The least of a row, under its meta: a folder.
    Caption,
}

pub trait Typed: Styled + Sized {
    fn text_role(self, role: Role, cx: &App) -> Self {
        let theme = cx.theme();
        match role {
            Role::PageTitle => self
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground),
            Role::Title => self
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground),
            Role::Body => self
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.foreground),
            Role::Secondary => self
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.muted_foreground),
            Role::Figure => self
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground),
            Role::Strong => self
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground),
            Role::Meta => self
                .text_xs()
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.muted_foreground),
            Role::Label => self
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground),
            Role::Caption => self
                .text_size(px(11.))
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.muted_foreground.opacity(0.8)),
        }
    }
}

impl<T: Styled> Typed for T {}

/// Icons beside small text: row actions, captions.
pub const ICON_SMALL: Pixels = px(14.);
/// Icons beside body text: navigation, buttons, menus.
pub const ICON: Pixels = px(16.);
