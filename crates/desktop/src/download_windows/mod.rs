//! A small window for each question the core asks about a browser download, and for a
//! download's progress (D23, D43): confirm a file, the video being read, pick a quality, install
//! the media add-on, a file already in the folder. At most five ask at once; the rest wait.
//! Which windows are open and where they go is `wirecatch_shell::download_windows`; this opens and
//! closes the GPUI windows it says.

pub mod finished;
mod media_form;
pub mod media_output;
mod proposed_name;
pub(crate) mod run_record;
mod view;
pub(crate) use view::{run_facts, run_panel};

use std::collections::HashMap;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Global, WeakEntity, Window, point, px,
    size,
};
use wirecatch_protocol::app::{
    DownloadStatus, DownloadSummary, FixAction, RETRY_FIXES, Repeated, UiEvent,
};
use wirecatch_protocol::{DownloadId, QuestionId};
use wirecatch_shell::download_windows::{
    Area, OpenWindow, Registry, Shows, Step, WINDOW_SIZE, place,
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
        UiEvent::MediaAnalysing { id, url } => {
            ask(*id, cx);
            if let Some(view) = view_showing(Shows::Question(*id), cx) {
                crate::closing::may_have_closed(
                    view.update(cx, |view, cx| view.reading_from(url, cx)),
                );
            }
        }
        UiEvent::AddonRequired { id, .. } => ask(*id, cx),
        UiEvent::MediaChoiceRequested(choice) => ask(choice.id, cx),
        UiEvent::FileExistsRequested(question) => ask(question.id, cx),
        UiEvent::FileTypeRequested(question) => ask(question.id, cx),
        UiEvent::ConfirmationClosed { id } => closed(*id, cx),
        UiEvent::CaptureRepeated(Repeated::Question(id)) => {
            let step = update(cx, |registry| registry.show_question(*id));
            run(step.into_iter().collect(), cx);
        }
        UiEvent::CaptureRepeated(Repeated::Download(id)) => show_download(*id, cx),
        _ => {}
    }
}

/// Opens windows for the questions open in the core at start, oldest first.
pub fn restore(open: &[QuestionId], cx: &mut App) {
    for id in update(cx, |registry| registry.closed_meanwhile(open)) {
        closed(id, cx);
    }
    let steps = update(cx, |registry| registry.restore(open));
    run(steps, cx);
    // Their question may have moved on during the events that were missed.
    for id in open {
        refresh(Shows::Question(*id), cx);
    }
}

/// Brings every open question's window up, opening the closed ones again.
pub fn show_all(open: &[QuestionId], cx: &mut App) {
    let steps = update(cx, |registry| registry.show_all(open));
    run(steps, cx);
}

/// Opens, or brings up, download `id`'s progress window.
pub fn show_download(id: DownloadId, cx: &mut App) {
    let step = update(cx, |registry| registry.show_download(id));
    run(vec![step], cx);
}

/// Asks before cancelling download `id`, as the list does (D23.4).
fn ask_to_cancel(id: DownloadId, window: &mut Window, cx: &mut App) {
    let library = cx.global::<Windows>().library.clone();
    crate::main_window::confirm::cancel(&library, &[id], &[], window, cx);
}

/// What a failed download's window offers it, the main one last: its fix, with Try again while
/// trying again can carry the fix out (`RETRY_FIXES`).
fn fixes_offered(row: &DownloadSummary) -> Vec<FixAction> {
    let fix = row.error.as_ref().and_then(|error| error.fix);
    match fix.filter(|_| row.status == DownloadStatus::Failed) {
        None | Some(FixAction::Retry) => vec![FixAction::Retry],
        Some(fix) if RETRY_FIXES.contains(&fix) => vec![fix, FixAction::Retry],
        Some(fix) => vec![fix],
    }
}

fn ask(id: QuestionId, cx: &mut App) {
    let step = update(cx, |registry| registry.ask(id));
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
    let steps = update(cx, |registry| registry.became_download(label, id));
    run(steps, cx);
}

/// Window `label` is gone; waiting questions may open.
fn gone(label: &str, cx: &mut App) {
    cx.global_mut::<Windows>().open.remove(label);
    let steps = update(cx, |registry| registry.gone(label));
    run(steps, cx);
}

/// Whether a download window shows `shows` now.
fn shows(shows: Shows, cx: &App) -> bool {
    cx.global::<Windows>()
        .registry
        .window_showing(shows)
        .is_some()
}

fn update<T>(cx: &mut App, change: impl FnOnce(&mut Registry) -> T) -> T {
    change(&mut cx.global_mut::<Windows>().registry)
}

/// Asks the window showing `shows`, if one is open, to read its question again.
fn refresh(shows: Shows, cx: &mut App) {
    if let Some(view) = view_showing(shows, cx) {
        crate::closing::may_have_closed(view.update(cx, view::DownloadWindow::reload));
    }
}

fn view_showing(shows: Shows, cx: &App) -> Option<WeakEntity<view::DownloadWindow>> {
    let windows = cx.global::<Windows>();
    let window = windows.registry.window_showing(shows)?;
    let (_, view) = windows.open.get(&window.label)?;
    Some(view.clone())
}

fn run(steps: Vec<Step>, cx: &mut App) {
    for step in steps {
        match step {
            Step::Open(window) => open(&window, cx),
            Step::Focus(label) => {
                let handle = cx
                    .global::<Windows>()
                    .open
                    .get(&label)
                    .map(|(handle, _)| *handle);
                if let Some(handle) = handle {
                    crate::closing::may_have_closed(
                        handle.update(cx, |_, window, _| window.activate_window()),
                    );
                }
            }
            Step::Close(label) => {
                let opened = cx.global_mut::<Windows>().open.remove(&label);
                if let Some((handle, _)) = opened {
                    crate::closing::may_have_closed(
                        handle.update(cx, |_, window, _| window.remove_window()),
                    );
                }
            }
        }
    }
}

/// Opens the GPUI window for `opened`, in its cascade slot on the main screen.
fn open(opened: &OpenWindow, cx: &mut App) {
    let library = cx.global::<Windows>().library.clone();
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
    let options = crate::popup_window_options(bounds);
    let label = opened.label.clone();
    let shows = opened.shows;
    let built = gpui_kit::open_window(options, cx, |window, cx| {
        let closing = label.clone();
        crate::closing::on_close(window, cx, move |_, cx| {
            gone(&closing, cx);
            true
        });
        cx.new(|cx| DownloadWindow::new(library, label.clone(), shows, window, cx))
    });
    match built {
        Ok((handle, view)) => {
            cx.global_mut::<Windows>()
                .open
                .insert(opened.label.clone(), (handle, view.downgrade()));
            // Opening alone leaves it behind the browser: Windows brings a background app's new
            // window forward only through activation.
            crate::closing::may_have_closed(
                handle.update(cx, |_, window, _| window.activate_window()),
            );
        }
        Err(e) => {
            tracing::error!("could not open a download window: {e}");
            let steps = update(cx, |registry| registry.failed(&opened.label));
            run(steps, cx);
        }
    }
}

/// A logical size or place the registry works in, in GPUI's pixels: whole pixels, so exact.
pub(crate) fn logical(value: f64) -> f32 {
    wirecatch_engine::numeric::f64_to_u64(value.round())
        .and_then(|whole| u16::try_from(whole).ok())
        .map_or(0., f32::from)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::WindowExt as _;
    use wirecatch_protocol::app::UserError;
    use wirecatch_test_support::downloads::download;

    use super::*;

    fn failed_with(fix: Option<FixAction>) -> DownloadSummary {
        DownloadSummary {
            status: DownloadStatus::Failed,
            error: Some(UserError {
                message: "It failed.".to_owned(),
                fix,
            }),
            ..download(1)
        }
    }

    #[test]
    fn a_failure_offers_its_fix_and_try_again_only_where_that_helps() {
        use FixAction::{ChangeQueue, ChooseFolder, FreeDiskSpace, KeepOriginal};
        use FixAction::{ProxySettings, RefreshLink, Retry};

        assert_eq!(fixes_offered(&failed_with(None)), [Retry]);
        assert_eq!(fixes_offered(&failed_with(Some(Retry))), [Retry]);
        assert_eq!(
            fixes_offered(&failed_with(Some(FreeDiskSpace))),
            [FreeDiskSpace, Retry]
        );
        for fix in [
            RefreshLink,
            ChooseFolder,
            KeepOriginal,
            ProxySettings,
            ChangeQueue,
        ] {
            assert_eq!(fixes_offered(&failed_with(Some(fix))), [fix]);
        }
    }

    #[test]
    fn a_cancelled_download_offers_try_again_whatever_its_last_error() {
        let cancelled = DownloadSummary {
            status: DownloadStatus::Cancelled,
            ..failed_with(Some(FixAction::RefreshLink))
        };
        assert_eq!(fixes_offered(&cancelled), [FixAction::Retry]);
    }

    #[gpui_kit::test]
    fn a_download_window_cannot_be_resized(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            let library = cx.new(|_| Library::empty());
            init(&library, cx);
            show_download(DownloadId(1), cx);
        });
        cx.run_until_parked();
        let resizable = cx.windows()[0]
            .update(cx, |_, window, _| window.is_resizable())
            .unwrap();
        assert!(!resizable);
    }

    #[gpui_kit::test]
    fn cancelling_from_a_download_window_asks_first(cx: &mut TestAppContext) {
        let id = DownloadId(1);
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            let library = cx.new(|_| {
                let mut library = Library::empty();
                library.downloads.replace(vec![DownloadSummary {
                    status: DownloadStatus::Downloading,
                    ..download(id.0)
                }]);
                library
            });
            init(&library, cx);
            show_download(id, cx);
        });
        cx.run_until_parked();
        let shown = cx.windows()[0];
        let asked = shown
            .update(cx, |_, window, cx| {
                ask_to_cancel(id, window, cx);
                window.has_active_dialog(cx)
            })
            .unwrap();
        assert!(asked);
    }
}
