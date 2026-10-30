//! The list header and the download list: one line per download, grouped by status in the order
//! [`Group::ALL`] gives, each group under a heading that folds it.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    px, uniform_list,
};
use wirecatch_core::Manager;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, PER_MILLE_DONE};
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api;

use super::{Group, MainWindow, View};
use crate::backend::Backend;
use crate::text;

/// Every line is this tall, headings included, so the list can lay out only what shows.
const LINE_HEIGHT: f32 = 44.;
const PROGRESS_WIDTH: f32 = 168.;
const SIZE_WIDTH: f32 = 136.;
const SPEED_WIDTH: f32 = 96.;
const LEFT_WIDTH: f32 = 84.;
const ACTIONS_WIDTH: f32 = 96.;

/// One line of the list.
#[derive(Clone, Copy)]
enum Line {
    Heading(Group, usize),
    Row(usize),
}

impl MainWindow {
    pub(super) fn header(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let summary = self.summary(cx);
        let library = self.library.read(cx);
        let any_paused = library
            .downloads
            .iter()
            .any(|row| row.status == DownloadStatus::Paused);
        let any_pausable = library
            .downloads
            .iter()
            .any(|row| matches!(Group::of(row.status), Group::Downloading | Group::UpNext));
        let offers_all = (any_paused || any_pausable)
            && !matches!(
                self.view,
                View::Group(Group::Finished | Group::Attention | Group::Cancelled)
            );
        let theme = cx.theme();
        h_flex()
            .px_5()
            .py_3()
            .gap_4()
            .border_b_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.title(cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(summary),
                    ),
            )
            .child(
                Button::new("new-download")
                    .small()
                    .primary()
                    .label("New download")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_download(&[], window, cx)),
                    ),
            )
            .when(offers_all, |header| {
                header.child(if any_paused {
                    Button::new("resume-all")
                        .small()
                        .outline()
                        .label("Resume all")
                        .on_click(cx.listener(|_, _, window, cx| act(window, cx, api::resume_all)))
                } else {
                    Button::new("pause-all")
                        .small()
                        .outline()
                        .label("Pause all")
                        .on_click(cx.listener(|_, _, window, cx| act(window, cx, api::pause_all)))
                })
            })
    }

    /// What the view holds: how many download and wait, or how many there are.
    fn summary(&self, cx: &App) -> String {
        let shown: Vec<_> = self
            .library
            .read(cx)
            .downloads
            .iter()
            .filter(|row| self.view.shows(row))
            .collect();
        let running = shown
            .iter()
            .filter(|row| Group::of(row.status) == Group::Downloading)
            .count();
        let waiting = shown
            .iter()
            .filter(|row| row.status == DownloadStatus::Queued)
            .count();
        match (running, waiting) {
            (0, 0) => plural(shown.len(), "download"),
            (0, waiting) => format!("Nothing downloading · {waiting} up next"),
            (running, 0) => format!("{running} downloading"),
            (running, waiting) => format!("{running} downloading · {waiting} up next"),
        }
    }

    pub(super) fn list(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> AnyElement {
        let lines = self.lines(cx);
        if lines.is_empty() {
            return self.empty(cx).into_any_element();
        }
        let count = lines.len();
        uniform_list(
            "downloads",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| lines.get(index).copied())
                    .map(|line| match line {
                        Line::Heading(group, shown) => this.heading(group, shown, cx),
                        Line::Row(index) => this.row(index, cx),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .into_any_element()
    }

    fn lines(&self, cx: &App) -> Vec<Line> {
        let downloads = &self.library.read(cx).downloads;
        let grouped = !matches!(self.view, View::Group(_));
        let mut lines = Vec::new();
        for group in Group::ALL {
            let members: Vec<usize> = downloads
                .iter()
                .enumerate()
                .filter(|(_, row)| self.view.shows(row) && Group::of(row.status) == group)
                .map(|(index, _)| index)
                .collect();
            if members.is_empty() {
                continue;
            }
            if grouped {
                lines.push(Line::Heading(group, members.len()));
                if self.folded.contains(&group) {
                    continue;
                }
            }
            lines.extend(members.into_iter().map(Line::Row));
        }
        lines
    }

    fn heading(&self, group: Group, shown: usize, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme();
        let folded = self.folded.contains(&group);
        h_flex()
            .id(ElementId::Name(SharedString::new(format!(
                "heading-{group:?}"
            ))))
            .w_full()
            .h(px(LINE_HEIGHT))
            .px_5()
            .items_end()
            .pb_1p5()
            .gap_2()
            .cursor_pointer()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(
                Icon::new(if folded {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall(),
            )
            .child(group.label())
            .child(
                div()
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(shown.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&group) {
                    this.folded.insert(group);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn row(&self, index: usize, cx: &mut Context<'_, Self>) -> AnyElement {
        let Some(row) = self.library.read(cx).downloads.get(index).cloned() else {
            return div().h(px(LINE_HEIGHT)).into_any_element();
        };
        let theme = cx.theme();
        let selected = self.selected.contains(&row.id);
        let tone = status_tone(&row, cx);
        let id = row.id;
        let under_name = under_name(&row);
        h_flex()
            .id(row_id("row", row.id))
            .group("row")
            .w_full()
            .h(px(LINE_HEIGHT))
            .px_5()
            .gap_4()
            .border_l_2()
            .border_color(theme.transparent)
            .when(selected, |line| {
                line.bg(theme.list_active)
                    .border_color(theme.list_active_border)
            })
            .when(!selected, |line| {
                line.hover(|line| line.bg(theme.list_hover))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(row.file_name.clone()))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(if row.status == DownloadStatus::Failed {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(under_name),
                    ),
            )
            .child(progress_cell(&row, tone, cx))
            .child(figure(SIZE_WIDTH, text::size(&row), cx))
            .child(figure(
                SPEED_WIDTH,
                match row.status {
                    DownloadStatus::Connecting => "Connecting".to_owned(),
                    DownloadStatus::Downloading if row.speed > 0 => text::speed(row.speed),
                    _ => String::new(),
                },
                cx,
            ))
            .child(figure(
                LEFT_WIDTH,
                match (row.status, row.eta_secs) {
                    (DownloadStatus::Downloading, Some(left)) => text::eta(left),
                    _ => String::new(),
                },
                cx,
            ))
            .child(row_actions(&row, cx))
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                    if event.modifiers().secondary(cx) {
                        if !this.selected.remove(&id) {
                            this.selected.insert(id);
                        }
                    } else {
                        this.selected.clear();
                        this.selected.insert(id);
                    }
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    fn empty(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (title, note) = match self.view {
            View::All => (
                "No downloads yet".to_owned(),
                "Copy a link, then press Ctrl+V here.".to_owned(),
            ),
            View::Group(Group::Attention) => {
                ("Nothing needs your attention".to_owned(), String::new())
            }
            View::Group(group) => (
                format!("Nothing {}", group.label().to_lowercase()),
                String::new(),
            ),
            View::Queue(_) => (format!("{} is empty", self.title(cx)), String::new()),
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(div().text_color(theme.muted_foreground).child(note))
    }
}

/// The line under a download's name: why it failed, where it comes from, or why it waits.
fn under_name(row: &DownloadSummary) -> String {
    if let Some(error) = &row.error {
        return text::first_sentence(&error.message).to_owned();
    }
    match row.status {
        DownloadStatus::Queued | DownloadStatus::Processing => text::status_short(row),
        _ if text::shown_wait(row).is_some() => text::status_short(row),
        _ => text::host(&row.url).to_owned(),
    }
}

/// The bar and what it means: the percentage while it runs, or the state it is in.
fn progress_cell(row: &DownloadSummary, tone: Hsla, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let per_mille = text::per_mille(row);
    let caption = match row.status {
        DownloadStatus::Downloading | DownloadStatus::Connecting => {
            text::percent(row).unwrap_or_default()
        }
        _ => text::status_short(row),
    };
    v_flex()
        .w(px(PROGRESS_WIDTH))
        .flex_shrink_0()
        .gap_1()
        .when(row.status != DownloadStatus::Cancelled, |cell| {
            cell.child(
                Progress::new(row_id("progress", row.id))
                    .color(tone)
                    .loading(per_mille.is_none())
                    .value(per_mille.map_or(0., |done| {
                        f32::from(done) * 100. / f32::from(PER_MILLE_DONE)
                    })),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(caption),
        )
}

fn figure(width: f32, value: String, cx: &App) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_shrink_0()
        .text_right()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .truncate()
        .child(value)
}

/// The buttons a row offers on hover: pause or resume, or try again.
fn row_actions(row: &DownloadSummary, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    let id = row.id;
    let action = match row.status {
        DownloadStatus::Queued
        | DownloadStatus::Connecting
        | DownloadStatus::Downloading
        | DownloadStatus::Processing => Some(("Pause", pause as fn(&Manager, DownloadId) -> _)),
        DownloadStatus::Paused => Some(("Resume", resume as fn(&Manager, DownloadId) -> _)),
        DownloadStatus::Failed => Some(("Try again", retry as fn(&Manager, DownloadId) -> _)),
        DownloadStatus::Cancelled => Some(("Start over", retry as fn(&Manager, DownloadId) -> _)),
        DownloadStatus::Completed => None,
    };
    h_flex()
        .w(px(ACTIONS_WIDTH))
        .flex_shrink_0()
        .justify_end()
        .invisible()
        .group_hover("row", gpui_kit::Styled::visible)
        .children(action.map(|(label, run)| {
            Button::new(row_id("act", id))
                .xsmall()
                .ghost()
                .label(label)
                .on_click(cx.listener(move |_, _, window, cx| {
                    act(window, cx, move |manager| run(manager, id));
                }))
        }))
}

fn pause(manager: &Manager, id: DownloadId) -> Result<(), ProtocolError> {
    api::pause(manager, id)
}

fn resume(manager: &Manager, id: DownloadId) -> Result<(), ProtocolError> {
    api::resume(manager, id)
}

fn retry(manager: &Manager, id: DownloadId) -> Result<(), ProtocolError> {
    api::retry(manager, id)
}

/// The colour a status is drawn in: the accent for data that moves, the warning colour for a
/// pause, danger for a failure, quiet for the rest.
fn status_tone(row: &DownloadSummary, cx: &App) -> Hsla {
    let theme = cx.theme();
    match row.status {
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Processing => {
            theme.primary
        }
        DownloadStatus::Paused => theme.warning,
        DownloadStatus::Failed => theme.danger,
        DownloadStatus::Completed => theme.success,
        DownloadStatus::Queued | DownloadStatus::Cancelled => theme.muted_foreground,
    }
}

/// Runs a command on the manager; a refusal shows as a notification in `window`.
fn act(
    window: &mut Window,
    cx: &mut Context<'_, MainWindow>,
    work: impl FnOnce(&Manager) -> Result<(), ProtocolError> + Send + 'static,
) {
    let done = Backend::run(cx, move |manager| async move { work(&manager) });
    cx.spawn_in(window, async move |_, cx| {
        let refused = match done.await {
            Ok(Ok(Ok(()))) => return,
            Ok(Ok(Err(error))) => error.message,
            Ok(Err(unopened)) => unopened.reason.message,
            Err(_) => "That did not work.".to_owned(),
        };
        _ = cx.update(|window, cx| window.push_notification(refused, cx));
    })
    .detach();
}

/// A row's element id; download ids are positive.
fn row_id(part: &'static str, id: DownloadId) -> ElementId {
    ElementId::NamedInteger(part.into(), id.0.unsigned_abs())
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
