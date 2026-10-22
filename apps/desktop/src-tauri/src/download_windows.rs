//! A window for each download (D23).
//!
//! Every question the core asks about a browser download (confirm a file, the video being read,
//! pick a quality, install the media add-on) opens its own small window in front, whatever the
//! main window is doing. After Download now the same window becomes that download's progress
//! window. "Open in its own window" in the main list opens one for any download.
//!
//! - **Labels**: `download-q<question id>` for a window opened for a question and
//!   `download-d<download id>` for one opened for a download, so the two id spaces never meet. A
//!   question window keeps its label when it starts showing its download; the [`Registry`]
//!   knows what each window shows now.
//! - **Limit**: at most [`MAX_QUESTION_WINDOWS`] windows ask a question at once; later
//!   questions wait and open, oldest first, as those windows close or start showing progress.
//! - **Placement**: each window takes the lowest free slot, and slot `n` sits
//!   [`DOWNLOAD_WINDOW_CASCADE_STEP`] logical pixels down and right of slot `n - 1`, on the
//!   screen under the pointer.
//! - **Front**: windows open always on top and focused, and drop always-on-top the first time
//!   they lose focus after having had it, so they behave like normal windows from then on.
//! - **Closing**: closing a question window does not answer it. The question stays open in the
//!   core and is counted in the main window's sidebar; it is not opened again by itself (only an
//!   event that moves it on, or the person, opens it again).
//!
//! - **Scope**: a download window shows text that websites chose, so it is trusted with little
//!   (D43). Its capability grants only the commands it calls, and [`may_act_on`] and
//!   [`may_change`] keep those to the question or download it shows and to its own two settings.
//!
//! Windows are created on the main thread ([`AppHandle::run_on_main_thread`]), never from an
//! async task or a command directly, so nothing blocks and nothing deadlocks on Windows.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, LogicalPosition, Manager as _, Runtime, WebviewUrl, WebviewWindowBuilder};
use wirecatch_core::Manager;
use wirecatch_engine::numeric::{f64_to_u64, saturating_cast};
use wirecatch_protocol::app::{
    AppSettingsPatch, DOWNLOAD_WINDOW_CASCADE_STEP, DOWNLOAD_WINDOW_MAX_HEIGHT,
    DOWNLOAD_WINDOW_MIN_HEIGHT, DOWNLOAD_WINDOW_WIDTH, MAX_QUESTION_WINDOWS,
};
use wirecatch_protocol::desktop::{DOWNLOAD_PARAMETER, DOWNLOAD_WINDOW_PAGE, QUESTION_PARAMETER};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::{DownloadId, QuestionId};

use crate::host::Core;
use crate::questions::open_question_ids;
use crate::window::{bring_forward, show_focused, warn_if_failed};

/// Every download window's label starts with this. The capability in
/// `capabilities/download-windows.json` matches it as `download-*`.
pub const LABEL_PREFIX: &str = "download-";

/// A download window's size when it opens, in logical pixels.
pub const WINDOW_SIZE: (f64, f64) = (DOWNLOAD_WINDOW_WIDTH as f64, 600.0);

/// The smallest a download window can be made, in logical pixels.
pub const MIN_WINDOW_SIZE: (f64, f64) = (360.0, DOWNLOAD_WINDOW_MIN_HEIGHT as f64);

/// The most a download window takes, in logical pixels: past it, its body scrolls.
pub const MAX_WINDOW_HEIGHT: f64 = DOWNLOAD_WINDOW_MAX_HEIGHT as f64;

/// The space kept free above and below a window that fits its content, in logical pixels.
pub const SCREEN_MARGIN: f64 = 48.0;

/// The height a download window takes for content `wanted` pixels tall: what it needs, never
/// less than the smallest window and never taller than the most a window takes or the screen it
/// is on (`screen`, the height of its work area, when the system says). A height that is not a number gives the
/// smallest window.
#[must_use]
pub fn fitted_height(wanted: f64, screen: Option<f64>) -> f64 {
    let floor = MIN_WINDOW_SIZE.1;
    let ceiling = screen.filter(|h| h.is_finite()).map_or(WINDOW_SIZE.1, |h| {
        (h - 2.0 * SCREEN_MARGIN).min(MAX_WINDOW_HEIGHT).max(floor)
    });
    if !wanted.is_finite() {
        return floor;
    }
    wanted.ceil().clamp(floor, ceiling)
}

/// Makes a download window as tall as its content (`height`, in logical pixels, as the page
/// measured it), keeping its width, and moves it up if it would now reach past the bottom of
/// its screen.
pub fn fit_to_content<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    let monitor = window.current_monitor().ok().flatten();
    let scale = window.scale_factor().unwrap_or(1.0);
    let area = monitor.as_ref().map(|m| {
        let s = m.scale_factor();
        let work = m.work_area();
        (
            f64::from(work.position.y) / s,
            f64::from(work.size.height) / s,
        )
    });
    let height = fitted_height(height, area.map(|(_, h)| h));
    let Ok(size) = window.inner_size() else {
        return;
    };
    let width = f64::from(size.width) / scale;
    if (f64::from(size.height) / scale - height).abs() < 1.0 {
        return;
    }
    warn_if_failed(
        "resize a download window",
        window.set_size(tauri::LogicalSize::new(width, height)),
    );
    if let (Some((top, screen)), Ok(at)) = (area, window.outer_position()) {
        let y = f64::from(at.y) / scale;
        let bottom = top + screen - SCREEN_MARGIN / 2.0;
        if y + height > bottom {
            let x = f64::from(at.x) / scale;
            warn_if_failed(
                "move a download window onto the screen",
                window.set_position(LogicalPosition::new(x, (bottom - height).max(top))),
            );
        }
    }
}

/// Whether `label` is a download window's.
#[must_use]
pub fn is_download_window(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// What a download window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shows {
    /// An open question, by its id in the core.
    Question(QuestionId),
    /// A download's progress.
    Download(DownloadId),
}

impl Shows {
    /// The label of a window opened for this.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Question(id) => format!("{LABEL_PREFIX}q{}", id.0),
            Self::Download(id) => format!("{LABEL_PREFIX}d{}", id.0),
        }
    }

    /// The interface's page for this, relative to the app's address.
    #[must_use]
    pub fn route(self) -> String {
        match self {
            Self::Question(id) => format!("{DOWNLOAD_WINDOW_PAGE}?{QUESTION_PARAMETER}={}", id.0),
            Self::Download(id) => format!("{DOWNLOAD_WINDOW_PAGE}?{DOWNLOAD_PARAMETER}={}", id.0),
        }
    }
}

/// An open download window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// Its label.
    pub label: String,
    /// What it shows now.
    pub shows: Shows,
    /// Its place in the cascade (see [`place`]).
    pub slot: usize,
}

/// What to do with the windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Create this window.
    Open(OpenWindow),
    /// Bring the window with this label to the front.
    Focus(String),
}

/// Which download windows are open, which questions wait for a window, and which ones the
/// person put aside by closing their window. Pure bookkeeping: [`Step`]s say what to do.
#[derive(Debug, Default)]
pub struct Registry {
    windows: Vec<OpenWindow>,
    waiting: VecDeque<QuestionId>,
    put_aside: BTreeSet<QuestionId>,
}

impl Registry {
    /// The open windows, in the order they opened.
    #[must_use]
    pub fn windows(&self) -> &[OpenWindow] {
        &self.windows
    }

    /// What the window `label` shows now, or `None` when no such window is open.
    #[must_use]
    pub fn shows(&self, label: &str) -> Option<Shows> {
        self.windows
            .iter()
            .find(|w| w.label == label)
            .map(|w| w.shows)
    }

    /// Questions waiting for room to open their window, oldest first.
    pub fn waiting(&self) -> impl Iterator<Item = QuestionId> + '_ {
        self.waiting.iter().copied()
    }

    fn asking(&self) -> usize {
        self.windows
            .iter()
            .filter(|w| matches!(w.shows, Shows::Question(_)))
            .count()
    }

    /// The lowest slot no open window takes. One of the first `windows.len() + 1` slots is
    /// always free, so the search ends there.
    fn free_slot(&self) -> usize {
        (0..=self.windows.len())
            .find(|slot| !self.windows.iter().any(|w| w.slot == *slot))
            .unwrap_or(self.windows.len())
    }

    fn window_showing(&self, shows: Shows) -> Option<&OpenWindow> {
        self.windows.iter().find(|w| w.shows == shows)
    }

    fn place_question(&mut self, id: QuestionId) -> Option<Step> {
        if self.window_showing(Shows::Question(id)).is_some() || self.waiting.contains(&id) {
            return None;
        }
        if self.asking() >= MAX_QUESTION_WINDOWS {
            self.waiting.push_back(id);
            return None;
        }
        let shows = Shows::Question(id);
        let window = OpenWindow {
            label: shows.label(),
            shows,
            slot: self.free_slot(),
        };
        self.windows.push(window.clone());
        Some(Step::Open(window))
    }

    /// The core asked question `id`, or moved it on (the video was read, the add-on is
    /// needed): its window opens, or waits for room, unless it is already open or waiting.
    pub fn ask(&mut self, id: QuestionId) -> Option<Step> {
        self.put_aside.remove(&id);
        self.place_question(id)
    }

    /// Questions found open in the core (`open`, oldest first) at start or after missed
    /// events: windows for those that have none, except the ones the person put aside.
    pub fn restore(&mut self, open: &[QuestionId]) -> Vec<Step> {
        self.put_aside.retain(|id| open.contains(id));
        self.waiting.retain(|id| open.contains(id));
        let mut steps = Vec::new();
        for id in open {
            if !self.put_aside.contains(id) {
                steps.extend(self.place_question(*id));
            }
        }
        steps
    }

    /// The person asked to see every open question (`open`, oldest first): open windows come
    /// to the front, closed ones open again.
    pub fn show_all(&mut self, open: &[QuestionId]) -> Vec<Step> {
        self.waiting.retain(|id| open.contains(id));
        let mut steps = Vec::new();
        for id in open {
            self.put_aside.remove(id);
            match self.window_showing(Shows::Question(*id)) {
                Some(window) => steps.push(Step::Focus(window.label.clone())),
                None => steps.extend(self.place_question(*id)),
            }
        }
        steps
    }

    /// Question `id` is no longer open in the core. Its window, if any, says so itself.
    pub fn closed(&mut self, id: QuestionId) {
        self.waiting.retain(|w| *w != id);
        self.put_aside.remove(&id);
    }

    /// The window `label` now shows download `id` (its question was answered there): it no
    /// longer counts as asking, so a waiting question may open.
    pub fn became_download(&mut self, label: &str, id: DownloadId) -> Vec<Step> {
        if let Some(window) = self.windows.iter_mut().find(|w| w.label == label) {
            window.shows = Shows::Download(id);
        }
        self.fill()
    }

    /// The window `label` is gone. A question it showed was put aside by the person (closing
    /// is not an answer), so it does not open again by itself. Waiting questions may open.
    pub fn gone(&mut self, label: &str) -> Vec<Step> {
        if let Some(at) = self.windows.iter().position(|w| w.label == label) {
            let window = self.windows.remove(at);
            if let Shows::Question(id) = window.shows {
                self.put_aside.insert(id);
            }
        }
        self.fill()
    }

    /// The window `label` could not be created: forget it, without putting its question aside.
    pub fn failed(&mut self, label: &str) -> Vec<Step> {
        self.windows.retain(|w| w.label != label);
        self.fill()
    }

    /// "Open in its own window" for download `id`: the window already showing it comes to the
    /// front, or a new one opens. Not limited: the person asked for it.
    pub fn show_download(&mut self, id: DownloadId) -> Step {
        if let Some(window) = self.window_showing(Shows::Download(id)) {
            return Step::Focus(window.label.clone());
        }
        let shows = Shows::Download(id);
        let window = OpenWindow {
            label: shows.label(),
            shows,
            slot: self.free_slot(),
        };
        self.windows.push(window.clone());
        Step::Open(window)
    }

    fn fill(&mut self) -> Vec<Step> {
        let mut steps = Vec::new();
        while self.asking() < MAX_QUESTION_WINDOWS {
            let Some(id) = self.waiting.pop_front() else {
                break;
            };
            steps.extend(self.place_question(id));
        }
        steps
    }
}

/// A screen's usable area, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// Where the window in `slot` goes on `area`, for a window of `size` (all logical pixels). The
/// first windows spread around the middle of the screen, each [`DOWNLOAD_WINDOW_CASCADE_STEP`]
/// down and right of the one before; slots that would leave the screen start again at the top
/// left of the cascade. A window larger than the screen goes to its top left corner.
#[must_use]
pub fn place(slot: usize, area: Area, size: (f64, f64)) -> (f64, f64) {
    let room_x = (area.width - size.0).max(0.0);
    let room_y = (area.height - size.1).max(0.0);
    // The first few windows centre on the screen together.
    let lead = cascade_offset(MAX_QUESTION_WINDOWS - 1) / 2.0;
    let start_x = (room_x / 2.0 - lead).max(0.0);
    let start_y = (room_y / 2.0 - lead).max(0.0);
    let fit = whole_steps(room_x - start_x)
        .min(whole_steps(room_y - start_y))
        .saturating_add(1);
    let offset = cascade_offset(slot % fit);
    (area.x + start_x + offset, area.y + start_y + offset)
}

/// `steps` cascade steps, in logical pixels: whole numbers, so exact as an `f64`. A count no
/// screen could hold gives no offset; [`place`] never asks for one.
fn cascade_offset(steps: usize) -> f64 {
    u32::try_from(steps)
        .ok()
        .and_then(|n| n.checked_mul(DOWNLOAD_WINDOW_CASCADE_STEP))
        .map_or(0.0, f64::from)
}

/// How many whole cascade steps fit in `length` logical pixels: none when it is negative or
/// not a number.
fn whole_steps(length: f64) -> usize {
    f64_to_u64((length / f64::from(DOWNLOAD_WINDOW_CASCADE_STEP)).floor())
        .map_or(0, saturating_cast)
}

/// The download windows, kept by the app.
#[derive(Default)]
pub struct DownloadWindows(Mutex<Registry>);

impl DownloadWindows {
    fn with<T>(&self, change: impl FnOnce(&mut Registry) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// What is open now, for tests.
    pub fn snapshot(&self) -> Vec<OpenWindow> {
        self.with(|r| r.windows().to_vec())
    }

    /// The questions waiting for room to open their window, oldest first, for tests.
    pub fn waiting(&self) -> Vec<QuestionId> {
        self.with(|r| r.waiting().collect())
    }
}

/// What a download window is told when it acts on anything but what it shows.
pub const NOT_ITS_OWN: &str =
    "This window acts only on its own download. Use the main window for the others.";

/// What a download window is told when it changes a setting other than its own two.
pub const NOT_ITS_SETTING: &str =
    "This window changes only its own settings. Use Settings in the main window for the others.";

/// Whether the window `label` may act on `target`: the main window on anything, a download
/// window only on the question or download it shows now (D43).
pub fn may_act_on<R: Runtime>(app: &AppHandle<R>, label: &str, target: Shows) -> bool {
    !is_download_window(label)
        || app
            .try_state::<DownloadWindows>()
            .is_some_and(|windows| windows.with(|r| r.shows(label)) == Some(target))
}

/// Whether a download window may make the settings change `patch`: only the two settings it
/// shows, whether browser downloads start without asking and whether progress windows open
/// (D43).
#[must_use]
pub fn may_change(patch: &AppSettingsPatch) -> bool {
    let its_own = AppSettingsPatch {
        extension: patch
            .extension
            .as_ref()
            .map(|extension| ExtensionSettingsPatch {
                ask_before_download: extension.ask_before_download,
                ..ExtensionSettingsPatch::default()
            }),
        show_progress_windows: patch.show_progress_windows,
        ..AppSettingsPatch::default()
    };
    *patch == its_own
}

fn update<R: Runtime>(app: &AppHandle<R>, change: impl FnOnce(&mut Registry) -> Vec<Step>) {
    let Some(windows) = app.try_state::<DownloadWindows>() else {
        return;
    };
    let steps = windows.with(change);
    for step in steps {
        apply(app, step);
    }
}

/// The core asked question `id` (or moved it on): its window opens if it is not open.
pub fn ask<R: Runtime>(app: &AppHandle<R>, id: QuestionId) {
    update(app, |r| r.ask(id).into_iter().collect());
}

/// Question `id` closed in the core.
pub fn question_closed<R: Runtime>(app: &AppHandle<R>, id: QuestionId) {
    update(app, |r| {
        r.closed(id);
        Vec::new()
    });
}

/// Every question open in the core right now, oldest first.
#[must_use]
pub fn open_questions(manager: &Manager) -> Vec<QuestionId> {
    open_question_ids(
        &manager.pending_confirmations(),
        &manager.pending_media_choices(),
        &manager.pending_addon_requests(),
        &manager.media_being_read(),
    )
}

/// Opens windows for the questions already open in the core (at start, or after events were
/// missed), except those the person put aside. Does nothing while the manager is not open.
pub fn restore<R: Runtime>(app: &AppHandle<R>) {
    let Some(open) = app
        .try_state::<Core>()
        .and_then(|core| core.get().map(open_questions))
    else {
        return;
    };
    update(app, |r| r.restore(&open));
}

/// Brings every open question's window to the front, opening the closed ones again.
pub fn show_all<R: Runtime>(app: &AppHandle<R>, manager: &Manager) {
    let open = open_questions(manager);
    update(app, |r| r.show_all(&open));
}

/// The window `label` now shows download `id`.
pub fn became_download<R: Runtime>(app: &AppHandle<R>, label: &str, id: DownloadId) {
    update(app, |r| r.became_download(label, id));
}

/// The window `label` was destroyed.
pub fn window_gone<R: Runtime>(app: &AppHandle<R>, label: &str) {
    update(app, |r| r.gone(label));
}

/// Opens (or brings forward) the progress window of download `id`.
pub fn show_download<R: Runtime>(app: &AppHandle<R>, id: DownloadId) {
    update(app, |r| vec![r.show_download(id)]);
}

fn apply<R: Runtime>(app: &AppHandle<R>, step: Step) {
    let handle = app.clone();
    let run = app.run_on_main_thread(move || match step {
        Step::Open(window) => create(&handle, &window),
        Step::Focus(label) => {
            if let Some(window) = handle.get_webview_window(&label) {
                bring_forward(&window);
            }
        }
    });
    if let Err(e) = run {
        tracing::warn!("could not reach the window thread: {e}");
    }
}

/// The screen a new window goes on: the one under the pointer (where the browser most likely
/// is), else the main screen. `None` when the system does not say.
fn screen_area<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Area> {
    let monitor = window
        .cursor_position()
        .ok()
        .and_then(|at| window.monitor_from_point(at.x, at.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    Some(Area {
        x: f64::from(work.position.x) / scale,
        y: f64::from(work.position.y) / scale,
        width: f64::from(work.size.width) / scale,
        height: f64::from(work.size.height) / scale,
    })
}

fn create<R: Runtime>(app: &AppHandle<R>, open: &OpenWindow) {
    if let Some(existing) = app.get_webview_window(&open.label) {
        // Still closing from before: show it rather than fail on the label.
        warn_if_failed("focus a download window", existing.set_focus());
        return;
    }
    let title = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| crate::host::CHANNEL.name().to_owned());
    let builder =
        WebviewWindowBuilder::new(app, &open.label, WebviewUrl::App(open.shows.route().into()));
    let built = crate::appearance::frameless(app, builder)
        .title(title)
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .resizable(true)
        .maximizable(false)
        .always_on_top(true)
        .focused(true)
        .visible(false)
        .disable_drag_drop_handler()
        .build();
    let window = match built {
        Ok(window) => {
            crate::appearance::paint_background(&window);
            window
        }
        Err(e) => {
            tracing::error!("could not open a download window: {e}");
            update(app, |r| r.failed(&open.label));
            return;
        }
    };
    match screen_area(&window) {
        Some(area) => {
            let (x, y) = place(open.slot, area, WINDOW_SIZE);
            warn_if_failed(
                "place a download window",
                window.set_position(LogicalPosition::new(x, y)),
            );
        }
        None => warn_if_failed("center a download window", window.center()),
    }
    show_focused(&window);
    // On top until it has had the focus and lost it, then a normal window.
    let had_focus = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let this = window.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::Focused(focused) = event else {
            return;
        };
        if *focused {
            had_focus.store(true, Ordering::SeqCst);
            return;
        }
        if !had_focus.load(Ordering::SeqCst) || dropped.load(Ordering::SeqCst) {
            return;
        }
        // On Windows the window also reports losing the focus when its own page takes it, so
        // look again once things settle: only another window having it counts.
        let (this, dropped) = (this.clone(), dropped.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(FOCUS_SETTLE).await;
            if this.is_focused().unwrap_or(true) {
                return;
            }
            if !dropped.swap(true, Ordering::SeqCst) {
                drop_always_on_top(&this);
            }
        });
    });
}

/// Makes a download window a normal window again, behind the window that took the focus.
fn drop_always_on_top<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    warn_if_failed(
        "stop keeping a download window on top",
        window.set_always_on_top(false),
    );
    // Windows puts a window that stops being always on top above every normal window, so it
    // would jump over the browser the person just went back to. Put it right behind that one.
    #[cfg(windows)]
    {
        let this = window.clone();
        warn_if_failed(
            "put a download window behind the browser",
            window.run_on_main_thread(move || behind_foreground(&this)),
        );
    }
}

#[cfg(windows)]
fn behind_foreground<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    let Ok(handle) = window.hwnd() else {
        return;
    };
    let ours = handle.0 as HWND;
    // SAFETY: both calls take window handles by value and only fail on a stale one; nothing
    // is read or written through them.
    unsafe {
        let front = GetForegroundWindow();
        if front.is_null() || front == ours {
            return;
        }
        SetWindowPos(
            ours,
            front,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// How long after a lost focus a download window checks whether another window has it.
const FOCUS_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::PortablePath;

    fn q(id: u64) -> QuestionId {
        QuestionId(id)
    }

    #[test]
    fn a_window_takes_the_height_its_content_needs_within_the_screen() {
        // A short progress view: smaller than the opening size, rounded up to whole pixels.
        assert_eq!(fitted_height(431.2, Some(1040.0)), 432.0);
        // Never smaller than the smallest window.
        assert_eq!(fitted_height(120.0, Some(1040.0)), MIN_WINDOW_SIZE.1);
        // A long quality list, or a group opened: never taller than the most a window takes;
        // the body scrolls.
        assert_eq!(fitted_height(2000.0, Some(1440.0)), MAX_WINDOW_HEIGHT);
        // On a short screen, as tall as the screen allows, with a margin.
        assert_eq!(
            fitted_height(2000.0, Some(700.0)),
            700.0 - 2.0 * SCREEN_MARGIN
        );
        // Unknown screen: no taller than the opening size.
        assert_eq!(fitted_height(900.0, None), WINDOW_SIZE.1);
        // A tiny screen still gets the smallest window, and nonsense gives the smallest too.
        assert_eq!(fitted_height(700.0, Some(300.0)), MIN_WINDOW_SIZE.1);
        assert_eq!(fitted_height(f64::NAN, Some(1040.0)), MIN_WINDOW_SIZE.1);
        assert_eq!(
            fitted_height(f64::INFINITY, Some(1040.0)),
            MIN_WINDOW_SIZE.1
        );
    }

    fn opened(step: Option<Step>) -> OpenWindow {
        match step {
            Some(Step::Open(window)) => window,
            other => panic!("expected a window to open, got {other:?}"),
        }
    }

    #[test]
    fn a_window_is_known_by_what_it_shows_now() {
        let mut registry = Registry::default();
        let window = opened(registry.ask(q(4)));
        assert_eq!(registry.shows(&window.label), Some(Shows::Question(q(4))));
        registry.became_download(&window.label, DownloadId(9));
        assert_eq!(
            registry.shows(&window.label),
            Some(Shows::Download(DownloadId(9)))
        );
        registry.gone(&window.label);
        assert_eq!(registry.shows(&window.label), None);
        assert_eq!(registry.shows("main"), None);
    }

    #[test]
    fn a_download_window_changes_only_its_own_two_settings() {
        let start_without_asking = AppSettingsPatch {
            extension: Some(ExtensionSettingsPatch {
                ask_before_download: Some(false),
                ..ExtensionSettingsPatch::default()
            }),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(&start_without_asking));
        let close_when_started = AppSettingsPatch {
            show_progress_windows: Some(false),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(&close_when_started));
        assert!(may_change(&AppSettingsPatch::default()));

        let elsewhere = AppSettingsPatch {
            default_folder: Some(PortablePath(r"C:\Users\someone\Startup".to_owned())),
            show_progress_windows: Some(false),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&elsewhere));
        let more_of_the_extension = AppSettingsPatch {
            extension: Some(ExtensionSettingsPatch {
                ask_before_download: Some(false),
                capture_enabled: Some(true),
                ..ExtensionSettingsPatch::default()
            }),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&more_of_the_extension));
        let start_with_system = AppSettingsPatch {
            start_with_system: Some(true),
            ..AppSettingsPatch::default()
        };
        assert!(!may_change(&start_with_system));
    }

    #[test]
    fn labels_never_mix_questions_and_downloads() {
        assert_eq!(Shows::Question(q(7)).label(), "download-q7");
        assert_eq!(Shows::Download(DownloadId(7)).label(), "download-d7");
        assert!(is_download_window("download-q7"));
        assert!(!is_download_window("main"));
        assert_eq!(Shows::Question(q(7)).route(), "download.html?question=7");
        assert_eq!(
            Shows::Download(DownloadId(3)).route(),
            "download.html?download=3"
        );
    }

    /// Tauri gives a window permissions by its label: without this capability a download
    /// window could not listen to events or pick a folder.
    #[test]
    fn the_capability_covers_every_download_window() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/download-windows.json")).unwrap();
        let windows = capability["windows"].as_array().unwrap();
        assert_eq!(windows, &[serde_json::json!(format!("{LABEL_PREFIX}*"))]);
        let permissions = capability["permissions"].to_string();
        assert!(permissions.contains("core:event:allow-listen"));
        assert!(permissions.contains("dialog:allow-open"));
    }

    #[test]
    fn at_most_five_questions_ask_at_once_and_the_rest_wait_their_turn() {
        let mut r = Registry::default();
        let slots: Vec<usize> = (1..=5).map(|id| opened(r.ask(q(id))).slot).collect();
        assert_eq!(slots, [0, 1, 2, 3, 4]);
        assert_eq!(r.ask(q(6)), None);
        assert_eq!(r.ask(q(7)), None);
        assert_eq!(r.waiting().collect::<Vec<_>>(), [q(6), q(7)]);
        // Asking again (the video was read) opens nothing twice.
        assert_eq!(r.ask(q(3)), None);
        assert_eq!(r.ask(q(6)), None);

        // A window that starts showing its download makes room; it keeps its place.
        let steps = r.became_download("download-q2", DownloadId(40));
        let [Step::Open(next)] = steps.as_slice() else {
            panic!("{steps:?}")
        };
        assert_eq!((next.shows, next.slot), (Shows::Question(q(6)), 5));
        assert!(r.windows().iter().any(|w| w.label == "download-q2"
            && w.shows == Shows::Download(DownloadId(40))
            && w.slot == 1));

        // A closed window makes room too, and its slot is used again.
        let steps = r.gone("download-q1");
        let [Step::Open(next)] = steps.as_slice() else {
            panic!("{steps:?}")
        };
        assert_eq!((next.shows, next.slot), (Shows::Question(q(7)), 0));
    }

    #[test]
    fn a_question_closed_while_waiting_never_opens() {
        let mut r = Registry::default();
        for id in 1..=6 {
            r.ask(q(id));
        }
        r.closed(q(6));
        assert_eq!(r.gone("download-q1"), []);
    }

    #[test]
    fn a_window_closed_by_the_person_stays_closed_until_asked_for() {
        let mut r = Registry::default();
        opened(r.ask(q(1)));
        opened(r.ask(q(2)));
        assert_eq!(r.gone("download-q1"), []);
        // At start or after missed events, a put-aside question does not pop up again, and
        // one still open is not opened twice.
        assert_eq!(r.restore(&[q(1), q(2)]), []);
        // The person asks for the waiting questions: the open one comes forward, the closed
        // one opens again.
        let steps = r.show_all(&[q(1), q(2)]);
        assert_eq!(steps.len(), 2);
        assert!(steps.contains(&Step::Focus("download-q2".into())));
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, Step::Open(w) if w.shows == Shows::Question(q(1))))
        );
        // The core moving a put-aside question on (its quality list is ready) opens it.
        assert_eq!(r.gone("download-q1"), []);
        opened(r.ask(q(1)));
    }

    #[test]
    fn restore_opens_only_what_is_open_in_the_core_and_has_no_window() {
        let mut r = Registry::default();
        opened(r.ask(q(3)));
        let steps = r.restore(&[q(2), q(3), q(4)]);
        let shown: Vec<Shows> = steps
            .iter()
            .map(|s| match s {
                Step::Open(w) => w.shows,
                Step::Focus(l) => panic!("focused {l}"),
            })
            .collect();
        assert_eq!(shown, [Shows::Question(q(2)), Shows::Question(q(4))]);
        // Answered questions are not in the core's lists, so nothing opens for them.
        assert_eq!(r.restore(&[q(2), q(3), q(4)]), []);
    }

    #[test]
    fn a_progress_window_is_opened_once_and_is_not_limited() {
        let mut r = Registry::default();
        for id in 1..=5 {
            r.ask(q(id));
        }
        let Step::Open(window) = r.show_download(DownloadId(9)) else {
            panic!("expected a new window")
        };
        assert_eq!(window.label, "download-d9");
        assert_eq!(window.slot, 5);
        assert_eq!(
            r.show_download(DownloadId(9)),
            Step::Focus("download-d9".into())
        );
        // A question window that became download 9's is the one shown.
        r.became_download("download-q1", DownloadId(10));
        assert_eq!(
            r.show_download(DownloadId(10)),
            Step::Focus("download-q1".into())
        );
        // A window that could not be made is forgotten without being put aside.
        r.failed("download-q2");
        assert_eq!(r.restore(&[q(2)]).len(), 1);
    }

    const SCREEN: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };

    #[test]
    fn each_window_sits_one_step_down_and_right_of_the_last() {
        let first = place(0, SCREEN, WINDOW_SIZE);
        let second = place(1, SCREEN, WINDOW_SIZE);
        let step = f64::from(DOWNLOAD_WINDOW_CASCADE_STEP);
        assert_eq!(second.0 - first.0, step);
        assert_eq!(second.1 - first.1, step);
        // The first five spread around the middle of the screen.
        let fifth = place(4, SCREEN, WINDOW_SIZE);
        let middle = (
            (first.0 + fifth.0 + WINDOW_SIZE.0) / 2.0,
            (first.1 + fifth.1 + WINDOW_SIZE.1) / 2.0,
        );
        assert_eq!(middle, (960.0, 520.0));
    }

    #[test]
    fn windows_stay_on_their_screen() {
        let second_screen = Area {
            x: 1920.0,
            y: -200.0,
            width: 1280.0,
            height: 680.0,
        };
        for slot in 0..40 {
            let (x, y) = place(slot, second_screen, WINDOW_SIZE);
            assert!(x >= second_screen.x && y >= second_screen.y, "slot {slot}");
            assert!(
                x + WINDOW_SIZE.0 <= second_screen.x + second_screen.width,
                "slot {slot}"
            );
            assert!(
                y + WINDOW_SIZE.1 <= second_screen.y + second_screen.height,
                "slot {slot}"
            );
        }
        // Slots past the room start the cascade again.
        assert_eq!(
            place(0, second_screen, WINDOW_SIZE),
            place(3, second_screen, WINDOW_SIZE)
        );
        // A screen smaller than the window: its top left corner.
        let tiny = Area {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 300.0,
        };
        assert_eq!(place(0, tiny, WINDOW_SIZE), (10.0, 20.0));
        assert_eq!(place(7, tiny, WINDOW_SIZE), (10.0, 20.0));
    }
}
