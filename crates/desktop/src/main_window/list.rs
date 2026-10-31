//! The download list: one line per download, grouped by status in the order [`Group::ALL`] gives,
//! each group under a heading that folds it. A row says what matters in its state and nothing
//! else: a running download its bar, speed and time left; a finished one its size and when.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas,
    div, px, uniform_list,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction, PER_MILLE_DONE};
use wirecatch_shell::api;

use super::drag::{self, DraggedRows};
use super::{Group, MainWindow, View, fixes, row_menu};
use crate::backend::act;
use crate::ui::{FileTile, Role, Typed as _, tokens};
use crate::{files, text};

/// Every line is this tall, headings included, so the list can lay out only what shows.
const LINE_HEIGHT: f32 = 56.;
/// The place of a download in its queue's line.
const PLACE_WIDTH: f32 = 20.;
/// The figures at a row's end, and the buttons that take their place on hover.
const END_WIDTH: f32 = 160.;

/// One line of the list.
#[derive(Clone, Copy)]
enum Line {
    Heading(Group, usize),
    /// A download by its index in the library, with its place in its queue's line.
    Row(usize, Option<usize>),
}

/// What a heading's button does to every download.
#[derive(Clone, Copy)]
enum GroupCommand {
    PauseAll,
    ResumeAll,
}

impl MainWindow {
    pub(super) fn list(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
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
                        Line::Row(index, place) => this.row(index, place, cx),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .pb_4()
        .into_any_element()
    }

    /// The rows the list shows, by their index in the library, in its order.
    pub(super) fn shown_rows(&self, cx: &App) -> Vec<usize> {
        self.lines(cx)
            .into_iter()
            .filter_map(|line| match line {
                Line::Row(index, _) => Some(index),
                Line::Heading(..) => None,
            })
            .collect()
    }

    /// Where download `id` is among the list's lines, if it shows.
    pub(super) fn line_of(&self, id: DownloadId, cx: &App) -> Option<usize> {
        let downloads = &self.library.read(cx).downloads;
        self.lines(cx)
            .iter()
            .position(|line| matches!(line, Line::Row(index, _) if downloads[*index].id == id))
    }

    /// The downloads of a queue's own view that are still to run or running, in the order they
    /// run, by their index in the library; empty in other views.
    pub(super) fn queue_line(&self, cx: &App) -> Vec<usize> {
        let View::Queue(queue) = self.view else {
            return Vec::new();
        };
        self.library
            .read(cx)
            .downloads
            .iter()
            .enumerate()
            .filter(|(_, row)| row.queue == queue && !Group::of(row.status).is_over())
            .map(|(index, _)| index)
            .collect()
    }

    /// The list's lines: the view's downloads that hold the search in their name or link. A queue's
    /// view lists its line first, numbered in the order it runs, then what is over by group; other
    /// views group everything, each group under its heading. A search shows everything it finds,
    /// folded or not.
    fn lines(&self, cx: &App) -> Vec<Line> {
        let downloads = &self.library.read(cx).downloads;
        let needle = self.needle(cx);
        let found = |row: &DownloadSummary| {
            needle.is_empty()
                || row.file_name.to_lowercase().contains(&needle)
                || row.url.to_lowercase().contains(&needle)
        };
        let mut lines: Vec<Line> = self
            .queue_line(cx)
            .into_iter()
            .enumerate()
            .filter(|(_, index)| found(&downloads[*index]))
            .map(|(place, index)| Line::Row(index, Some(place + 1)))
            .collect();
        let in_queue = matches!(self.view, View::Queue(_));
        for group in Group::ALL {
            if in_queue && !group.is_over() {
                continue;
            }
            let mut members: Vec<usize> = downloads
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    self.view.shows(row) && Group::of(row.status) == group && found(row)
                })
                .map(|(index, _)| index)
                .collect();
            self.sort.apply(
                group,
                &mut members,
                downloads,
                &mut self.frozen.borrow_mut(),
            );
            if members.is_empty() {
                continue;
            }
            lines.push(Line::Heading(group, members.len()));
            if needle.is_empty() && self.folded.contains(&group) {
                continue;
            }
            lines.extend(members.into_iter().map(|index| Line::Row(index, None)));
        }
        lines
    }

    /// The search, as the list compares it.
    pub(super) fn needle(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    /// The button a heading offers for every download: Pause all over what runs, or over what
    /// waits when nothing runs, and Resume all over what is paused.
    fn group_command(&self, group: Group, cx: &App) -> Option<(&'static str, GroupCommand)> {
        let running = self
            .library
            .read(cx)
            .downloads
            .iter()
            .any(|row| Group::of(row.status) == Group::Downloading);
        match group {
            Group::Downloading => Some(("Pause all", GroupCommand::PauseAll)),
            Group::UpNext if !running => Some(("Pause all", GroupCommand::PauseAll)),
            Group::Paused => Some(("Resume all", GroupCommand::ResumeAll)),
            _ => None,
        }
    }

    fn heading(&self, group: Group, shown: usize, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = cx.theme();
        let folded = self.folded.contains(&group);
        let command = self.group_command(group, cx);
        h_flex()
            .w_full()
            .h(px(LINE_HEIGHT))
            .px_5()
            .items_end()
            .pb_1p5()
            .gap_2()
            .text_role(Role::Label, cx)
            .child(
                h_flex()
                    .id(ElementId::Name(SharedString::new(format!(
                        "fold-{group:?}"
                    ))))
                    .flex_1()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|label| label.text_color(theme.foreground))
                    .child(
                        Icon::new(if folded {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .with_size(tokens::ICON_SMALL),
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
                    })),
            )
            .when_some(command, |line, (label, command)| {
                line.child(
                    Button::new(ElementId::Name(SharedString::new(format!("all-{group:?}"))))
                        .xsmall()
                        .ghost()
                        .label(label)
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let work = match command {
                                GroupCommand::PauseAll => api::pause_all,
                                GroupCommand::ResumeAll => api::resume_all,
                            };
                            act(window.window_handle(), cx, work);
                        })),
                )
            })
            .into_any_element()
    }

    fn row(&self, index: usize, place: Option<usize>, cx: &mut Context<'_, Self>) -> AnyElement {
        let Some(row) = self.library.read(cx).downloads.get(index).cloned() else {
            return div().h(px(LINE_HEIGHT)).into_any_element();
        };
        let theme = cx.theme();
        let selected = self.selected.contains(&row.id);
        let tone = status_tone(&row, cx);
        let id = row.id;
        let shows_actions = selected || self.hovered == Some(id);
        let fix = (row.status == DownloadStatus::Failed)
            .then(|| fixes::offered(&row))
            .flatten();
        let main = cx.entity().downgrade();
        let cursor_bounds = self.cursor_bounds.clone();
        h_flex()
            .id(row_id("row", row.id))
            .w_full()
            .h(px(LINE_HEIGHT))
            .px_5()
            .gap_3()
            .border_l_2()
            .border_color(theme.transparent)
            .when(selected, |line| {
                line.bg(theme.list_active)
                    .border_color(theme.list_active_border)
            })
            .when(!selected, |line| {
                line.hover(|line| line.bg(theme.list_hover))
            })
            .when_some(place, |line, place| line.child(place_cell(place, cx)))
            .child(FileTile::new(row.category).tone(tone))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_role(Role::Strong, cx)
                            .child(row.file_name.clone()),
                    )
                    .child(under_name(&row, tone, cx)),
            )
            .child(if shows_actions {
                row_actions(&row, cx).into_any_element()
            } else if let Some(fix) = fix {
                fix_button(id, fix).into_any_element()
            } else {
                figures(&row, tone, cx).into_any_element()
            })
            .when(self.cursor == Some(id), |line| {
                line.child(marker(cursor_bounds))
            })
            .on_drag(
                self.dragged_rows(id, &row.file_name),
                |dragged, _, _, cx| gpui_kit::AppContext::new(cx, |_| dragged.clone()),
            )
            .drag_over::<DraggedRows>(|style, _, _, cx| drag::drop_target(style, cx))
            .on_drop(cx.listener(move |this, dragged: &DraggedRows, window, cx| {
                this.drop_rows_on(id, dragged, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| this.right_click_row(id, cx)),
            )
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
                    this.click_row(id, event.modifiers(), window, cx);
                }),
            )
            .context_menu(move |menu, _, cx| row_menu::build(menu, &main, id, cx))
            .into_any_element()
    }
}

/// Under the name: the bar while work is under way or paused, else why it failed, why it waits,
/// or where it came from.
fn under_name(row: &DownloadSummary, tone: Hsla, cx: &App) -> AnyElement {
    if matches!(
        row.status,
        DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing
            | DownloadStatus::Paused
    ) {
        let per_mille = text::per_mille(row);
        return h_flex()
            .child(
                div().flex_1().child(
                    Progress::new(row_id("progress", row.id))
                        .color(tone)
                        .loading(per_mille.is_none())
                        .value(per_mille.map_or(0., |done| {
                            f32::from(done) * 100. / f32::from(PER_MILLE_DONE)
                        })),
                ),
            )
            .into_any_element();
    }
    let said = if let Some(error) = &row.error {
        text::first_sentence(&error.message).to_owned()
    } else if row.status == DownloadStatus::Queued || text::shown_wait(row).is_some() {
        text::status_short(row)
    } else {
        text::host(&row.url).to_owned()
    };
    div()
        .truncate()
        .text_role(Role::Meta, cx)
        .when(row.status == DownloadStatus::Failed, |line| {
            line.text_color(cx.theme().danger)
        })
        .child(said)
        .into_any_element()
}

/// The row's end: what matters most in its state, over what comes next in the state's colour.
fn figures(row: &DownloadSummary, tone: Hsla, cx: &App) -> impl IntoElement {
    let with_percent = |said: String| match text::percent(row) {
        Some(percent) if said.is_empty() => percent,
        Some(percent) => format!("{percent} · {said}"),
        None => said,
    };
    let next = match row.status {
        DownloadStatus::Downloading if row.speed > 0 => with_percent(
            row.eta_secs
                .map(|left| format!("{} left", text::eta(left)))
                .unwrap_or_default(),
        ),
        DownloadStatus::Downloading | DownloadStatus::Connecting => "Starting".to_owned(),
        DownloadStatus::Processing => with_percent("Finishing".to_owned()),
        DownloadStatus::Queued => "Up next".to_owned(),
        DownloadStatus::Paused => with_percent("Paused".to_owned()),
        DownloadStatus::Failed => "Failed".to_owned(),
        DownloadStatus::Cancelled => "Cancelled".to_owned(),
        DownloadStatus::Completed => row.completed_at.map(text::ago).unwrap_or_default(),
    };
    let first = if row.status == DownloadStatus::Downloading && row.speed > 0 {
        text::speed(row.speed)
    } else {
        text::size(row)
    };
    let quiet = matches!(
        row.status,
        DownloadStatus::Completed | DownloadStatus::Queued | DownloadStatus::Cancelled
    );
    v_flex()
        .w(px(END_WIDTH))
        .flex_shrink_0()
        .items_end()
        .gap_0p5()
        .when(!first.is_empty(), |end| {
            end.child(div().truncate().text_role(Role::Body, cx).child(first))
        })
        .child(
            div()
                .truncate()
                .text_role(Role::Meta, cx)
                .when(!quiet, |line| line.text_color(tone))
                .child(next),
        )
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
            Self::Pause => act(window.window_handle(), cx, move |manager| {
                api::pause(manager, id)
            }),
            Self::Resume => act(window.window_handle(), cx, move |manager| {
                api::resume(manager, id)
            }),
            Self::Retry => act(window.window_handle(), cx, move |manager| {
                api::retry(manager, id)
            }),
            Self::Open => files::open(id, window, cx),
            Self::ShowInFolder => files::show_in_folder(id, window, cx),
        }
    }
}

/// A download's place in its queue's line, before its tile.
fn place_cell(place: usize, cx: &App) -> impl IntoElement {
    div()
        .w(px(PLACE_WIDTH))
        .flex_shrink_0()
        .text_right()
        .text_role(Role::Meta, cx)
        .child(place.to_string())
}

/// Records where the cursor's row is drawn, for the menu the keyboard opens.
fn marker(
    bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
) -> impl IntoElement {
    canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, (), _, _| {})
        .absolute()
        .size_full()
}

/// A failed row's fix, shown where its figures would be.
fn fix_button(id: DownloadId, fix: FixAction) -> impl IntoElement {
    h_flex()
        .w(px(END_WIDTH))
        .flex_shrink_0()
        .justify_end()
        .child(
            Button::new(row_id("fix", id))
                .small()
                .outline()
                .label(text::fix_label(fix))
                .on_click(move |_, window, cx| fixes::run(id, fix, window, cx)),
        )
}

/// The buttons a row offers on hover, then its details; its menu opens on a right click.
fn row_actions(row: &DownloadSummary, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    let id = row.id;
    h_flex()
        .w(px(END_WIDTH))
        .flex_shrink_0()
        .gap_1()
        .justify_end()
        .children(
            RowAction::offered(row.status)
                .iter()
                .map(|(action, label, icon)| {
                    let (action, label) = (*action, *label);
                    Button::new(ElementId::NamedInteger(label.into(), id.0.unsigned_abs()))
                        .small()
                        .ghost()
                        .icon(icon.clone())
                        .tooltip(label)
                        .on_click(cx.listener(move |_, _, window, cx| action.run(id, window, cx)))
                }),
        )
        .child(
            Button::new(row_id("details", id))
                .small()
                .ghost()
                .icon(IconName::Info)
                .tooltip("Details")
                .on_click(cx.listener(move |this, _, window, cx| {
                    super::details::open(this.library.clone(), id, window, cx);
                })),
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

/// A row's element id; download ids are positive.
fn row_id(part: &'static str, id: DownloadId) -> ElementId {
    ElementId::NamedInteger(part.into(), id.0.unsigned_abs())
}
