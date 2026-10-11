//! The main window: created on demand, destroyed when closed to the tray (D13), and the links
//! passed on the command line that its Add dialog should offer.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindowBuilder};

/// Label of the one window, as declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

/// The flag the bridge (and the start-with-system entry) passes: run with no window.
pub const BACKGROUND_FLAG: &str = "--background";

/// Tells the webview that links are waiting; it fetches them with `take_launch_links`.
pub const LINKS_EVENT: &str = "wirecatch://links";

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
pub const MAX_LINK_LEN: usize = 8 * 1024;

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
        self.0.lock().unwrap_or_else(|e| e.into_inner())
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
    pub fn from_args(args: &[String]) -> Self {
        Self {
            background: wants_background(args),
            links: links_from_args(args),
        }
    }

    /// Whether this launch should bring up the window. A link always does, even with
    /// `--background`, because the person asked to download something.
    pub fn shows_window(&self) -> bool {
        !self.background || !self.links.is_empty()
    }
}

/// Shows the main window, creating it if it was closed to the tray, and brings it to the front.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
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
    match WebviewWindowBuilder::from_config(app, config).and_then(|b| b.build()) {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => tracing::error!("could not open the window: {e}"),
    }
}

/// Handles a launch: keeps its links for the interface and shows the window when it should.
pub fn handle_launch<R: Runtime>(app: &AppHandle<R>, launch: &Launch) {
    if !launch.links.is_empty() {
        if let Some(links) = app.try_state::<LaunchLinks>() {
            links.push(launch.links.iter().cloned());
        }
        // An open interface picks them up now; a new one asks for them when it loads.
        let _ = app.emit(LINKS_EVENT, ());
    }
    if launch.shows_window() {
        show_main_window(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
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
        assert!(links_from_args(["https://not-the-program-name.example"]).is_empty());
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
        assert!(links.take().is_empty());
    }
}
