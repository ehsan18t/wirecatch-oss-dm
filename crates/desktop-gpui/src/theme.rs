//! The look of every window: Wirecatch's light and dark colours (`assets/theme.json`), with the
//! accent the person chose laid over them (D85, D99).

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, SharedString, WindowAppearance, rgb};
use wirecatch_protocol::app::{Accent, Theme as ThemeSetting};

const THEMES: &str = include_str!("../assets/theme.json");
const LIGHT: &str = "Wirecatch Light";
const DARK: &str = "Wirecatch Dark";

/// Text on a filled accent: white on the light scheme's deep tones, near black on the dark
/// scheme's bright ones.
const ON_ACCENT_LIGHT: u32 = 0x00ff_ffff;
const ON_ACCENT_DARK: u32 = 0x0012_1110;

/// A destructive button: a deep red with white text in both schemes, where the dark scheme's pale
/// danger colour, made to read as text, would fill it in pastel.
const DANGER_FILL_LIGHT: u32 = 0x00b6_322d;
const DANGER_FILL_DARK: u32 = 0x00c9_372c;

/// Sharper than Segoe UI at small sizes; Windows 11 has it, older systems keep the system font.
const PREFERRED_FONT: &str = "Segoe UI Variable Text";

fn preferred_font(cx: &App) -> Option<SharedString> {
    cx.text_system()
        .all_font_names()
        .iter()
        .any(|name| name == PREFERRED_FONT)
        .then(|| PREFERRED_FONT.into())
}

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
    let danger_fill: Hsla = rgb(if dark {
        DANGER_FILL_DARK
    } else {
        DANGER_FILL_LIGHT
    })
    .into();
    let font = preferred_font(cx);
    Theme::update(cx, |theme| {
        if let Some(font) = font {
            theme.font_family = font;
        }
        theme.primary = tone;
        theme.primary_hover = tone.opacity(0.9);
        theme.primary_active = tone.opacity(0.8);
        theme.primary_foreground = on_tone;
        theme.button_primary = tone;
        theme.button_primary_hover = tone.opacity(0.9);
        theme.button_primary_active = tone.opacity(0.8);
        theme.button_primary_foreground = on_tone;
        theme.button_danger = danger_fill;
        theme.button_danger_hover = danger_fill.opacity(0.9);
        theme.button_danger_active = danger_fill.opacity(0.8);
        theme.button_danger_foreground = rgb(ON_ACCENT_LIGHT).into();
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
        theme.sidebar_accent = tone.opacity(0.12);
        theme.sidebar_accent_foreground = tone;
        theme.sidebar_primary = tone;
        theme.sidebar_primary_foreground = on_tone;
    });
}

/// The accent in this scheme and the text that goes on it, each pair at 4.5:1 contrast or more.
/// The system accent takes the hue and saturation of the colour chosen in Windows at Wirecatch's
/// own lightness, so its text keeps that contrast; without one it is Wirecatch's cyan.
pub fn accent_colors(accent: Accent, dark: bool) -> (Hsla, Hsla) {
    let (tone, on_tone) = fixed_colors(accent, dark);
    if accent != Accent::System {
        return (tone, on_tone);
    }
    let Some([red, green, blue]) = wirecatch_shell::appearance::system_accent() else {
        return (tone, on_tone);
    };
    let system: Hsla = gpui_kit::Rgba {
        r: f32::from(red) / 255.,
        g: f32::from(green) / 255.,
        b: f32::from(blue) / 255.,
        a: 1.,
    }
    .into();
    (
        Hsla {
            h: system.h,
            s: system.s,
            ..tone
        },
        on_tone,
    )
}

/// The accents Wirecatch has, the system's shown as cyan.
fn fixed_colors(accent: Accent, dark: bool) -> (Hsla, Hsla) {
    let (light_tone, dark_tone) = match accent {
        Accent::Cyan | Accent::System => (0x0007_7c86, 0x0044_d4e2),
        Accent::Blue => (0x0035_71ba, 0x0092_c1fd),
        Accent::Indigo => (0x0065_68bd, 0x00b1_b7fd),
        Accent::Purple => (0x0086_5eb1, 0x00d0_aafc),
        Accent::Pink => (0x00ac_507c, 0x00f9_9dc6),
        Accent::Red => (0x00b4_514c, 0x00fd_a19a),
        Accent::Orange => (0x00ae_5724, 0x00fd_a578),
        Accent::Amber => (0x0098_6603, 0x00eb_b25f),
        Accent::Teal => (0x000a_7f6f, 0x0052_d7c1),
        Accent::Green => (0x0019_8044, 0x007f_d497),
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
