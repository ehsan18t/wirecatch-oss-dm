//! The main window: created on demand, hidden and asleep when closed to the tray (D13), and the links
//! passed on the command line that its Add dialog should offer.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindowBuilder};
use wirecatch_protocol::desktop::{REVEAL_EVENT, Reveal};

/// Label of the one window, as declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

pub use wirecatch_shell::launch::{BACKGROUND_FLAG, Launch, MAX_LAUNCH_LINKS, wants_background};

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
/// notification the person clicked was about, the browser settings, or the proxy settings.
pub fn reveal<R: Runtime>(app: &AppHandle<R>, reveal: &Reveal) {
    let listening = app
        .try_state::<PendingSelection>()
        .is_some_and(|pending| pending.request(reveal));
    // Showing wakes a page asleep in the tray first, so the selection reaches it awake.
    show_main_window(app);
    if listening && (!reveal.ids.is_empty() || reveal.browser_settings || reveal.proxy_settings) {
        warn_if_failed(
            "show what the main window was asked for",
            app.emit_to(MAIN_WINDOW, REVEAL_EVENT, reveal),
        );
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

    #[test]
    fn at_most_twenty_links_are_kept() {
        let found: Vec<String> = (0..MAX_LAUNCH_LINKS)
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        let waiting = LaunchLinks::default();
        waiting.push(found.clone());
        waiting.push(found);
        assert_eq!(waiting.take().len(), MAX_LAUNCH_LINKS);
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
