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

use super::{Group, MainWindow, View, details};
use crate::backend::Backend;
use crate::components::layout::PageHeader;
use crate::{files, text};

/// Every line is this tall, headings included, so the list can lay out only what shows.
const LINE_HEIGHT: f32 = 44.;
const PROGRESS_WIDTH: f32 = 168.;
const SIZE_WIDTH: f32 = 136.;
const SPEED_WIDTH: f32 = 96.;
const LEFT_WIDTH: f32 = 84.;
const ACTIONS_WIDTH: f32 = 96.;

/// Space a column takes beside its neighbours.
const GAP: f32 = 16.;
/// The row's padding on both sides.
const ROW_PADDING: f32 = 40.;
/// The least the name keeps before a figure column folds under it.
const NAME_MIN_WIDTH: f32 = 220.;

/// Which figure columns show; in a narrow list they fold under the name, Left first, then Speed,
/// then Size.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Columns {
    size: bool,
    speed: bool,
    left: bool,
}

impl Columns {
    pub(super) const ALL: Self = Self {
        size: true,
        speed: true,
        left: true,
    };

    /// The columns a list `width` wide has room for.
    fn fitting(width: f32) -> Self {
        let mut needed = ROW_PADDING + NAME_MIN_WIDTH + PROGRESS_WIDTH + ACTIONS_WIDTH + 3. * GAP;
        let mut fits = |column: f32| {
            needed += column + GAP;
            needed <= width
        };
        let size = fits(SIZE_WIDTH);
        let speed = size && fits(SPEED_WIDTH);
        let left = speed && fits(LEFT_WIDTH);
        Self { size, speed, left }
    }
}

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
        PageHeader::new(self.title(cx), summary)
            .action(
                Button::new("new-download")
                    .small()
                    .primary()
                    .label("New download")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_download(&[], window, cx)),
                    ),
            )
            .when(offers_all, |header| {
                header.action(if any_paused {
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
        let needle = self.needle(cx);
        if !needle.is_empty() {
            let found = shown
                .iter()
                .filter(|row| {
                    row.file_name.to_lowercase().contains(&needle)
                        || row.url.to_lowercase().contains(&needle)
                })
                .count();
            return format!(
                "{} matching “{}”",
                plural(found, "download"),
                self.search.read(cx).value().trim()
            );
        }
        match (running, waiting) {
            (0, 0) => plural(shown.len(), "download"),
            (0, waiting) => format!("Nothing downloading · {waiting} up next"),
            (running, 0) => format!("{running} downloading"),
            (running, waiting) => format!("{running} downloading · {waiting} up next"),
        }
    }

    pub(super) fn list(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> AnyElement {
        self.columns =
            Columns::fitting(f32::from(window.viewport_size().width) - super::sidebar::WIDTH);
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

    /// The list's lines: the view's downloads that hold the search in their name or link, by
    /// group. One group alone has no heading; a search shows everything it finds, folded or not.
    fn lines(&self, cx: &App) -> Vec<Line> {
        let downloads = &self.library.read(cx).downloads;
        let needle = self.needle(cx);
        let groups: Vec<(Group, Vec<usize>)> = Group::ALL
            .into_iter()
            .map(|group| {
                let members = downloads
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| {
                        self.view.shows(row)
                            && Group::of(row.status) == group
                            && (needle.is_empty()
                                || row.file_name.to_lowercase().contains(&needle)
                                || row.url.to_lowercase().contains(&needle))
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                (group, members)
            })
            .filter(|(_, members)| !members.is_empty())
            .collect();
        let headed = groups.len() > 1;
        let mut lines = Vec::new();
        for (group, members) in groups {
            if headed {
                lines.push(Line::Heading(group, members.len()));
                if needle.is_empty() && self.folded.contains(&group) {
                    continue;
                }
            }
            lines.extend(members.into_iter().map(Line::Row));
        }
        lines
    }

    /// The search, as the list compares it.
    pub(super) fn needle(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
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
        let shows_actions = selected || self.hovered == Some(id);
        h_flex()
            .id(row_id("row", row.id))
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
            .child(name_cell(&row, self.columns, cx))
            .child(progress_cell(&row, tone, cx))
            .when(self.columns.size, |line| {
                line.child(figure(SIZE_WIDTH, text::size(&row), cx))
            })
            .when(self.columns.speed, |line| {
                line.child(figure(SPEED_WIDTH, speed_said(&row), cx))
            })
            .when(self.columns.left, |line| {
                line.child(figure(LEFT_WIDTH, left_said(&row), cx))
            })
            .when(shows_actions, |line| line.child(row_actions(&row, cx)))
            .when(!shows_actions, |line| {
                line.child(div().w(px(ACTIONS_WIDTH)).flex_shrink_0())
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered = Some(id);
                } else if this.hovered == Some(id) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                    if event.modifiers().secondary(cx) {
                        if !this.selected.remove(&id) {
                            this.selected.insert(id);
                        }
                    } else {
                        this.selected.clear();
                        this.selected.insert(id);
                        details::open(this.library.clone(), id, window, cx);
                    }
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}

/// The name, with the line under it.
fn name_cell(row: &DownloadSummary, columns: Columns, cx: &App) -> impl IntoElement {
    let mut under = vec![under_name(row)];
    if !columns.size {
        under.push(text::size(row));
    }
    if !columns.speed {
        under.push(speed_said(row));
    }
    if !columns.left {
        let left = left_said(row);
        if !left.is_empty() {
            under.push(format!("{left} left"));
        }
    }
    under.retain(|said| !said.is_empty());
    let theme = cx.theme();
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
                .child(under.join(" · ")),
        )
}

fn speed_said(row: &DownloadSummary) -> String {
    match row.status {
        DownloadStatus::Connecting => "Connecting".to_owned(),
        DownloadStatus::Downloading if row.speed > 0 => text::speed(row.speed),
        _ => String::new(),
    }
}

fn left_said(row: &DownloadSummary) -> String {
    match (row.status, row.eta_secs) {
        (DownloadStatus::Downloading, Some(left)) => text::eta(left),
        _ => String::new(),
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

/// What a row's hover buttons do.
#[derive(Clone, Copy)]
enum RowAction {
    Pause,
    Resume,
    Retry,
    Open,
    ShowInFolder,
}

impl RowAction {
    /// The buttons a download in `status` offers on hover.
    fn offered(status: DownloadStatus) -> &'static [(Self, &'static str, IconName)] {
        match status {
            DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing => &[(Self::Pause, "Pause", IconName::Pause)],
            DownloadStatus::Paused => &[(Self::Resume, "Resume", IconName::Play)],
            DownloadStatus::Failed => &[(Self::Retry, "Try again", IconName::RotateCw)],
            DownloadStatus::Cancelled => &[(Self::Retry, "Start over", IconName::RotateCw)],
            DownloadStatus::Completed => &[
                (Self::Open, "Open", IconName::ExternalLink),
                (Self::ShowInFolder, "Show in folder", IconName::FolderOpen),
            ],
        }
    }

    fn run(self, id: DownloadId, window: &mut Window, cx: &mut Context<'_, MainWindow>) {
        match self {
            Self::Pause => act(window, cx, move |manager| api::pause(manager, id)),
            Self::Resume => act(window, cx, move |manager| api::resume(manager, id)),
            Self::Retry => act(window, cx, move |manager| api::retry(manager, id)),
            Self::Open => files::open(id, window, cx),
            Self::ShowInFolder => files::show_in_folder(id, window, cx),
        }
    }
}

/// The buttons a row offers on hover.
fn row_actions(row: &DownloadSummary, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    let id = row.id;
    h_flex()
        .w(px(ACTIONS_WIDTH))
        .flex_shrink_0()
        .gap_1()
        .justify_end()
        .children(
            RowAction::offered(row.status)
                .iter()
                .map(|(action, label, icon)| {
                    let (action, label) = (*action, *label);
                    Button::new(ElementId::NamedInteger(label.into(), id.0.unsigned_abs()))
                        .xsmall()
                        .ghost()
                        .icon(icon.clone())
                        .tooltip(label)
                        .on_click(cx.listener(move |_, _, window, cx| action.run(id, window, cx)))
                }),
        )
}

/// The colour a status is drawn in: the accent for data that moves, the warning colour for a
/// pause, danger for a failure, quiet for the rest.
pub(super) fn status_tone(row: &DownloadSummary, cx: &App) -> Hsla {
    let theme = cx.theme();
    match row.status {
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Processing => {
            theme.primary
        }
        DownloadStatus::Paused => theme.warning,
        DownloadStatus::Failed => theme.danger,
        DownloadStatus::Completed | DownloadStatus::Queued | DownloadStatus::Cancelled => {
            theme.muted_foreground
        }
    }
}

/// Runs a command on the manager; a refusal shows as a notification in `window`.
pub(super) fn act<T: 'static>(
    window: &mut Window,
    cx: &mut Context<'_, T>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figure_columns_fold_left_first_then_speed_then_size() {
        assert!(Columns::fitting(2000.) == Columns::ALL);
        let shown = |width| {
            let c = Columns::fitting(width);
            (c.size, c.speed, c.left)
        };
        assert_eq!(shown(900.), (true, true, false));
        assert_eq!(shown(800.), (true, false, false));
        assert_eq!(shown(700.), (false, false, false));
    }
}
