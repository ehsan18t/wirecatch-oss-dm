//! The main window: created on demand, hidden and asleep when closed to the tray (D13), and the links
//! passed on the command line that its Add dialog should offer.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindowBuilder};
use wirecatch_engine::units::KIB_USIZE;
use wirecatch_protocol::desktop::{REVEAL_EVENT, Reveal};

/// Label of the one window, as declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

/// The flag the bridge (and the start-with-system entry) passes: run with no window.
pub const BACKGROUND_FLAG: &str = "--background";

/// True when the command line asks to start without a window. `args` includes the program name.
pub fn wants_background<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .any(|a| a.as_ref() == BACKGROUND_FLAG)
}

/// Most links taken from one command line, and kept waiting for the interface at once.
pub const MAX_LAUNCH_LINKS: usize = 20;

/// Longest link taken from a command line, in bytes.
pub const MAX_LINK_LEN: usize = 8 * KIB_USIZE;

/// Schemes a command-line link may use.
const LINK_SCHEMES: [&str; 5] = ["http://", "https://", "ftp://", "ftps://", "ftpes://"];

/// Whether `arg` (already trimmed) is a link the Add dialog may offer: a known scheme with
/// something after it, no control characters, and not absurdly long.
fn is_launch_link(arg: &str) -> bool {
    if arg.len() > MAX_LINK_LEN || arg.chars().any(char::is_control) {
        return false;
    }
    let lower = arg.to_ascii_lowercase();
    LINK_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

/// Web and FTP links on a command line (program name first), in order, for the Add dialog.
/// Anything else on the command line is ignored, and at most [`MAX_LAUNCH_LINKS`] are taken.
pub fn links_from_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .map(|a| a.as_ref().trim().to_owned())
        .filter(|a| is_launch_link(a))
        .take(MAX_LAUNCH_LINKS)
        .collect()
}

/// Links from the command line that the interface has not picked up yet.
#[derive(Default)]
pub struct LaunchLinks(Mutex<Vec<String>>);

impl LaunchLinks {
    /// Keeps `links` for the interface, at most [`MAX_LAUNCH_LINKS`] waiting at once.
    pub fn push(&self, links: impl IntoIterator<Item = String>) {
        let mut waiting = self.lock();
        let room = MAX_LAUNCH_LINKS.saturating_sub(waiting.len());
        waiting.extend(links.into_iter().take(room));
    }

    /// Hands every waiting link over, once.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lock())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Downloads a clicked notification asks the main window to select (D52), or the browser
/// settings the extension asks for (D61), kept while its page is not listening (it was closed to
/// the tray, or is loading because the request started the app). The newest request wins: it is
/// the one the person just made.
#[derive(Default)]
pub struct PendingSelection(Mutex<Selection>);

#[derive(Default)]
struct Selection {
    listening: bool,
    waiting: Option<Reveal>,
}

impl PendingSelection {
    fn lock(&self) -> std::sync::MutexGuard<'_, Selection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Keeps `reveal` unless the page listens, and says whether it listens.
    fn request(&self, reveal: &Reveal) -> bool {
        let mut selection = self.lock();
        if !selection.listening {
            selection.waiting = Some(reveal.clone());
        }
        selection.listening
    }

    /// The page has loaded and listens: hands the waiting request over, once.
    pub fn take(&self) -> Option<Reveal> {
        let mut selection = self.lock();
        selection.listening = true;
        selection.waiting.take()
    }

    /// The page is gone: it listens no more.
    pub fn page_gone(&self) {
        self.lock().listening = false;
    }
}

/// Shows the main window, in front and focused, on what `reveal` names: the downloads a
/// notification the person clicked was about, or the browser settings.
pub fn reveal<R: Runtime>(app: &AppHandle<R>, reveal: &Reveal) {
    let listening = app
        .try_state::<PendingSelection>()
        .is_some_and(|pending| pending.request(reveal));
    // Showing wakes a page asleep in the tray first, so the selection reaches it awake.
    show_main_window(app);
    if listening && (!reveal.ids.is_empty() || reveal.browser_settings) {
        warn_if_failed(
            "show what the main window was asked for",
            app.emit_to(MAIN_WINDOW, REVEAL_EVENT, reveal),
        );
    }
}

/// What a launch asks for, read from its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Started for the browser or at sign-in: show nothing.
    pub background: bool,
    /// Links to offer in the Add dialog.
    pub links: Vec<String>,
}

impl Launch {
    /// Reads a command line (program name first).
    #[must_use]
    pub fn from_args(args: &[String]) -> Self {
        Self {
            background: wants_background(args),
            links: links_from_args(args),
        }
    }

    /// Whether this launch should bring up the window. A link always does, even with
    /// `--background`, because the person asked to download something.
    #[must_use]
    pub fn shows_window(&self) -> bool {
        !self.background || !self.links.is_empty()
    }
}

/// Shows the main window, creating it if it was closed to the tray, and brings it to the front
/// with the focus.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        crate::sleep::wake_main(&window);
        bring_forward(&window);
        return;
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
    else {
        tracing::error!("the main window is missing from tauri.conf.json");
        return;
    };
    // Opened hidden: the page shows it once its first view is drawn (`main_window_ready`), so
    // it never flashes blank. If the page never says so, it is shown after a moment anyway.
    let built = WebviewWindowBuilder::from_config(app, config)
        .and_then(|b| crate::appearance::frameless(app, b).visible(false).build());
    match built {
        Ok(window) => {
            crate::appearance::paint_background(&window);
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(REVEAL_PATIENCE).await;
                reveal_main(&window);
            });
        }
        Err(e) => tracing::error!("could not open the window: {e}"),
    }
}

/// The longest a new main window stays hidden waiting for its page to draw.
pub const REVEAL_PATIENCE: std::time::Duration = std::time::Duration::from_secs(3);

/// Shows a new main window and brings it to the front, unless it is hidden in the tray on
/// purpose or already showing. Another window calling this changes nothing.
pub fn reveal_main<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    if window.label() != MAIN_WINDOW || crate::sleep::skips_events(window.app_handle(), MAIN_WINDOW)
    {
        return;
    }
    if window.is_visible().unwrap_or(true) {
        return;
    }
    show_focused(window);
}

/// Shows `window` in front, restored if minimized, without taking the focus: while it cannot be
/// focused the system shows it without activating it.
pub fn show_quietly<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let label = window.label();
    warn_if_failed(
        format_args!("keep the window {label} from taking the focus"),
        window.set_focusable(false),
    );
    warn_if_failed(
        format_args!("restore the minimized window {label}"),
        window.unminimize(),
    );
    warn_if_failed(format_args!("show the window {label}"), window.show());
    warn_if_failed(
        format_args!("let the window {label} take the focus again"),
        window.set_focusable(true),
    );
}

/// Brings `window` to the front: restored if minimized, shown and focused.
pub fn bring_forward<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    warn_if_failed(
        format_args!("restore the minimized window {}", window.label()),
        window.unminimize(),
    );
    show_focused(window);
}

/// Shows `window` and gives it the focus.
pub fn show_focused<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let label = window.label();
    warn_if_failed(format_args!("show the window {label}"), window.show());
    warn_if_failed(format_args!("focus the window {label}"), window.set_focus());
}

/// Logs a window or event call that failed. Nothing can be retried: the window is closing or
/// the system refused, and the person can still act on the window themselves.
pub fn warn_if_failed(action: impl std::fmt::Display, result: tauri::Result<()>) {
    if let Err(e) = result {
        tracing::warn!("could not {action}: {e}");
    }
}

/// Handles a launch: keeps its links for the interface and shows the window when it should.
pub fn handle_launch<R: Runtime>(app: &AppHandle<R>, launch: &Launch) {
    if !launch.links.is_empty() {
        if let Some(links) = app.try_state::<LaunchLinks>() {
            links.push(launch.links.iter().cloned());
        }
        // An open interface picks them up now; a new one asks for them when it loads.
        warn_if_failed(
            "tell the interface that links arrived",
            app.emit(crate::LINKS_EVENT, ()),
        );
    }
    if launch.shows_window() {
        show_main_window(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn background_flag_is_read_after_the_program_name() {
        assert!(wants_background(["wirecatch-app", "--background"]));
        assert!(!wants_background(["wirecatch-app"]));
        assert!(!wants_background(["--background"]));
        assert!(!wants_background(["wirecatch-app", "--backgrounds"]));
    }

    #[test]
    fn links_are_picked_from_the_command_line() {
        let found = links_from_args([
            "C:\\Apps\\wirecatch-app.exe",
            "https://example.com/a.zip",
            "--background",
            "notes.txt",
            "HTTP://EXAMPLE.COM/B",
            "ftp://files.example.com/c.iso",
            "https://",
        ]);
        assert_eq!(
            found,
            [
                "https://example.com/a.zip",
                "HTTP://EXAMPLE.COM/B",
                "ftp://files.example.com/c.iso"
            ]
        );
        assert_eq!(
            links_from_args(["https://not-the-program-name.example"]),
            [] as [String; 0]
        );
    }

    #[test]
    fn odd_or_hostile_arguments_are_not_taken_as_links() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_LEN));
        let found = links_from_args([
            "app",
            "  https://example.com/ok  ",
            "ftpes://files.example.com/x",
            "https://example.com/a\nb",
            "https://example.com/\u{7}bell",
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1)",
            "data:text/html,hi",
            long.as_str(),
        ]);
        assert_eq!(
            found,
            ["https://example.com/ok", "ftpes://files.example.com/x"]
        );
    }

    #[test]
    fn at_most_twenty_links_are_taken_and_kept() {
        let many: Vec<String> = std::iter::once("app".to_string())
            .chain((0..50).map(|i| format!("https://example.com/{i}")))
            .collect();
        let found = links_from_args(&many);
        assert_eq!(found.len(), MAX_LAUNCH_LINKS);
        assert_eq!(found[0], "https://example.com/0");

        let waiting = LaunchLinks::default();
        waiting.push(found.clone());
        waiting.push(found);
        assert_eq!(waiting.take().len(), MAX_LAUNCH_LINKS);
    }

    #[test]
    fn a_background_launch_shows_nothing_unless_it_brings_a_link() {
        assert!(!Launch::from_args(&args(&["app", "--background"])).shows_window());
        assert!(Launch::from_args(&args(&["app"])).shows_window());
        let with_link = Launch::from_args(&args(&["app", "--background", "https://x.example/f"]));
        assert!(with_link.shows_window());
        assert_eq!(with_link.links, ["https://x.example/f"]);
    }

    #[test]
    fn launch_links_are_handed_over_once() {
        let links = LaunchLinks::default();
        links.push(["https://a.example/1".to_string()]);
        links.push(["https://a.example/2".to_string()]);
        assert_eq!(links.take(), ["https://a.example/1", "https://a.example/2"]);
        assert_eq!(links.take(), [] as [String; 0]);
    }
}
