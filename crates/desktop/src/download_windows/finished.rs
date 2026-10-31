//! The download complete window (D52.6): one small window at the bottom right of the main
//! screen, listing what finished or failed, newest first, until the person closes it. It opens
//! without taking the keyboard. "Use notifications instead" switches the setting in one click.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, WindowBounds, WindowOptions,
    div, point, px, size,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    AppSettingsPatch, DownloadStatus, DownloadSummary, NotifyOnFinish, UiEvent,
};
use wirecatch_protocol::desktop::FinishedQueue;
use wirecatch_shell::api;
use wirecatch_shell::download_windows::Shows;
use wirecatch_shell::finished::{FinishedBook, queue_header};
use wirecatch_shell::teller::Tell;

use crate::backend::act;
use crate::files;
use crate::ui::small_title_bar::small_title_bar;
use crate::ui::{Role, Typed as _};

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
/// window is open is left out: that window shows it finished.
pub fn add(tell: Tell, cx: &mut App) {
    let (queue, rows) = match tell {
        Tell::Finished(rows) | Tell::Failed(rows) => (None, rows),
        Tell::Queue(report) => (
            Some((report.queue, report.name)),
            report.completed.into_iter().chain(report.failed).collect(),
        ),
    };
    let rows: Vec<DownloadSummary> = rows
        .into_iter()
        .filter(|row| !super::shows(Shows::Download(row.id), cx))
        .collect();
    // Counted from what the window lists, so the heading agrees with the rows under it.
    let queue = queue.map(|(id, name)| queue_header(id, &name, &rows));
    if !cx.has_global::<Finished>() {
        cx.set_global(Finished::default());
    }
    if !cx.global_mut::<Finished>().book.add(queue, &rows) {
        return;
    }
    match cx.global::<Finished>().window.clone() {
        Some((_, view)) if view.upgrade().is_some() => {
            _ = view.update(cx, |_, cx| cx.notify());
        }
        _ => open(cx),
    }
}

/// Keeps the listed downloads in step with the core's events.
pub fn follow(event: &UiEvent, cx: &mut App) {
    if !cx.has_global::<Finished>() {
        return;
    }
    cx.global_mut::<Finished>().book.follow(event);
    if let Some((_, view)) = cx.global::<Finished>().window.clone() {
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
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            origin,
            size(px(WIDTH), px(HEIGHT)),
        ))),
        focus: false,
        ..crate::window_options()
    };
    let built = gpui_kit::open_window(options, cx, |window, cx| {
        crate::closing::on_close(window, cx, |_, cx| {
            if cx.has_global::<Finished>() {
                *cx.global_mut::<Finished>() = Finished::default();
            }
            true
        });
        cx.new(|_| FinishedView)
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
        let groups = cx
            .try_global::<Finished>()
            .map(|finished| finished.book.list().groups.clone())
            .unwrap_or_default();
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
            .child(
                h_flex().p_3().justify_end().child(
                    Button::new("use-notifications")
                        .ghost()
                        .label("Use notifications instead")
                        .on_click(|_, window, cx| {
                            act(window.window_handle(), cx, |manager| {
                                api::change_settings(
                                    manager,
                                    AppSettingsPatch {
                                        notify_on_finish: Some(NotifyOnFinish::Notification),
                                        ..AppSettingsPatch::default()
                                    },
                                )
                            });
                        }),
                ),
            )
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
    let id = row.id;
    let theme = cx.theme();
    let failed = row.status == DownloadStatus::Failed;
    let said = if failed {
        row.error.as_ref().map_or_else(
            || "Failed".to_owned(),
            |error| crate::text::first_sentence(&error.message).to_owned(),
        )
    } else {
        crate::text::status_short(row)
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
        .children(row_buttons(id, row.status, alone))
        .into_any_element()
}

fn row_buttons(id: DownloadId, status: DownloadStatus, alone: bool) -> Vec<Button> {
    let named = |name: &str| ElementId::NamedInteger(name.to_owned().into(), id.0.unsigned_abs());
    match status {
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
        DownloadStatus::Failed => vec![
            Button::new(named("finished-retry"))
                .label("Try again")
                .on_click(move |_, window, cx| {
                    act(window.window_handle(), cx, move |m| api::retry(m, id));
                }),
        ],
        _ => Vec::new(),
    }
}
