//! How every window looks before its page draws, and while the settings and the system change
//! (D36): frameless, with the system's material behind it, in the theme and accent the person
//! chose.
//!
//! - **Material**: Mica on Windows 11, Acrylic on Windows 10, nothing on Linux, where the page
//!   paints a solid background. A window with a material is transparent, and its page says so
//!   with `data-material` on its root element.
//! - **First paint**: each window gets a script that runs before its page, setting
//!   `data-scheme` (`light` or `dark`, "system" resolved by `prefers-color-scheme`),
//!   `data-accent`, `data-material` and `--accent-system` on the root element, so the first
//!   frame is already right. From then on the page follows the settings itself.
//! - **Live**: a theme change sets the windows' own theme too, so the material and the web
//!   view's `prefers-color-scheme` match it; the system's accent colour is read again when a
//!   window gains the focus or the system theme changes, and sent on
//!   [`SYSTEM_APPEARANCE_EVENT`] when it changed.

#![forbid(unsafe_code)]

use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindowBuilder};
use wirecatch_protocol::app::{ACCENTS, Accent, AppSettings, THEMES, Theme};
use wirecatch_protocol::desktop::{SYSTEM_APPEARANCE_EVENT, SystemAppearance};

use crate::host::Core;
use crate::window::warn_if_failed;

/// The system's background effect behind a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    /// Windows 11's.
    Mica,
    /// Windows 10's.
    Acrylic,
}

/// The first Windows 11 build.
const WINDOWS_11_BUILD: u32 = 22_000;
/// Windows 10 version 1809, the first that blurs a window behind with Acrylic.
const WINDOWS_10_ACRYLIC_BUILD: u32 = 17_763;

/// The material for Windows build `build`: none before Acrylic existed.
#[must_use]
pub fn material_for_windows(build: u32) -> Option<Material> {
    if build >= WINDOWS_11_BUILD {
        Some(Material::Mica)
    } else if build >= WINDOWS_10_ACRYLIC_BUILD {
        Some(Material::Acrylic)
    } else {
        None
    }
}

/// The material this system puts behind windows.
#[must_use]
pub fn material() -> Option<Material> {
    #[cfg(windows)]
    {
        material_for_windows(windows_version::OsVersion::current().build)
    }
    #[cfg(not(windows))]
    {
        None
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
    /// `data-material`, when there is one.
    pub material: Option<Material>,
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
            material: material(),
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

/// How a window shows the material behind its page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Backing {
    /// The web view is see-through wherever the page paints nothing.
    see_through: bool,
    /// The window keeps a drawing surface of its own, under the web view's. Over a material it
    /// must not: whatever is drawn there shows through the page, and the web view's hidden child
    /// window draws there, at the least the box between its scroll bars, a white square in the
    /// bottom-right corner (D36, item 2).
    own_surface: bool,
}

impl Backing {
    const fn for_material(material: Option<Material>) -> Self {
        let see_through = material.is_some();
        Self {
            see_through,
            own_surface: !see_through,
        }
    }
}

/// Makes `builder` a frameless window with the system's material, in the current theme, with
/// the first-paint script. Every window is built through this.
pub fn frameless<'a, R: Runtime, M: tauri::Manager<R>>(
    app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let settings = current_settings(app);
    let first_paint = FirstPaint::new(settings.as_ref(), last_system_accent(app));
    let backing = Backing::for_material(first_paint.material);
    let builder = builder
        .decorations(false)
        .theme(window_theme(first_paint.theme))
        .initialization_script(first_paint.script())
        .transparent(backing.see_through)
        .no_redirection_bitmap(!backing.own_surface);
    match first_paint.material {
        Some(material) => builder.effects(
            EffectsBuilder::new()
                .effect(match material {
                    Material::Mica => Effect::Mica,
                    Material::Acrylic => Effect::Acrylic,
                })
                .build(),
        ),
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

/// Gives every open window the theme `theme`, so its material and its web view's
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
    fn windows_11_gets_mica_and_windows_10_acrylic() {
        assert_eq!(material_for_windows(26_100), Some(Material::Mica));
        assert_eq!(material_for_windows(22_000), Some(Material::Mica));
        assert_eq!(material_for_windows(19_045), Some(Material::Acrylic));
        assert_eq!(material_for_windows(17_763), Some(Material::Acrylic));
        assert_eq!(material_for_windows(17_134), None);
    }

    #[test]
    fn a_window_over_a_material_has_no_surface_of_its_own_to_show_through() {
        for material in [Material::Mica, Material::Acrylic] {
            let backing = Backing::for_material(Some(material));
            assert!(backing.see_through, "{material:?}");
            assert!(!backing.own_surface, "{material:?}");
        }
        let solid = Backing::for_material(None);
        assert!(!solid.see_through && solid.own_surface);
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
        let case = |theme, accent, material, system_accent: Option<&str>| FirstPaint {
            theme,
            accent,
            material,
            system_accent: system_accent.map(str::to_owned),
            themes: THEMES,
            accents: ACCENTS,
        };
        let cases = serde_json::json!({
            "dark_green_on_mica": case(Theme::Dark, Accent::Green, Some(Material::Mica), Some("#0078d4")).script(),
            "system_accent_on_acrylic": case(Theme::Light, Accent::System, Some(Material::Acrylic), Some("#a94dc1")).script(),
            "system_accent_on_linux": case(Theme::System, Accent::System, None, None).script(),
        });
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
