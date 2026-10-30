//! How every window looks before its page draws, and while the settings and the system change
//! (D45): frameless and opaque, in the theme and accent the person chose.
//!
//! - **Background**: every window and its web view are painted in the base colour of the scheme
//!   they show (`--wc-base` in the design tokens) from the moment they exist, so nothing else is
//!   ever on screen before the page draws. No window is see-through: Windows paints a system
//!   material (Mica, Acrylic) only after the window is shown, so one always flashes.
//! - **First paint**: each window gets a script that runs before its page, setting
//!   `data-scheme` (`light` or `dark`, "system" resolved by `prefers-color-scheme`),
//!   `data-accent` and `--accent-system` on the root element, so the first frame is already
//!   right. From then on the page follows the settings itself.
//! - **Live**: a theme change sets the windows' own theme and background too, so the web view's
//!   `prefers-color-scheme` and the colour at a resizing edge match it; the system's accent colour
//!   is read again when a window gains the focus or the system theme changes, and sent on
//!   [`SYSTEM_APPEARANCE_EVENT`] when it changed.

#![forbid(unsafe_code)]

use std::sync::{LazyLock, Mutex, PoisonError};

use serde::Serialize;
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindow, WebviewWindowBuilder};
use wirecatch_protocol::app::{ACCENTS, Accent, AppSettings, THEMES, Theme};
use wirecatch_protocol::desktop::{SYSTEM_APPEARANCE_EVENT, SystemAppearance};

use crate::host::Core;
use crate::window::warn_if_failed;

/// The design tokens, where the base colour of each scheme has its one home.
const TOKENS: &str = include_str!("../../../../packages/ui/src/tokens.css");

/// The light and dark base colours, `light-dark(#rrggbb, #rrggbb)` in the `--wc-base` declaration
/// of `tokens`; `None` when it is missing or not in that form.
fn base_colours(tokens: &str) -> Option<(Color, Color)> {
    let declaration = tokens.split("--wc-base:").nth(1)?.split(';').next()?;
    let pair = declaration
        .trim()
        .strip_prefix("light-dark(")?
        .strip_suffix(')')?;
    let (light, dark) = pair.split_once(',')?;
    Some((hex(light.trim())?, hex(dark.trim())?))
}

/// An opaque colour written `#rrggbb`.
fn hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#').filter(|d| d.len() == 6)?;
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some(Color(channel(0)?, channel(2)?, channel(4)?, u8::MAX))
}

/// The colour a window and its web view show before the page draws: the base of `scheme`.
/// `None` only if the tokens lost their `--wc-base` pair, which a test rules out.
#[must_use]
pub fn window_background(scheme: tauri::Theme) -> Option<Color> {
    static BASE: LazyLock<Option<(Color, Color)>> = LazyLock::new(|| base_colours(TOKENS));
    let (light, dark) = (*BASE)?;
    Some(if scheme == tauri::Theme::Dark {
        dark
    } else {
        light
    })
}

/// Light or dark, for the theme setting and whether the system is dark now.
#[must_use]
pub fn scheme(theme: Theme, system_dark: bool) -> tauri::Theme {
    match theme {
        Theme::Dark => tauri::Theme::Dark,
        Theme::System if system_dark => tauri::Theme::Dark,
        Theme::Light | Theme::System => tauri::Theme::Light,
    }
}

/// Whether Windows is set to dark apps: the text colour it gives apps is light then.
#[cfg(windows)]
fn system_is_dark() -> bool {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Foreground))
        .is_ok_and(|text| u16::from(text.R) + u16::from(text.G) + u16::from(text.B) > 3 * 128)
}

/// Linux has no one answer before a window exists; [`paint_background`] corrects the guess from
/// the window's own theme as soon as it is built.
#[cfg(not(windows))]
fn system_is_dark() -> bool {
    false
}

/// Paints `window` in the base colour of the scheme it shows now: after it is built, which settles
/// the system's light or dark on every platform, and after each theme change.
/// When its theme cannot be read, the window keeps the colour it has.
pub fn paint_background<R: Runtime>(window: &WebviewWindow<R>) {
    let label = window.label();
    match window.theme() {
        Ok(shown) => warn_if_failed(
            format_args!("paint the background of the window {label}"),
            window.set_background_color(window_background(shown)),
        ),
        Err(e) => tracing::warn!("could not read the theme of the window {label}: {e}"),
    }
}

/// The window theme Tauri gives a window for `theme`: `None` follows the system.
#[must_use]
pub fn window_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}

/// What a window's first-paint script knows: `ShellLook` in `apps/desktop/src/lib/first-paint.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstPaint {
    /// The theme setting; the script resolves `system` with `prefers-color-scheme`.
    pub theme: Theme,
    /// The accent setting.
    pub accent: Accent,
    /// `--accent-system`, when the system has an accent colour.
    pub system_accent: Option<String>,
    /// Every theme this build has, to check the look the page remembered against.
    pub themes: &'static [Theme],
    /// Every accent this build has, likewise.
    pub accents: &'static [Accent],
}

impl FirstPaint {
    /// What a window opened now shows, for `settings` (the defaults while the download list is
    /// still opening).
    #[must_use]
    pub fn new(settings: Option<&AppSettings>, system_accent: Option<String>) -> Self {
        let (theme, accent) = settings.map_or((Theme::default(), Accent::default()), |s| {
            (s.theme, s.accent)
        });
        Self {
            theme,
            accent,
            system_accent,
            themes: THEMES,
            accents: ACCENTS,
        }
    }

    /// The script that runs before the page's own: `first-paint.js` as a plain script (no
    /// `export`), inside a function so it adds nothing to the page, calling `paintFirst` with
    /// these values.
    #[must_use]
    pub fn script(&self) -> String {
        // Serializing these plain values cannot fail; the JSON is a JavaScript literal.
        let values = serde_json::to_string(self).unwrap_or_else(|_| "null".to_owned());
        format!(
            "(() => {{\n{}\npaintFirst({values}, document, window);\n}})();\n",
            plain_script(FIRST_PAINT_MODULE)
        )
    }
}

/// The rules for a window's look, shared with the page (`apps/desktop/src/lib/first-paint.js`).
const FIRST_PAINT_MODULE: &str = include_str!("../../src/lib/first-paint.js");

/// `module` as a plain script: `export` taken off each line that starts with it. The module
/// imports nothing at run time (a test holds it to that), so nothing else needs changing.
fn plain_script(module: &str) -> String {
    module
        .lines()
        .map(|line| line.strip_prefix("export ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The settings now, or `None` while the download list is still opening.
fn current_settings<R: Runtime>(app: &AppHandle<R>) -> Option<AppSettings> {
    app.try_state::<Core>()
        .and_then(|core| core.get().map(wirecatch_core::Manager::get_settings))
}

/// Makes `builder` a frameless, opaque window in the current theme, painted in its scheme's base
/// colour, with the first-paint script. Every window is built through this; call
/// [`paint_background`] once it is built.
pub fn frameless<'a, R: Runtime, M: tauri::Manager<R>>(
    app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let settings = current_settings(app);
    let first_paint = FirstPaint::new(settings.as_ref(), last_system_accent(app));
    let builder = builder
        .decorations(false)
        .theme(window_theme(first_paint.theme))
        .initialization_script(first_paint.script());
    match window_background(scheme(first_paint.theme, system_is_dark())) {
        Some(colour) => builder.background_color(colour),
        None => builder,
    }
}

/// The system's accent colour, as the app last read it.
#[derive(Default)]
pub struct SystemAccent(Mutex<Option<String>>);

impl SystemAccent {
    /// Reads it now, when the app starts.
    #[must_use]
    pub fn read() -> Self {
        Self(Mutex::new(read_system_accent()))
    }

    fn with<T>(&self, change: impl FnOnce(&mut Option<String>) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The system's accent colour, as the app last read it.
#[must_use]
pub fn last_system_accent<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.try_state::<SystemAccent>()
        .and_then(|state| state.with(|known| known.clone()))
}

/// The system turned light or dark under the window `label`: a window following the system takes
/// its new background at once, and the system's accent colour, which may change with it, is read
/// again.
pub fn system_theme_changed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        paint_background(&window);
    }
    refresh_system_accent(app);
}

/// Reads the system's accent colour again, off the window thread, and tells every window when
/// it changed.
pub fn refresh_system_accent<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<SystemAccent>() else {
            return;
        };
        let now = read_system_accent();
        let changed = state.with(|known| std::mem::replace(known, now.clone()) != now);
        if changed {
            warn_if_failed(
                "tell the windows the system accent changed",
                app.emit(SYSTEM_APPEARANCE_EVENT, SystemAppearance { accent: now }),
            );
        }
    });
}

/// The theme the windows were last given.
#[derive(Default)]
pub struct AppliedTheme(Mutex<Option<Theme>>);

impl AppliedTheme {
    /// Records `theme` as given; `false` when the windows already have it.
    pub fn changes_to(&self, theme: Theme) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(theme)
            != Some(theme)
    }
}

/// Gives every open window the theme `theme`, so its background and its web view's
/// `prefers-color-scheme` follow the setting; nothing when they already have it. Called when the
/// download list opens (a window built before then got the system's theme) and whenever the
/// settings change. A window built later gets it when it is built ([`frameless`]).
pub fn follow_theme<R: Runtime>(app: &AppHandle<R>, theme: Theme) {
    if app
        .try_state::<AppliedTheme>()
        .is_some_and(|applied| !applied.changes_to(theme))
    {
        return;
    }
    for window in app.webview_windows().values() {
        warn_if_failed(
            format_args!("change the theme of the window {}", window.label()),
            window.set_theme(window_theme(theme)),
        );
        paint_background(window);
    }
}

/// Gives `window` the stored theme, once its page has drawn. A window built while the app
/// starts is only queued then, and a theme given to it before the system has made it is lost, so
/// [`follow_theme`] at that moment cannot reach it.
pub fn match_stored_theme<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    if let Some(settings) = current_settings(window.app_handle()) {
        warn_if_failed(
            format_args!("give the window {} its theme", window.label()),
            window.set_theme(window_theme(settings.theme)),
        );
        paint_background(window);
    }
}

/// `#rrggbb` for a colour's red, green and blue.
#[must_use]
pub fn hex_colour(red: u8, green: u8, blue: u8) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// The accent colour chosen in Windows, or `None` when the system does not say.
#[cfg(windows)]
#[must_use]
pub fn read_system_accent() -> Option<String> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let colour = UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Accent))
        .inspect_err(|e| tracing::debug!("could not read the system accent colour: {e}"))
        .ok()?;
    Some(hex_colour(colour.R, colour.G, colour.B))
}

/// Linux desktops have no accent colour Wirecatch can read; "system" shows cyan there.
#[cfg(not(windows))]
#[must_use]
pub fn read_system_accent() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_is_opaque_in_its_schemes_base_colour() {
        let light = window_background(tauri::Theme::Light).expect("tokens.css declares --wc-base");
        let dark = window_background(tauri::Theme::Dark).expect("tokens.css declares --wc-base");
        assert_eq!((light.3, dark.3), (u8::MAX, u8::MAX), "never see-through");
        assert_ne!(light, dark);
    }

    #[test]
    fn the_base_colours_are_the_tokens_light_dark_pair() {
        assert_eq!(
            base_colours("a { --wc-base: light-dark(#eceef2, #12151B); }"),
            Some((Color(0xec, 0xee, 0xf2, 0xff), Color(0x12, 0x15, 0x1b, 0xff)))
        );
        assert_eq!(base_colours("a { --wc-base: #eceef2; }"), None);
        assert_eq!(
            base_colours("a { --wc-base: light-dark(#ecee, #12151b); }"),
            None
        );
        assert_eq!(
            base_colours("a { --wc-other: light-dark(#eceef2, #12151b); }"),
            None
        );
    }

    #[test]
    fn the_scheme_follows_the_setting_and_the_system_only_when_asked() {
        assert_eq!(scheme(Theme::Light, true), tauri::Theme::Light);
        assert_eq!(scheme(Theme::Dark, false), tauri::Theme::Dark);
        assert_eq!(scheme(Theme::System, true), tauri::Theme::Dark);
        assert_eq!(scheme(Theme::System, false), tauri::Theme::Light);
    }

    #[test]
    fn the_windows_are_given_a_theme_only_when_it_changes() {
        let applied = AppliedTheme::default();
        assert!(applied.changes_to(Theme::System), "the first time, always");
        assert!(!applied.changes_to(Theme::System));
        assert!(applied.changes_to(Theme::Light));
        assert!(!applied.changes_to(Theme::Light));
    }

    #[test]
    fn the_window_theme_follows_the_setting() {
        assert_eq!(window_theme(Theme::System), None);
        assert_eq!(window_theme(Theme::Light), Some(tauri::Theme::Light));
        assert_eq!(window_theme(Theme::Dark), Some(tauri::Theme::Dark));
    }

    #[test]
    fn colours_are_written_as_css_hex() {
        assert_eq!(hex_colour(0, 120, 212), "#0078d4");
        assert_eq!(hex_colour(255, 255, 255), "#ffffff");
    }

    #[test]
    fn a_window_opened_before_the_settings_load_shows_the_defaults() {
        let first = FirstPaint::new(None, None);
        assert_eq!((first.theme, first.accent), (Theme::System, Accent::Cyan));
    }

    #[test]
    fn the_first_paint_script_is_the_shared_module_as_a_plain_script() {
        let module = FIRST_PAINT_MODULE;
        assert!(
            !module.lines().any(|l| l.starts_with("import ")),
            "first-paint.js runs as a plain script, so it can import nothing at run time"
        );
        let script = FirstPaint::new(None, None).script();
        assert!(
            !script.lines().any(|l| l.starts_with("export ")),
            "{script}"
        );
        assert!(script.contains("function paintFirst(shell, doc, win)"));
    }

    /// The scripts the page's tests run (`apps/desktop/src/lib/first-paint.generated.json`),
    /// exactly as the shell gives them to windows. When the module or the values change, this
    /// writes the file again and fails once, so the new file is committed with the change.
    #[test]
    fn the_scripts_the_page_tests_run_are_current() {
        let case = |theme, accent, system_accent: Option<&str>| FirstPaint {
            theme,
            accent,
            system_accent: system_accent.map(str::to_owned),
            themes: THEMES,
            accents: ACCENTS,
        };
        // Sorted by name whatever serde_json's features: `preserve_order`, which another crate in
        // the workspace may turn on, would keep a `json!` object in the order written.
        let cases = std::collections::BTreeMap::from([
            (
                "dark_green",
                case(Theme::Dark, Accent::Green, Some("#0078d4")).script(),
            ),
            (
                "system_accent_on_windows",
                case(Theme::Light, Accent::System, Some("#a94dc1")).script(),
            ),
            (
                "system_accent_on_linux",
                case(Theme::System, Accent::System, None).script(),
            ),
        ]);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/lib/first-paint.generated.json");
        let written = format!("{}\n", serde_json::to_string_pretty(&cases).unwrap());
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current.replace("\r\n", "\n") != written {
            std::fs::write(&path, &written).unwrap();
            panic!(
                "{} was out of date and is written again: commit it",
                path.display()
            );
        }
    }
}
