//! The main window: created on demand, hidden and asleep when closed to the tray (D13), and the links
//! passed on the command line that its Add dialog should offer.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager as _, Runtime, WebviewWindowBuilder};
use wirecatch_engine::units::KIB_USIZE;
use wirecatch_protocol::app::DownloadSummary;

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

/// Most browser downloads kept for the main window's notes while its page is not listening.
pub const MAX_PENDING_CAPTURES: usize = 20;

/// Browser downloads that started without a window of their own (D36.7) while the main
/// window's page was not listening yet: the main window is opened for them, and its page, still
/// loading, would miss their `Captured` events, so it takes them once it has loaded. A page
/// listens from the moment it takes them until it goes (a reload, or the window is destroyed).
#[derive(Default)]
pub struct PendingCaptures(Mutex<Captures>);

#[derive(Default)]
struct Captures {
    listening: bool,
    waiting: Vec<DownloadSummary>,
}

impl PendingCaptures {
    fn lock(&self) -> std::sync::MutexGuard<'_, Captures> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A download started without a window: kept unless the page already listens (then its
    /// event reaches it), at most [`MAX_PENDING_CAPTURES`].
    pub fn started(&self, row: &DownloadSummary) {
        let mut captures = self.lock();
        if !captures.listening && captures.waiting.len() < MAX_PENDING_CAPTURES {
            captures.waiting.push(row.clone());
        }
    }

    /// The page has loaded and listens: hands every waiting download over, once.
    pub fn take(&self) -> Vec<DownloadSummary> {
        let mut captures = self.lock();
        captures.listening = true;
        std::mem::take(&mut captures.waiting)
    }

    /// The page is gone (it reloads, or the window was destroyed): it listens no more.
    pub fn page_gone(&self) {
        self.lock().listening = false;
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

/// Whether showing a window gives it the keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// It takes the focus: the person asked for it (the tray, a second launch, the browser).
    Take,
    /// It comes to the front and leaves the focus where it is: a browser download started
    /// without asking while the person keeps browsing (D36.7).
    Leave,
}

/// How a main window that is still opening shows once its page has drawn: the last request
/// wins, and taking the focus wins over leaving it, since the person asked for the window.
#[derive(Default)]
pub struct PendingReveal(Mutex<Option<Focus>>);

impl PendingReveal {
    /// Asks for `focus` when the opening window shows.
    pub fn request(&self, focus: Focus) {
        let mut pending = self.lock();
        if *pending != Some(Focus::Take) {
            *pending = Some(focus);
        }
    }

    /// How to show the window now, once; [`Focus::Take`] when nothing asked otherwise.
    pub fn take(&self) -> Focus {
        self.lock().take().unwrap_or(Focus::Take)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Focus>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Shows the main window, creating it if it was closed to the tray, and brings it to the front
/// with the focus.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    show_main_window_with(app, Focus::Take);
}

/// Shows the main window, creating it if it was closed to the tray, and brings it to the front,
/// with or without the focus.
pub fn show_main_window_with<R: Runtime>(app: &AppHandle<R>, focus: Focus) {
    if let Some(pending) = app.try_state::<PendingReveal>() {
        pending.request(focus);
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        crate::sleep::wake_main(&window);
        let focus = app
            .try_state::<PendingReveal>()
            .map_or(focus, |pending| pending.take());
        match focus {
            Focus::Take => bring_forward(&window),
            Focus::Leave => show_quietly(&window),
        }
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
    let focus = window
        .try_state::<PendingReveal>()
        .map_or(Focus::Take, |pending| pending.take());
    match focus {
        Focus::Take => show_focused(window),
        Focus::Leave => show_quietly(window),
    }
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

    fn started(id: i64) -> DownloadSummary {
        DownloadSummary {
            id: wirecatch_protocol::DownloadId(id),
            url: format!("https://example.com/{id}.zip"),
            file_name: format!("{id}.zip"),
            path: format!("C:/Downloads/{id}.zip"),
            queue: wirecatch_protocol::QueueId(1),
            category: wirecatch_protocol::app::Category::Archive,
            status: wirecatch_protocol::app::DownloadStatus::Downloading,
            total: None,
            done: 0,
            speed: 0,
            connections: 0,
            eta_secs: None,
            error: None,
            waiting: None,
            media: None,
            added_at: 0,
            completed_at: None,
            processing: None,
        }
    }

    /// D36.7 on the start-with-system path: the main window opens for a browser download, and
    /// its page, still loading, gets the note once it has loaded.
    #[test]
    fn a_page_that_was_not_listening_gets_the_downloads_it_missed_once() {
        let captures = PendingCaptures::default();
        captures.started(&started(1));
        captures.started(&started(2));
        let taken: Vec<_> = captures.take().into_iter().map(|r| r.id.0).collect();
        assert_eq!(taken, [1, 2]);
        // Listening now: the events reach the page, nothing is kept twice.
        captures.started(&started(3));
        assert_eq!(captures.take(), []);
        // A reload stops listening until the new page takes them.
        captures.page_gone();
        captures.started(&started(4));
        assert_eq!(captures.take().len(), 1);
    }

    #[test]
    fn at_most_twenty_missed_downloads_are_kept() {
        let captures = PendingCaptures::default();
        for id in 0..50 {
            captures.started(&started(id));
        }
        assert_eq!(captures.take().len(), MAX_PENDING_CAPTURES);
    }

    #[test]
    fn an_opening_window_shows_as_last_asked_and_asking_for_it_wins() {
        let pending = PendingReveal::default();
        assert_eq!(
            pending.take(),
            Focus::Take,
            "nothing asked: the person opened it"
        );
        pending.request(Focus::Leave);
        assert_eq!(pending.take(), Focus::Leave);
        assert_eq!(pending.take(), Focus::Take, "taken once");
        pending.request(Focus::Leave);
        pending.request(Focus::Take);
        pending.request(Focus::Leave);
        assert_eq!(
            pending.take(),
            Focus::Take,
            "the person asked for it meanwhile"
        );
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
