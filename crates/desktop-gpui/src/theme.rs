//! The look of every window: Wirecatch's light and dark colours (`assets/theme.json`), with the
//! accent the person chose laid over them (D85, D99).

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, WindowAppearance, rgb};
use wirecatch_protocol::app::{Accent, Theme as ThemeSetting};

const THEMES: &str = include_str!("../assets/theme.json");
const LIGHT: &str = "Wirecatch Light";
const DARK: &str = "Wirecatch Dark";

/// Text on a filled accent: white on the light scheme's deep tones, near black on the dark
/// scheme's bright ones.
const ON_ACCENT_LIGHT: u32 = 0x00ff_ffff;
const ON_ACCENT_DARK: u32 = 0x000b_0d12;

/// Registers the two schemes. Call once, after `gpui_kit::init`. Should `assets/theme.json` not
/// parse (its test catches that first), the windows keep gpui-kit's own colours.
pub fn init(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    if let Err(e) = registry.load_themes_from_str(THEMES) {
        tracing::error!("could not read the colours: {e}");
        return;
    }
    let (Some(light), Some(dark)) = (registry.themes().get(LIGHT), registry.themes().get(DARK))
    else {
        return;
    };
    let (light, dark) = (Rc::clone(light), Rc::clone(dark));
    Theme::update(cx, |theme| {
        theme.light_theme = light;
        theme.dark_theme = dark;
    });
}

/// Shows every window in the scheme `setting` names (the system's for
/// [`ThemeSetting::System`]) with `accent`.
pub fn apply(setting: ThemeSetting, accent: Accent, cx: &mut App) {
    let dark = match setting {
        ThemeSetting::Light => false,
        ThemeSetting::Dark => true,
        ThemeSetting::System => matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    };
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let (tone, on_tone) = accent_colors(accent, dark);
    Theme::update(cx, |theme| {
        theme.primary = tone;
        theme.primary_hover = tone.opacity(0.9);
        theme.primary_active = tone.opacity(0.8);
        theme.primary_foreground = on_tone;
        theme.ring = tone;
        theme.caret = tone;
        theme.link = tone;
        theme.link_hover = tone.opacity(0.85);
        theme.link_active = tone.opacity(0.75);
        theme.progress_bar = tone;
        theme.slider_bar = tone;
        theme.drag_border = tone;
        theme.drop_target = tone.opacity(0.16);
        theme.selection = tone.opacity(0.32);
        theme.list_active = tone.opacity(0.14);
        theme.list_active_border = tone.opacity(0.6);
        theme.table_active = tone.opacity(0.14);
        theme.table_active_border = tone.opacity(0.6);
        theme.sidebar_primary = tone;
        theme.sidebar_primary_foreground = on_tone;
    });
}

/// The accent in this scheme and the text that goes on it. Each pair is the one the web
/// interface's tokens hold to 4.5:1 (`packages/ui/src/tokens.css`). The system accent is not
/// read yet, so it shows Wirecatch's own.
fn accent_colors(accent: Accent, dark: bool) -> (Hsla, Hsla) {
    let (light_tone, dark_tone) = match accent {
        Accent::Cyan | Accent::System => (0x000a_6b70, 0x004f_d1c7),
        Accent::Blue => (0x0000_65b8, 0x0061_cdff),
        Accent::Indigo => (0x0047_47c9, 0x00a5_b4fc),
        Accent::Purple => (0x006b_33cc, 0x00be_99ff),
        Accent::Pink => (0x00b0_1f67, 0x00f9_8bc2),
        Accent::Red => (0x00b8_1d2a, 0x00ff_858f),
        Accent::Orange => (0x00a3_4008, 0x00ff_a352),
        Accent::Amber => (0x008c_5a00, 0x00f5_b942),
        Accent::Teal => (0x0003_6e56, 0x003f_d8a8),
        Accent::Green => (0x000b_6f3d, 0x0059_cf94),
    };
    if dark {
        (rgb(dark_tone).into(), rgb(ON_ACCENT_DARK).into())
    } else {
        (rgb(light_tone).into(), rgb(ON_ACCENT_LIGHT).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_schemes_parse() {
        let mut registry = ThemeRegistry::default();
        registry.load_themes_from_str(THEMES).unwrap();
        assert!(registry.themes().contains_key(LIGHT));
        assert!(registry.themes().contains_key(DARK));
    }
}
