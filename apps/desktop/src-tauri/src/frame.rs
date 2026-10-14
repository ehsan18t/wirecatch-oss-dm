//! The title bars of the frameless windows (D36): their caption buttons, the state they show,
//! and the system's snap layouts over the maximize button.
//!
//! Each page draws its own title bar and calls `control_window` for its buttons. The state a
//! title bar shows ([`WindowFrame`]) is sent to its window on [`WINDOW_FRAME_EVENT`] whenever it
//! changes.
//!
//! **Snap layouts.** Windows 11 offers its snap layouts when the pointer rests on a window's
//! maximize button, which it finds by asking the window under the pointer what is there
//! (`WM_NCHITTEST` answered `HTMAXBUTTON`). Over a frameless window that is the web view, which
//! only ever answers "page". So the page says where its maximize button is
//! (`set_maximize_button`, in CSS pixels) and the shell lays a small invisible window of its own
//! exactly over it (`crate::maximize_overlay`, on Windows) that answers `HTMAXBUTTON`. Since that
//! window, not the page, then has the pointer there, it maximizes or restores the window on a
//! click itself and reports the pointer being over it as [`WindowFrame::maximize_hovered`], so
//! the page can draw the hover. The shell keeps the page's area and places the overlay again
//! when the window changes size or moves to a screen with another scale, and removes it when a
//! page starts loading, so each page reports its own button. Chosen over `tauri-plugin-decorum`
//! (unmaintained since 2024; it opens the snap layouts by faking the Win+Z keys) and
//! `tauri-plugin-frame` (draws its own title bar, and its repository is gone).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter as _, Manager as _, Runtime, WebviewWindow};
use wirecatch_protocol::desktop::{ButtonArea, WINDOW_FRAME_EVENT, WindowAction, WindowFrame};

use crate::window::warn_if_failed;

/// A rectangle in physical pixels: left, top, width, height.
pub type PhysicalArea = (i32, i32, i32, i32);

/// The largest button area accepted, in physical pixels on each side: anything larger is not a
/// caption button.
const MAX_BUTTON_SIDE: f64 = 1024.0;

/// `area` (CSS pixels) in physical pixels at `scale`, or `None` when it is not a button anyone
/// could point at: empty, negative, not a number, or absurdly large.
#[must_use]
pub fn physical_area(area: ButtonArea, scale: f64) -> Option<PhysicalArea> {
    let ButtonArea {
        x,
        y,
        width,
        height,
    } = area;
    let usable = |v: f64| v.is_finite() && (0.0..=MAX_BUTTON_SIDE * 8.0).contains(&v);
    let side = |v: f64| usable(v) && v > 0.0 && v <= MAX_BUTTON_SIDE;
    if !(scale.is_finite() && scale > 0.0 && usable(x) && usable(y) && side(width) && side(height))
    {
        return None;
    }
    let px = |v: f64| {
        // In range by the checks above: at most 8192 CSS pixels times a sane scale.
        wirecatch_engine::numeric::f64_to_u64((v * scale).round())
            .and_then(|v| i32::try_from(v).ok())
    };
    Some((px(x)?, px(y)?, px(width)?, px(height)?))
}

/// Whether a window may do `action`: one that cannot be maximized (a download window) is never
/// maximized from its page either.
#[must_use]
pub fn allows(action: WindowAction, maximizable: bool) -> bool {
    action != WindowAction::ToggleMaximize || maximizable
}

/// Per window: where its page's maximize button is, whether the pointer is over it, and what
/// its title bar was last told.
#[derive(Default)]
pub struct Frames(Mutex<HashMap<String, FrameState>>);

#[derive(Default)]
struct FrameState {
    /// The page's maximize button, in CSS pixels.
    area: Option<ButtonArea>,
    maximize_hovered: bool,
    /// What the window was last told, so a resize that changes nothing sends nothing.
    last_sent: Option<WindowFrame>,
}

impl Frames {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, FrameState>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps where the page of window `label` has its maximize button (`None`: it has none).
    pub fn remember(&self, label: &str, area: Option<ButtonArea>) {
        self.lock().entry(label.to_owned()).or_default().area = area;
    }

    /// Where the maximize button of window `label` is in physical pixels at `scale`, if its
    /// page said.
    #[must_use]
    pub fn placement(&self, label: &str, scale: f64) -> Option<PhysicalArea> {
        self.lock()
            .get(label)
            .and_then(|f| f.area)
            .and_then(|area| physical_area(area, scale))
    }

    /// Records whether the pointer is over the maximize button of window `label`; `false` when
    /// that is no change, or the window is not known (closed meanwhile).
    pub fn set_hovered(&self, label: &str, hovered: bool) -> bool {
        self.lock()
            .get_mut(label)
            .is_some_and(|f| std::mem::replace(&mut f.maximize_hovered, hovered) != hovered)
    }

    fn hovered(&self, label: &str) -> bool {
        self.lock().get(label).is_some_and(|f| f.maximize_hovered)
    }

    /// Records `frame` as sent to window `label`; `false` when it was already.
    fn sends(&self, label: &str, frame: WindowFrame) -> bool {
        self.lock()
            .entry(label.to_owned())
            .or_default()
            .last_sent
            .replace(frame)
            != Some(frame)
    }

    /// A new page is loading in window `label`: it has reported no button yet, and has been
    /// told nothing.
    pub fn page_loading(&self, label: &str) {
        if let Some(f) = self.lock().get_mut(label) {
            *f = FrameState::default();
        }
    }

    /// Window `label` is gone.
    pub fn forget(&self, label: &str) {
        self.lock().remove(label);
    }
}

/// The state `window`'s title bar shows.
#[must_use]
pub fn frame_of<R: Runtime>(window: &WebviewWindow<R>) -> WindowFrame {
    WindowFrame {
        maximized: window.is_maximized().unwrap_or(false),
        focused: window.is_focused().unwrap_or(false),
        maximize_hovered: window
            .try_state::<Frames>()
            .is_some_and(|frames| frames.hovered(window.label())),
    }
}

/// Sends `window` its title bar's state, when it changed since it was last sent.
pub fn send_frame<R: Runtime>(window: &WebviewWindow<R>) {
    let frame = frame_of(window);
    let changed = window
        .try_state::<Frames>()
        .is_none_or(|frames| frames.sends(window.label(), frame));
    if !changed {
        return;
    }
    warn_if_failed(
        format_args!("tell the window {} its state", window.label()),
        window.emit_to(window.label(), WINDOW_FRAME_EVENT, frame),
    );
}

/// Does what a caption button asks. A window that cannot be maximized ignores a maximize.
///
/// # Errors
///
/// The system refused, or the window is already gone.
pub fn control<R: Runtime>(window: &WebviewWindow<R>, action: WindowAction) -> tauri::Result<()> {
    if !allows(action, window.is_maximizable()?) {
        tracing::debug!("the window {} cannot be maximized", window.label());
        return Ok(());
    }
    match action {
        WindowAction::Minimize => window.minimize(),
        WindowAction::ToggleMaximize => toggle_maximize(window),
        // As its X: the main window may only hide to the tray (`lifecycle`).
        WindowAction::Close => window.close(),
    }
}

/// Maximizes `window`, or restores it when it is maximized.
///
/// # Errors
///
/// The system refused, or the window is already gone.
pub fn toggle_maximize<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    if window.is_maximized()? {
        window.unmaximize()
    } else {
        window.maximize()
    }
}

/// The page of `window` says where its maximize button is now (`None`: it has none). A window
/// that cannot be maximized offers no snap layouts, whatever its page says.
pub fn set_maximize_button<R: Runtime>(window: &WebviewWindow<R>, area: Option<ButtonArea>) {
    let Some(frames) = window.try_state::<Frames>() else {
        return;
    };
    let area = area.filter(|_| window.is_maximizable().unwrap_or(false));
    frames.remember(window.label(), area);
    place_overlay(window);
}

/// Lays the shell's window over the maximize button where the page last said it is, at the
/// window's scale now, named for what a click does; removes it when the page said nothing.
/// Called when the page reports, and when the window changes size or moves to a screen with
/// another scale.
pub fn place_overlay<R: Runtime>(window: &WebviewWindow<R>) {
    let Some(frames) = window.try_state::<Frames>() else {
        return;
    };
    let area = frames.placement(window.label(), window.scale_factor().unwrap_or(1.0));
    #[cfg(windows)]
    {
        let restores = window.is_maximized().unwrap_or(false);
        let this = window.clone();
        warn_if_failed(
            "place the window over the maximize button",
            window.run_on_main_thread(move || {
                crate::maximize_overlay::place(&this, area, restores);
            }),
        );
    }
    #[cfg(not(windows))]
    let _ = area;
}

/// A page starts loading in `window` (a reload, or recovering from a crash): the button the
/// last page reported is gone with it.
pub fn page_loading<R: Runtime>(window: &WebviewWindow<R>) {
    if let Some(frames) = window.try_state::<Frames>() {
        frames.page_loading(window.label());
    }
    place_overlay(window);
}

/// The pointer moved onto or off the maximize button of the window `label`.
pub fn hover<R: Runtime>(app: &AppHandle<R>, label: &str, hovered: bool) {
    let changed = app
        .try_state::<Frames>()
        .is_some_and(|frames| frames.set_hovered(label, hovered));
    if !changed {
        return;
    }
    if let Some(window) = app.get_webview_window(label) {
        // Off the window thread: reading the window's state there waits on that same thread.
        tauri::async_runtime::spawn(async move { send_frame(&window) });
    }
}

/// The window `label` is gone.
pub fn forget<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(frames) = app.try_state::<Frames>() {
        frames.forget(label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f64, y: f64, width: f64, height: f64) -> ButtonArea {
        ButtonArea {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_button_area_is_scaled_to_physical_pixels() {
        assert_eq!(
            physical_area(area(1000.0, 0.0, 46.0, 32.0), 1.5),
            Some((1500, 0, 69, 48))
        );
        assert_eq!(
            physical_area(area(10.2, 0.4, 46.0, 32.0), 1.0),
            Some((10, 0, 46, 32))
        );
    }

    /// Moving to a screen with another scale changes the button's physical size, not its CSS
    /// size, so the page reports nothing: the shell places it again from what it kept.
    #[test]
    fn the_button_is_placed_again_at_a_new_scale() {
        let frames = Frames::default();
        frames.remember("main", Some(area(1188.0, 0.0, 46.0, 32.0)));
        assert_eq!(frames.placement("main", 1.0), Some((1188, 0, 46, 32)));
        assert_eq!(frames.placement("main", 1.5), Some((1782, 0, 69, 48)));
        frames.page_loading("main");
        assert_eq!(
            frames.placement("main", 1.0),
            None,
            "a new page reports its own"
        );
    }

    #[test]
    fn hovering_is_tracked_only_for_known_windows() {
        let frames = Frames::default();
        assert!(
            !frames.set_hovered("closed", true),
            "a window closed meanwhile"
        );
        frames.remember("main", None);
        assert!(frames.set_hovered("main", true));
        assert!(!frames.set_hovered("main", true));
        frames.page_loading("main");
        assert!(!frames.hovered("main"));
        frames.forget("main");
        assert!(!frames.set_hovered("main", true));
    }

    #[test]
    fn a_window_that_cannot_be_maximized_is_never_maximized() {
        assert!(!allows(WindowAction::ToggleMaximize, false));
        assert!(allows(WindowAction::ToggleMaximize, true));
        assert!(allows(WindowAction::Minimize, false));
        assert!(allows(WindowAction::Close, false));
    }

    #[test]
    fn an_area_no_one_could_point_at_is_refused() {
        assert_eq!(physical_area(area(0.0, 0.0, 0.0, 32.0), 1.0), None);
        assert_eq!(physical_area(area(-5.0, 0.0, 46.0, 32.0), 1.0), None);
        assert_eq!(physical_area(area(f64::NAN, 0.0, 46.0, 32.0), 1.0), None);
        assert_eq!(
            physical_area(area(0.0, 0.0, 46.0, f64::INFINITY), 1.0),
            None
        );
        assert_eq!(physical_area(area(0.0, 0.0, 5000.0, 32.0), 1.0), None);
        assert_eq!(physical_area(area(0.0, 0.0, 46.0, 32.0), 0.0), None);
    }
}
