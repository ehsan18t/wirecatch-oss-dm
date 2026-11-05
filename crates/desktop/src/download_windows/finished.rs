//! The download complete window (D52.6): one small window at the bottom right of the main
//! screen, listing what finished or failed, newest first, until the person closes it or the last
//! of it leaves the download list. It opens without taking the keyboard. "Use notifications
//! instead" switches the setting in one click.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, WindowOptions, div, point,
    px, size,
};
use wirecatch_protocol::app::{
    AppSettingsPatch, DownloadStatus, DownloadSummary, FixAction, NotifyOnFinish, UiEvent,
};
use wirecatch_protocol::desktop::FinishedQueue;
use wirecatch_shell::api;
use wirecatch_shell::download_windows::Shows;
use wirecatch_shell::finished::{FinishedBook, queue_header};
use wirecatch_shell::teller::Tell;

use crate::backend::act;
use crate::main_window::fixes::run_from_download_window;
use crate::ui::small_title_bar::small_title_bar;
use crate::ui::{Role, Typed as _};
use crate::{files, text};

const WIDTH: f32 = 380.;
const HEIGHT: f32 = 420.;
/// Space between the window and the screen's edges.
const MARGIN: f32 = 16.;

/// The list and its window, while open.
#[derive(Default)]
struct Finished {
    book: FinishedBook,
    window: Option<(AnyWindowHandle, WeakEntity<FinishedView>)>,
}

impl Global for Finished {}

/// Adds what ended to the window, opening it if it is closed. A download whose own progress
/// window is open is left out: that window shows it finished. So is one no longer in the list:
/// this comes on its own channel, so its removal may have reached the list first, and no later
/// event would take it out of the window.
pub fn add(tell: Tell, cx: &mut App) {
    let (queue, rows) = match tell {
        Tell::Finished(rows) | Tell::Failed(rows) => (None, rows),
        Tell::Queue(report) => (
            Some((report.queue, report.name)),
            report.completed.into_iter().chain(report.failed).collect(),
        ),
    };
    let listed = &cx.global::<super::Windows>().library.read(cx).downloads;
    let rows: Vec<DownloadSummary> = rows
        .into_iter()
        .filter(|row| listed.find(row.id).is_some())
        .filter(|row| !super::shows(Shows::Download(row.id), cx))
        .collect();
    // Counted from what the window lists, so the heading agrees with the rows under it.
    let queue = queue.map(|(id, name)| queue_header(id, &name, &rows));
    if !cx.default_global::<Finished>().book.add(queue, &rows) {
        return;
    }
    match cx.global::<Finished>().window.clone() {
        Some((_, view)) if view.upgrade().is_some() => {
            _ = view.update(cx, |_, cx| cx.notify());
        }
        _ => open(cx),
    }
}

/// Brings the listed downloads up to date after missed events.
pub fn reload(rows: &[DownloadSummary], cx: &mut App) {
    cx.default_global::<Finished>().book.reload(rows);
    show_what_is_left(cx);
}

/// Keeps the listed downloads in step with the core's events.
pub fn follow(event: &UiEvent, cx: &mut App) {
    cx.default_global::<Finished>().book.follow(event);
    show_what_is_left(cx);
}

/// Redraws the window, or closes it once nothing is left to list.
fn show_what_is_left(cx: &mut App) {
    let finished = cx.global::<Finished>();
    let empty = finished.book.list().groups.is_empty();
    let Some((handle, view)) = finished.window.clone() else {
        return;
    };
    if empty {
        _ = handle.update(cx, |_, window, cx| crate::closing::close(window, cx));
    } else {
        _ = view.update(cx, |_, cx| cx.notify());
    }
}

fn open(cx: &mut App) {
    let area = cx.primary_display().map_or_else(
        || Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.))),
        |display| display.visible_bounds(),
    );
    let origin = point(
        area.origin.x + area.size.width - px(WIDTH + MARGIN),
        area.origin.y + area.size.height - px(HEIGHT + MARGIN),
    );
    let options = WindowOptions {
        focus: false,
        ..crate::popup_window_options(Bounds::new(origin, size(px(WIDTH), px(HEIGHT))))
    };
    let library = cx.global::<super::Windows>().library.clone();
    let built = gpui_kit::open_window(options, cx, |window, cx| {
        crate::closing::on_close(window, cx, |_, cx| {
            cx.set_global(Finished::default());
            true
        });
        cx.new(|cx| {
            cx.observe(&library, |_, _, cx| cx.notify()).detach();
            FinishedView
        })
    });
    match built {
        Ok((handle, view)) => cx.global_mut::<Finished>().window = Some((handle, view.downgrade())),
        Err(e) => tracing::error!("could not open the download complete window: {e}"),
    }
}

/// The window's content, read from the list on each draw.
pub struct FinishedView;

impl Render for FinishedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let groups = cx.global::<Finished>().book.list().groups.clone();
        let alone = matches!(groups.as_slice(), [group] if group.downloads.len() == 1);
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_role(Role::Body, cx)
            .child(small_title_bar(
                div()
                    .text_role(Role::Strong, cx)
                    .child("Downloads finished"),
                window,
                cx,
            ))
            .child(
                v_flex()
                    .id("finished-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .gap_3()
                    .children(groups.iter().map(|group| {
                        v_flex()
                            .gap_1()
                            .when_some(group.queue.as_ref(), |list, queue| {
                                list.child(
                                    div().text_role(Role::Label, cx).child(queue_heading(queue)),
                                )
                            })
                            .children(
                                group
                                    .downloads
                                    .iter()
                                    .map(|row| finished_row(row, alone, cx)),
                            )
                    })),
            )
            .child(foot(cx))
    }
}

/// What the foot says once finished downloads go somewhere other than this window.
fn moved_note(notify: NotifyOnFinish) -> Option<&'static str> {
    match notify {
        NotifyOnFinish::Window => None,
        NotifyOnFinish::Notification => Some("Finished downloads now show as notifications."),
        NotifyOnFinish::Browser => Some("Finished downloads now show in your browser."),
        NotifyOnFinish::Nothing => Some("Finished downloads now show only in the download list."),
    }
}

fn set_notify(notify: NotifyOnFinish, window: &Window, cx: &mut App) {
    act(window.window_handle(), cx, move |manager| {
        api::change_settings(
            manager,
            AppSettingsPatch {
                notify_on_finish: Some(notify),
                ..AppSettingsPatch::default()
            },
        )
    });
}

/// The switch to notifications, or what it did with a way back. Read from the settings, so a
/// refused change or one made in Settings shows too.
fn foot(cx: &App) -> AnyElement {
    let library = cx.global::<super::Windows>().library.read(cx);
    let moved = library
        .settings
        .as_ref()
        .and_then(|settings| moved_note(settings.notify_on_finish));
    match moved {
        Some(note) => h_flex()
            .p_3()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_role(Role::Meta, cx)
                    .child(note),
            )
            .child(
                Button::new("undo-notifications")
                    .ghost()
                    .label("Undo")
                    .on_click(|_, window, cx| set_notify(NotifyOnFinish::Window, window, cx)),
            )
            .into_any_element(),
        None => h_flex()
            .p_3()
            .justify_end()
            .child(
                Button::new("use-notifications")
                    .ghost()
                    .label("Use notifications instead")
                    .on_click(|_, window, cx| set_notify(NotifyOnFinish::Notification, window, cx)),
            )
            .into_any_element(),
    }
}

/// A queue's summary heading: its name, with how many finished and failed.
fn queue_heading(queue: &FinishedQueue) -> String {
    let mut said = vec![format!("{} finished", queue.completed)];
    if queue.failed > 0 {
        said.push(format!("{} failed", queue.failed));
    }
    format!("{}: {}", queue.name, said.join(", "))
}

/// One download that ended: its name, how it ended, and what can be done with it. Alone in the
/// window, Open is its main button.
fn finished_row(row: &DownloadSummary, alone: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let failed = row.status == DownloadStatus::Failed;
    let said = if failed {
        row.error.as_ref().map_or_else(
            || "Failed".to_owned(),
            |error| text::first_sentence(&error.message).to_owned(),
        )
    } else {
        text::status_short(row)
    };
    h_flex()
        .gap_2()
        .py_1()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_role(Role::Strong, cx)
                        .child(row.file_name.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_role(Role::Meta, cx)
                        .when(failed, |line| line.text_color(theme.danger))
                        .child(said),
                ),
        )
        .children(row_buttons(row, alone))
        .into_any_element()
}

fn row_buttons(row: &DownloadSummary, alone: bool) -> Vec<Button> {
    let id = row.id;
    let named = |name: &str| ElementId::NamedInteger(name.to_owned().into(), id.0.unsigned_abs());
    match row.status {
        DownloadStatus::Completed => vec![
            Button::new(named("finished-folder"))
                .ghost()
                .label("Show in folder")
                .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            Button::new(named("finished-open"))
                .with_variant(if alone {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                })
                .label("Open")
                .on_click(move |_, window, cx| files::open(id, window, cx)),
        ],
        DownloadStatus::Failed => {
            let fixes = super::fixes_offered(row);
            let main = fixes.last().copied();
            fixes
                .into_iter()
                .map(|fix| {
                    let name = if fix == FixAction::Retry {
                        "finished-retry"
                    } else {
                        "finished-fix"
                    };
                    let shown = Button::new(named(name));
                    if Some(fix) == main {
                        shown
                    } else {
                        shown.ghost()
                    }
                    .label(text::fix_label(fix))
                    .on_click(move |_, window, cx| {
                        run_from_download_window(id, fix, window, cx);
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use wirecatch_protocol::DownloadId;
    use wirecatch_test_support::downloads::download;

    use super::*;
    use crate::library::Library;

    fn failed(id: i64) -> DownloadSummary {
        DownloadSummary {
            status: DownloadStatus::Failed,
            ..download(id)
        }
    }

    /// The download windows over a library listing `rows`.
    fn windows_over(rows: Vec<DownloadSummary>, cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            let library = cx.new(|_| {
                let mut library = Library::empty();
                library.downloads.replace(rows);
                library
            });
            super::super::init(&library, cx);
        });
    }

    #[gpui_kit::test]
    fn a_download_gone_from_the_list_opens_no_window(cx: &mut TestAppContext) {
        windows_over(Vec::new(), cx);
        cx.update(|cx| add(Tell::Failed(vec![failed(1)]), cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0);
    }

    #[gpui_kit::test]
    fn the_window_cannot_be_resized(cx: &mut TestAppContext) {
        windows_over(vec![failed(1)], cx);
        cx.update(|cx| add(Tell::Failed(vec![failed(1)]), cx));
        cx.run_until_parked();
        let resizable = cx.windows()[0]
            .update(cx, |_, window, _| window.is_resizable())
            .unwrap();
        assert!(!resizable);
    }

    #[gpui_kit::test]
    fn the_window_closes_once_every_row_is_removed(cx: &mut TestAppContext) {
        windows_over(vec![failed(1), failed(2)], cx);
        cx.update(|cx| add(Tell::Failed(vec![failed(1), failed(2)]), cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 1);

        let removed = |id| UiEvent::DownloadsRemoved(vec![DownloadId(id)]);
        cx.update(|cx| follow(&removed(1), cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 1, "one row is left");

        cx.update(|cx| follow(&removed(2), cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0);
    }

    #[gpui_kit::test]
    fn the_window_closes_when_missed_events_removed_every_row(cx: &mut TestAppContext) {
        windows_over(vec![failed(1)], cx);
        cx.update(|cx| add(Tell::Failed(vec![failed(1)]), cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 1);

        cx.update(|cx| reload(&[], cx));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0);
    }

    #[test]
    fn the_foot_says_where_finished_downloads_went() {
        assert_eq!(moved_note(NotifyOnFinish::Window), None);
        for elsewhere in [
            NotifyOnFinish::Notification,
            NotifyOnFinish::Browser,
            NotifyOnFinish::Nothing,
        ] {
            assert!(moved_note(elsewhere).is_some());
        }
    }
}
