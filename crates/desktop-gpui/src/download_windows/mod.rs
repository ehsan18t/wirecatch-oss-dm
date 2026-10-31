//! A small window for each question the core asks about a browser download, and for a
//! download's progress (D23, D43): confirm a file, the video being read, pick a quality, install
//! the media add-on, a file already in the folder. At most five ask at once; the rest wait.
//! Which windows are open and where they go is `wirecatch_shell::download_windows`; this opens and
//! closes the GPUI windows it says.

pub mod finished;
mod view;

use std::collections::HashMap;

use gpui_kit::component::TitleBar;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Global, WeakEntity, WindowBounds,
    WindowOptions, point, px, size,
};
use wirecatch_protocol::app::{Repeated, UiEvent};
use wirecatch_protocol::{DownloadId, QuestionId};
use wirecatch_shell::download_windows::{
    Area, MIN_WINDOW_SIZE, OpenWindow, Registry, Shows, Step, WINDOW_SIZE, place,
};

use crate::library::Library;
use view::DownloadWindow;

/// The download windows, kept for the life of the app.
struct Windows {
    library: Entity<Library>,
    registry: Registry,
    /// Each open window by its label, with its view.
    open: HashMap<String, (AnyWindowHandle, WeakEntity<DownloadWindow>)>,
}

impl Global for Windows {}

/// Keeps the windows of `library`'s questions. Call once at start.
pub fn init(library: &Entity<Library>, cx: &mut App) {
    cx.set_global(Windows {
        library: library.clone(),
        registry: Registry::default(),
        open: HashMap::new(),
    });
}

/// What `event` asks of the windows: a question opens its window (or its open window shows its
/// next step), a closed question leaves, a link sent again brings its window back (D89).
pub fn follow(event: &UiEvent, cx: &mut App) {
    match event {
        UiEvent::ConfirmationRequested(capture) => ask(capture.id, cx),
        UiEvent::MediaAnalysing { id, .. } | UiEvent::AddonRequired { id, .. } => ask(*id, cx),
        UiEvent::MediaChoiceRequested(choice) => ask(choice.id, cx),
        UiEvent::FileExistsRequested(question) => ask(question.id, cx),
        UiEvent::ConfirmationClosed { id } => closed(*id, cx),
        UiEvent::CaptureRepeated(Repeated::Question(id)) => {
            let step = update(cx, |registry| registry.show_question(*id)).flatten();
            run(step.into_iter().collect(), cx);
        }
        UiEvent::CaptureRepeated(Repeated::Download(id)) => show_download(*id, cx),
        _ => {}
    }
}

/// Opens windows for the questions open in the core at start, oldest first.
pub fn restore(open: &[QuestionId], cx: &mut App) {
    let steps = update(cx, |registry| registry.restore(open)).unwrap_or_default();
    run(steps, cx);
}

/// Brings every open question's window up, opening the closed ones again.
pub fn show_all(open: &[QuestionId], cx: &mut App) {
    let steps = update(cx, |registry| registry.show_all(open)).unwrap_or_default();
    run(steps, cx);
}

/// Opens, or brings up, download `id`'s progress window.
pub fn show_download(id: DownloadId, cx: &mut App) {
    let step = update(cx, |registry| registry.show_download(id));
    run(step.into_iter().collect(), cx);
}

fn ask(id: QuestionId, cx: &mut App) {
    let step = update(cx, |registry| registry.ask(id)).flatten();
    match step {
        Some(step) => run(vec![step], cx),
        // Already open: it shows the question's next step.
        None => refresh(Shows::Question(id), cx),
    }
}

fn closed(id: QuestionId, cx: &mut App) {
    update(cx, |registry| registry.closed(id));
    refresh(Shows::Question(id), cx);
}

/// Window `label` now shows download `id`, its question answered there.
fn became_download(label: &str, id: DownloadId, cx: &mut App) {
    let steps = update(cx, |registry| registry.became_download(label, id)).unwrap_or_default();
    run(steps, cx);
}

/// Window `label` is gone; waiting questions may open.
fn gone(label: &str, cx: &mut App) {
    if cx.has_global::<Windows>() {
        cx.global_mut::<Windows>().open.remove(label);
    }
    let steps = update(cx, |registry| registry.gone(label)).unwrap_or_default();
    run(steps, cx);
}

/// Whether a download window shows `shows` now.
fn shows(shows: Shows, cx: &App) -> bool {
    cx.try_global::<Windows>()
        .is_some_and(|windows| windows.registry.window_showing(shows).is_some())
}

/// `change` applied to the registry; `None` before the windows are kept.
fn update<T>(cx: &mut App, change: impl FnOnce(&mut Registry) -> T) -> Option<T> {
    cx.has_global::<Windows>()
        .then(|| change(&mut cx.global_mut::<Windows>().registry))
}

/// Asks the window showing `shows`, if one is open, to read its question again.
fn refresh(shows: Shows, cx: &mut App) {
    let Some(windows) = cx.try_global::<Windows>() else {
        return;
    };
    let Some(window) = windows.registry.window_showing(shows) else {
        return;
    };
    let Some((_, view)) = windows.open.get(&window.label).cloned() else {
        return;
    };
    _ = view.update(cx, view::DownloadWindow::reload);
}

fn run(steps: Vec<Step>, cx: &mut App) {
    for step in steps {
        match step {
            Step::Open(window) => open(&window, cx),
            Step::Focus(label) => {
                let handle = cx
                    .try_global::<Windows>()
                    .and_then(|windows| windows.open.get(&label).map(|(handle, _)| *handle));
                if let Some(handle) = handle {
                    _ = handle.update(cx, |_, window, _| window.activate_window());
                }
            }
        }
    }
}

/// Opens the GPUI window for `opened`, in its cascade slot on the main screen.
fn open(opened: &OpenWindow, cx: &mut App) {
    let Some(library) = cx.try_global::<Windows>().map(|w| w.library.clone()) else {
        return;
    };
    let area = cx.primary_display().map_or(
        Area {
            x: 0.,
            y: 0.,
            width: 1280.,
            height: 800.,
        },
        |display| {
            let bounds = display.visible_bounds();
            Area {
                x: f64::from(f32::from(bounds.origin.x)),
                y: f64::from(f32::from(bounds.origin.y)),
                width: f64::from(f32::from(bounds.size.width)),
                height: f64::from(f32::from(bounds.size.height)),
            }
        },
    );
    let (x, y) = place(opened.slot, area, WINDOW_SIZE);
    let bounds = Bounds {
        origin: point(px(logical(x)), px(logical(y))),
        size: size(px(logical(WINDOW_SIZE.0)), px(logical(WINDOW_SIZE.1))),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(
            px(logical(MIN_WINDOW_SIZE.0)),
            px(logical(MIN_WINDOW_SIZE.1)),
        )),
        ..TitleBar::window_options()
    };
    let label = opened.label.clone();
    let shows = opened.shows;
    let built = gpui_kit::open_window(options, cx, |window, cx| {
        let closing = label.clone();
        window.on_window_should_close(cx, move |_, cx| {
            gone(&closing, cx);
            true
        });
        cx.new(|cx| DownloadWindow::new(library, label.clone(), shows, window, cx))
    });
    match built {
        Ok((handle, view)) => {
            if cx.has_global::<Windows>() {
                cx.global_mut::<Windows>()
                    .open
                    .insert(opened.label.clone(), (handle, view.downgrade()));
            }
        }
        Err(e) => {
            tracing::error!("could not open a download window: {e}");
            let steps = update(cx, |registry| registry.failed(&opened.label)).unwrap_or_default();
            run(steps, cx);
        }
    }
}

/// A logical size or place the registry works in, in GPUI's pixels: whole pixels, so exact.
fn logical(value: f64) -> f32 {
    wirecatch_engine::numeric::f64_to_u64(value.round())
        .and_then(|whole| u16::try_from(whole).ok())
        .map_or(0., f32::from)
}
