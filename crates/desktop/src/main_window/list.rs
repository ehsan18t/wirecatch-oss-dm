//! The download list: one line per download, grouped by status in the order [`Group::ALL`] gives,
//! each group under a heading that folds it; finished downloads in queue order sit under the day
//! they finished. Every row shares one grid: a circle that picks it, its time to the second, its
//! file tile, then its name over the lines the View menu keeps (link, save folder), and one figure.

use chrono::NaiveDate;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, canvas, div, px, relative, uniform_list,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    DownloadStatus, DownloadSummary, FixAction, ListLines, PER_MILLE_DONE,
};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::api;

use super::drag::{self, DraggedRows};
use super::{Group, MainWindow, View, fixes, row_menu, sort};
use crate::backend::act;
use crate::ui::select_circle::{Picked, SelectCircle};
use crate::ui::{FileTile, Role, Typed as _, icon, tokens};
use crate::{files, text};

/// Space before the circle column and after the figure.
pub(super) const PAD: f32 = 12.;
/// The column of circles that pick rows and headings.
pub(super) const CIRCLE_WIDTH: f32 = 18.;
const TIME_WIDTH: f32 = 78.;
const TILE_WIDTH: f32 = 32.;
const GAP: f32 = 12.;
/// Every line's left border, which marks a selected row.
const BORDER: f32 = 2.;
/// Where a row's text starts, which its progress line follows.
const TEXT_START: f32 = BORDER + PAD + CIRCLE_WIDTH + GAP + TIME_WIDTH + GAP + TILE_WIDTH + GAP;
/// The figure at a row's end, and the buttons that take its place on hover.
const END_WIDTH: f32 = 112.;
const PROGRESS_HEIGHT: f32 = 2.;

/// One line of the list.
#[derive(Clone, Copy)]
enum Line {
    Heading(Group, usize),
    /// The day the rows under it finished on.
    Day(NaiveDate),
    /// A download by its index in the library, with its place in its queue's line, and whether
    /// a day heading over it already says its day.
    Row(usize, Option<usize>, bool),
}

/// What a heading's button does to every download.
#[derive(Clone, Copy)]
pub(super) enum GroupCommand {
    PauseAll,
    ResumeAll,
}

impl GroupCommand {
    pub(super) fn run(self, window: &Window, cx: &mut App) {
        let work = match self {
            Self::PauseAll => api::pause_all,
            Self::ResumeAll => api::resume_all,
        };
        act(window.window_handle(), cx, work);
    }
}

/// How tall every line is: the list lays out only what shows, so a line's height is fixed by
/// how many lines a row shows under its name.
fn line_height(shown: ListLines) -> f32 {
    match usize::from(shown.link) + usize::from(shown.folder) {
        0 => 36.,
        1 => 52.,
        _ => 68.,
    }
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
                let shown = this.list_lines(cx);
                range
                    .filter_map(|index| Some((index, *lines.get(index)?)))
                    .map(|(index, line)| match line {
                        Line::Heading(group, count) => this.heading(group, count, shown, cx),
                        Line::Day(day) => {
                            let rows = rows_under(&lines[index + 1..]);
                            this.day_heading(day, &rows, shown, cx)
                        }
                        Line::Row(row, place, dated) => this.row(row, place, dated, shown, cx),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .pb_4()
        .into_any_element()
    }

    /// The lines the settings keep under each row's name.
    pub(super) fn list_lines(&self, cx: &App) -> ListLines {
        self.library
            .read(cx)
            .settings
            .as_ref()
            .map(|settings| settings.list_lines)
            .unwrap_or_default()
    }

    /// The rows the list shows, by their index in the library, in its order.
    pub(super) fn shown_rows(&self, cx: &App) -> Vec<usize> {
        self.lines(cx)
            .into_iter()
            .filter_map(|line| match line {
                Line::Row(index, ..) => Some(index),
                Line::Heading(..) | Line::Day(_) => None,
            })
            .collect()
    }

    /// Where download `id` is among the list's lines, if it shows.
    pub(super) fn line_of(&self, id: DownloadId, cx: &App) -> Option<usize> {
        let downloads = &self.library.read(cx).downloads;
        self.lines(cx)
            .iter()
            .position(|line| matches!(line, Line::Row(index, ..) if downloads[*index].id == id))
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

    /// The view's downloads in `group` that the search finds, in the sort's order.
    fn members(&self, group: Group, cx: &App) -> Vec<usize> {
        let downloads = &self.library.read(cx).downloads;
        let needle = self.needle(cx);
        let mut members: Vec<usize> = downloads
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                self.view.shows(row) && Group::of(row.status) == group && found(row, &needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.sort.apply(
            group,
            &mut members,
            downloads,
            &mut self.frozen.borrow_mut(),
        );
        members
    }

    /// The list's lines: the view's downloads that hold the search in their name or link. A queue's
    /// view lists its line first, numbered in the order it runs, then what is over by group; All
    /// downloads groups everything, each group under its heading, and a status's own page lists its
    /// downloads with no heading. Finished downloads in queue order go under the day they finished
    /// instead. A search shows everything it finds, folded or not.
    fn lines(&self, cx: &App) -> Vec<Line> {
        let downloads = &self.library.read(cx).downloads;
        let needle = self.needle(cx);
        let mut lines: Vec<Line> = self
            .queue_line(cx)
            .into_iter()
            .enumerate()
            .filter(|(_, index)| found(&downloads[*index], &needle))
            .map(|(place, index)| Line::Row(index, Some(place + 1), false))
            .collect();
        let in_queue = matches!(self.view, View::Queue(_));
        for group in Group::ALL {
            if in_queue && !group.is_over() {
                continue;
            }
            let members = self.members(group, cx);
            if members.is_empty() {
                continue;
            }
            if group == Group::Finished && self.sort.key == sort::Key::QueueOrder {
                let mut last = None;
                for index in members {
                    let day = finished_day(&downloads[index]);
                    if day != last {
                        lines.extend(day.map(Line::Day));
                        last = day;
                    }
                    lines.push(Line::Row(index, None, day.is_some()));
                }
                continue;
            }
            if !matches!(self.view, View::Group(_)) {
                lines.push(Line::Heading(group, members.len()));
                if needle.is_empty() && self.folded.contains(&group) {
                    continue;
                }
            }
            lines.extend(
                members
                    .into_iter()
                    .map(|index| Line::Row(index, None, false)),
            );
        }
        lines
    }

    /// The search, as the list compares it.
    pub(super) fn needle(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    /// The button a heading, or a status's own page, offers for every download: Pause all over
    /// what runs, or over what waits when nothing runs, and Resume all over what is paused.
    pub(super) fn group_command(
        &self,
        group: Group,
        cx: &App,
    ) -> Option<(&'static str, GroupCommand)> {
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

    /// How much of `ids` is ticked; a row only clicked is not.
    pub(super) fn picked_of(&self, ids: &[DownloadId]) -> Picked {
        let ticked = if self.picked {
            ids.iter().filter(|id| self.selected.contains(id)).count()
        } else {
            0
        };
        match ticked {
            0 => Picked::None,
            n if n == ids.len() => Picked::All,
            _ => Picked::Some,
        }
    }

    fn ids_of(&self, rows: &[usize], cx: &App) -> Vec<DownloadId> {
        let downloads = &self.library.read(cx).downloads;
        rows.iter()
            .filter_map(|index| downloads.get(*index).map(|row| row.id))
            .collect()
    }

    /// A heading's circle, which ticks every download under it.
    fn heading_circle(
        &self,
        name: SharedString,
        ids: Vec<DownloadId>,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let picked = self.picked_of(&ids);
        circle_cell().child(
            SelectCircle::new(ElementId::Name(name), picked)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_rows(&ids, cx))),
        )
    }

    fn heading(
        &self,
        group: Group,
        count: usize,
        shown: ListLines,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let (quiet, loud) = (cx.theme().muted_foreground, cx.theme().foreground);
        let folded = self.folded.contains(&group);
        let command = self.group_command(group, cx);
        let ids = self.ids_of(&self.members(group, cx), cx);
        heading_line(shown)
            .child(self.heading_circle(format!("pick-{group:?}").into(), ids, cx))
            .child(
                h_flex()
                    .id(ElementId::Name(SharedString::new(format!(
                        "fold-{group:?}"
                    ))))
                    .flex_1()
                    .gap_2()
                    .cursor_pointer()
                    .child(div().text_role(Role::Strong, cx).child(group.label()))
                    .child(div().text_role(Role::Meta, cx).child(count.to_string()))
                    .child(
                        div()
                            .text_color(quiet)
                            .hover(|chevron| chevron.text_color(loud))
                            .child(
                                Icon::new(if folded {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .with_size(tokens::ICON_SMALL),
                            ),
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
                        .on_click(cx.listener(move |_, _, window, cx| command.run(window, cx))),
                )
            })
            .into_any_element()
    }

    /// A day over the downloads that finished on it, with how many and how much.
    fn day_heading(
        &self,
        day: NaiveDate,
        rows: &[usize],
        shown: ListLines,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let downloads = &self.library.read(cx).downloads;
        let bytes: u64 = rows
            .iter()
            .filter_map(|index| downloads.get(*index))
            .map(|row| row.total.unwrap_or(row.done))
            .sum();
        let files = match rows.len() {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        let ids = self.ids_of(rows, cx);
        heading_line(shown)
            .child(self.heading_circle(format!("pick-day-{day}").into(), ids, cx))
            .child(
                div()
                    .text_role(Role::Strong, cx)
                    .child(text::day_heading(day)),
            )
            .child(
                div()
                    .text_role(Role::Meta, cx)
                    .child(format!("{files}, {}", format_bytes(bytes))),
            )
            .into_any_element()
    }

    fn row(
        &self,
        index: usize,
        place: Option<usize>,
        dated: bool,
        shown: ListLines,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let height = line_height(shown);
        let Some(row) = self.library.read(cx).downloads.get(index).cloned() else {
            return div().h(px(height)).into_any_element();
        };
        let theme = cx.theme();
        let id = row.id;
        let selected = self.selected.contains(&id);
        let hovered = self.hovered == Some(id);
        let tone = status_tone(&row, cx);
        let compact = !shown.link && !shown.folder;
        let running = Group::of(row.status) == Group::Downloading;
        let shows_circle = !running && (hovered || (self.picked && !self.selected.is_empty()));
        let shows_actions = hovered || (selected && !self.picked);
        let main = cx.entity().downgrade();
        let cursor_bounds = self.cursor_bounds.clone();
        h_flex()
            .id(row_id("row", id))
            .relative()
            .w_full()
            .h(px(height))
            .pl(px(PAD))
            .pr(px(PAD))
            .gap(px(GAP))
            .border_l(px(BORDER))
            .border_color(theme.transparent)
            .when(selected, |line| {
                line.bg(theme.list_active)
                    .border_color(theme.list_active_border)
            })
            .when(!selected, |line| {
                line.hover(|line| line.bg(theme.list_hover))
            })
            .child(circle_cell().when(shows_circle, |cell| {
                cell.child(
                    SelectCircle::new(row_id("pick", id), self.picked_of(&[id]))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_rows(&[id], cx))),
                )
            }))
            .child(time_cell(&row, place, dated || compact, cx))
            .child(tile(&row, compact, tone))
            .child(text_column(&row, shown, tone, cx))
            .child(row_end(&row, shows_actions, tone, cx))
            .when_some(progress_line(&row, tone, cx), |line, progress| {
                line.child(progress)
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

/// The row's end: its buttons while it is hovered, a failed row's fix, else its figure.
fn row_end(
    row: &DownloadSummary,
    shows_actions: bool,
    tone: Hsla,
    cx: &mut Context<'_, MainWindow>,
) -> AnyElement {
    if shows_actions {
        return row_actions(row, cx).into_any_element();
    }
    let fix = (row.status == DownloadStatus::Failed)
        .then(|| fixes::offered(row))
        .flatten();
    match fix {
        Some(fix) => fix_button(row.id, fix).into_any_element(),
        None => figure(row, tone, cx).into_any_element(),
    }
}

/// The file's tile, or only its picture in a compact row.
fn tile(row: &DownloadSummary, compact: bool, tone: Hsla) -> AnyElement {
    if !compact {
        return FileTile::new(row.category).tone(tone).into_any_element();
    }
    div()
        .w(px(TILE_WIDTH))
        .flex_shrink_0()
        .flex()
        .justify_center()
        .text_color(tone)
        .child(Icon::new(icon::category(row.category)).with_size(tokens::ICON))
        .into_any_element()
}

/// Whether a row holds the search in its name or link.
fn found(row: &DownloadSummary, needle: &str) -> bool {
    needle.is_empty()
        || row.file_name.to_lowercase().contains(needle)
        || row.url.to_lowercase().contains(needle)
}

/// The rows at the start of `lines`, up to the next heading.
fn rows_under(lines: &[Line]) -> Vec<usize> {
    lines
        .iter()
        .map_while(|line| match line {
            Line::Row(index, ..) => Some(*index),
            Line::Heading(..) | Line::Day(_) => None,
        })
        .collect()
}

/// The day a finished download goes under: the day it finished, or was added when that is unknown.
fn finished_day(row: &DownloadSummary) -> Option<NaiveDate> {
    text::local_day(row.completed_at.unwrap_or(row.added_at))
}

/// A heading's line, in the rows' grid; its content sits at the bottom, over the rows it heads.
fn heading_line(shown: ListLines) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .h(px(line_height(shown)))
        .pl(px(PAD))
        .pr(px(PAD))
        .gap(px(GAP))
        .border_l(px(BORDER))
        .items_end()
        .pb_2()
}

/// The cell a circle sits in, kept when the circle is hidden so the columns stay put.
fn circle_cell() -> gpui_kit::Div {
    h_flex()
        .w(px(CIRCLE_WIDTH))
        .flex_shrink_0()
        .justify_center()
}

/// When a row happened, to the second, or the word for a download under way.
fn time_cell(row: &DownloadSummary, place: Option<usize>, dated: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let cell = v_flex().w(px(TIME_WIDTH)).flex_shrink_0().justify_center();
    let word = |said: String, color: Hsla| {
        div()
            .text_role(Role::Strong, cx)
            .text_color(color)
            .child(said)
            .into_any_element()
    };
    let shown = match (Group::of(row.status), place) {
        (Group::Downloading, _) => word("now".to_owned(), theme.primary),
        (Group::Paused, _) => word("paused".to_owned(), theme.warning),
        (_, Some(place)) => word(place.to_string(), theme.muted_foreground),
        (Group::UpNext, None) => word("queued".to_owned(), theme.muted_foreground),
        _ => {
            let at = row.completed_at.unwrap_or(row.added_at);
            let (clock, seconds) = text::clock_parts(at);
            let day = (!dated).then(|| text::local_day(at)).flatten();
            return cell
                .child(
                    h_flex()
                        .items_baseline()
                        .child(div().text_role(Role::Strong, cx).child(clock))
                        .child(div().text_role(Role::Caption, cx).child(seconds)),
                )
                .when_some(day, |cell, day| {
                    cell.child(
                        div()
                            .text_role(Role::Caption, cx)
                            .child(text::day_brief(day)),
                    )
                })
                .into_any_element();
        }
    };
    cell.child(shown).into_any_element()
}

/// The name over what the row says under it: its state while that matters, then the link and the
/// folder the View menu keeps, as many lines as the settings give.
fn text_column(
    row: &DownloadSummary,
    shown: ListLines,
    tone: Hsla,
    cx: &mut Context<'_, MainWindow>,
) -> impl IntoElement {
    let danger = cx.theme().danger;
    let failed = row.status == DownloadStatus::Failed;
    let room = usize::from(shown.link) + usize::from(shown.folder);
    let mut under: Vec<AnyElement> = Vec::new();
    under.extend(status_line(row, tone, cx));
    if shown.link {
        under.push(link_line(row, cx));
    }
    if shown.folder {
        under.push(folder_line(row, cx));
    }
    under.truncate(room);
    v_flex()
        .flex_1()
        .min_w_0()
        .items_start()
        .justify_center()
        .gap_0p5()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_role(Role::Strong, cx)
                        .child(row.file_name.clone()),
                )
                .when(failed && room == 0, |line| {
                    line.child(
                        div()
                            .flex_shrink_0()
                            .text_role(Role::Meta, cx)
                            .text_color(danger)
                            .child("Failed"),
                    )
                }),
        )
        .children(under)
}

/// What a row says of its state under its name, when its state is not plain finished.
fn status_line(row: &DownloadSummary, tone: Hsla, cx: &App) -> Option<AnyElement> {
    let theme = cx.theme();
    let line = || {
        h_flex()
            .max_w_full()
            .gap_3()
            .overflow_hidden()
            .text_role(Role::Meta, cx)
    };
    let said = match row.status {
        DownloadStatus::Completed | DownloadStatus::Cancelled => return None,
        DownloadStatus::Failed => {
            line()
                .text_color(theme.danger)
                .child(div().truncate().child(
                    row.error.as_ref().map_or_else(String::new, |error| {
                        text::first_sentence(&error.message).to_owned()
                    }),
                ))
        }
        DownloadStatus::Downloading if row.speed > 0 && text::shown_wait(row).is_none() => line()
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(tone)
                    .child(text::speed(row.speed)),
            )
            .when_some(row.eta_secs, |line, left| {
                line.child(
                    div()
                        .flex_shrink_0()
                        .child(format!("{} left", text::eta(left))),
                )
            })
            .child(div().truncate().child(text::size(row))),
        DownloadStatus::Paused => line().child(
            div()
                .truncate()
                .child(format!("Paused at {}", text::size(row))),
        ),
        _ => line().child(div().truncate().child(text::status_short(row))),
    };
    Some(said.into_any_element())
}

/// The full link, its host brighter; a click copies it, and hovering shows it whole.
fn link_line(row: &DownloadSummary, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let link = text::link_for_display(&row.url);
    let (scheme, rest) = link
        .split_once("://")
        .map_or(("", link.as_str()), |(scheme, rest)| (scheme, rest));
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let scheme = if scheme.is_empty() {
        String::new()
    } else {
        format!("{scheme}://")
    };
    let (host, path) = (rest[..host_end].to_owned(), rest[host_end..].to_owned());
    let copied = link.clone();
    h_flex()
        .id(row_id("link", row.id))
        .max_w_full()
        .overflow_hidden()
        .cursor_pointer()
        .text_role(Role::Meta, cx)
        .child(div().flex_shrink_0().child(scheme))
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.foreground.opacity(0.8))
                .child(host),
        )
        .child(div().min_w_0().truncate().child(path))
        .hover(|line| line.text_color(theme.foreground))
        .tooltip(move |window, cx| whole(link.clone(), "Click to copy the link", window, cx))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
            window.push_notification("Link copied.", cx);
        })
        .into_any_element()
}

/// The folder the file is saved in; a click opens it, and hovering shows it whole.
fn folder_line(row: &DownloadSummary, cx: &mut Context<'_, MainWindow>) -> AnyElement {
    let theme = cx.theme();
    let id = row.id;
    let folder = std::path::Path::new(&text::folder(&row.path))
        .parent()
        .map_or_else(String::new, |folder| folder.display().to_string());
    let shown = folder.clone();
    h_flex()
        .id(row_id("folder", id))
        .max_w_full()
        .gap_1()
        .overflow_hidden()
        .cursor_pointer()
        .text_role(Role::Caption, cx)
        .child(
            Icon::new(IconName::Folder)
                .with_size(px(12.))
                .flex_shrink_0(),
        )
        .child(div().min_w_0().truncate().child(shown))
        .hover(|line| line.text_color(theme.foreground))
        .tooltip(move |window, cx| whole(folder.clone(), "Click to open the folder", window, cx))
        .on_click(cx.listener(move |_, _, window, cx| {
            cx.stop_propagation();
            files::show_in_folder(id, window, cx);
        }))
        .into_any_element()
}

/// A popover with a line's whole text, wrapped, over what a click on it does.
fn whole(said: String, hint: &'static str, window: &mut Window, cx: &mut App) -> gpui_kit::AnyView {
    Tooltip::element(move |_, cx| {
        v_flex()
            .max_w(px(420.))
            .gap_1()
            .child(div().text_role(Role::Body, cx).child(said.clone()))
            .child(div().text_role(Role::Meta, cx).child(hint))
    })
    .build(window, cx)
}

/// The figure at a row's end: how far work under way is, a finished file's size, or the state
/// a download ended in.
fn figure(row: &DownloadSummary, tone: Hsla, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (said, color) = match row.status {
        DownloadStatus::Connecting
        | DownloadStatus::Downloading
        | DownloadStatus::Processing
        | DownloadStatus::Paused => (text::percent(row).unwrap_or_default(), tone),
        DownloadStatus::Failed => ("Failed".to_owned(), theme.danger),
        DownloadStatus::Cancelled => ("Cancelled".to_owned(), theme.muted_foreground),
        DownloadStatus::Completed | DownloadStatus::Queued => {
            (text::size(row), theme.muted_foreground)
        }
    };
    h_flex()
        .w(px(END_WIDTH))
        .flex_shrink_0()
        .justify_end()
        .child(
            div()
                .truncate()
                .text_role(Role::Body, cx)
                .text_color(color)
                .child(said),
        )
}

/// The thin line along a row's bottom while work is under way or paused, from its text on.
fn progress_line(row: &DownloadSummary, tone: Hsla, cx: &App) -> Option<impl IntoElement> {
    if !matches!(
        row.status,
        DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing
            | DownloadStatus::Paused
    ) {
        return None;
    }
    let done = text::per_mille(row).map_or(0., |done| f32::from(done) / f32::from(PER_MILLE_DONE));
    Some(
        div()
            .absolute()
            .bottom_0()
            .left(px(TEXT_START))
            .right(px(PAD))
            .h(px(PROGRESS_HEIGHT))
            .rounded_full()
            .bg(cx.theme().muted_foreground.opacity(0.15))
            .child(div().h_full().w(relative(done)).rounded_full().bg(tone)),
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

/// Records where the cursor's row is drawn, for the menu the keyboard opens.
fn marker(
    bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
) -> impl IntoElement {
    canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, (), _, _| {})
        .absolute()
        .size_full()
}

/// A failed row's fix, shown where its figure would be.
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
                .on_click(move |_, window, cx| {
                    // The row under it would open the details too.
                    cx.stop_propagation();
                    fixes::run(id, fix, window, cx);
                }),
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
                        .on_click(cx.listener(move |_, _, window, cx| {
                            cx.stop_propagation();
                            action.run(id, window, cx);
                        }))
                }),
        )
        .child(
            Button::new(row_id("details", id))
                .small()
                .ghost()
                .icon(IconName::Info)
                .tooltip("Details")
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
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
