//! The download list: one line per download, grouped by status in the order [`Group::ALL`] gives,
//! each group under a heading that folds it; finished downloads in queue order sit under the day
//! they finished. Every row shares one grid: a circle that picks it, its time to the second (or
//! time left), its file tile, its name over one line of its state, link and folder, and a figure.

use std::rc::Rc;

use chrono::NaiveDate;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, Hsla, InteractiveElement as _, IntoElement, ListAlignment,
    ListOffset, ListState, MouseButton, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, list, px, relative,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    DownloadStatus, DownloadSummary, FixAction, ListLines, PER_MILLE_DONE,
};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::api;

use super::applies::RowAction;
use super::drag::{self, DraggedRows};
use super::laid::{Asked, Day, Laid, Line};
use super::{Group, MainWindow, View, fixes, row_menu};
use crate::backend::act;
use crate::rows::Tally;
use crate::text;
use crate::ui::select_circle::{Picked, SelectCircle};
use crate::ui::{Chip, FileTile, Role, Typed as _, icon, tokens, tooltip};

/// Space before the circle column and after the figure.
pub(super) const PAD: f32 = 12.;
/// The column of circles that pick rows and headings.
pub(super) const CIRCLE_WIDTH: f32 = 18.;
const TIME_WIDTH: f32 = 78.;
const TILE_WIDTH: f32 = 32.;
const GAP: f32 = 12.;
/// Every line's left border, which marks a selected row.
const BORDER: f32 = 2.;
/// The figure at a row's end, and the buttons that take its place on hover.
const END_WIDTH: f32 = 112.;
const PROGRESS_HEIGHT: f32 = 3.;
const ONE_LINE_HEIGHT: f32 = 36.;
const TWO_LINES_HEIGHT: f32 = 56.;
/// How far past the visible lines the list lays out, so scrolling shows no gaps.
const OVERDRAW: f32 = 400.;
const HEADING_HEIGHT: f32 = 32.;
const HEADING_SPACE: f32 = 12.;
/// A list whose top is in one of its first lines counts as at the top: a wheel notch passes the
/// first heading.
const NEAR_TOP_LINES: usize = 2;

/// The list's scroll and the lines it last laid out, which its state follows.
pub(super) struct Scroll {
    state: ListState,
    drawn: Option<Rc<Laid>>,
    /// What each drawn line shows.
    drawn_keys: Vec<LineKey>,
    shown: Option<ListLines>,
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            state: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            drawn: None,
            drawn_keys: Vec::new(),
            shown: None,
        }
    }
}

/// Where line `index` sits in view of a list whose top is `top`.
#[derive(Clone, Copy)]
struct InView {
    index: usize,
    top: ListOffset,
}

/// The id of a part of row `id` holding tooltips, from where the row sits in view. GPUI hides a
/// tooltip on a wheel scroll, a pointer move or keyboard input, not when lines move otherwise, so
/// a row that moved takes new ids, which drops its tooltip; a row that stays keeps them.
fn tip_id(part: &'static str, id: DownloadId, at: InView) -> ElementId {
    let (below, lines) = (
        at.index >= at.top.item_ix,
        at.index.abs_diff(at.top.item_ix),
    );
    ElementId::Name(
        format!(
            "{part}-{}-{below}-{lines}-{:?}",
            id.0, at.top.offset_in_item
        )
        .into(),
    )
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

/// The command over `group`, while it has downloads.
fn command_for(group: Group, tally: &Tally) -> Option<(&'static str, GroupCommand)> {
    if tally.in_group(group) == 0 {
        return None;
    }
    match group {
        Group::Downloading => Some((text::PAUSE_ALL, GroupCommand::PauseAll)),
        Group::UpNext if tally.running.is_empty() => {
            Some((text::PAUSE_ALL, GroupCommand::PauseAll))
        }
        Group::Paused => Some((text::RESUME_ALL, GroupCommand::ResumeAll)),
        _ => None,
    }
}

/// What a line shows, which stays when other lines come and go: row lines name downloads by
/// their index in the library, which a removal above them changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LineKey {
    Heading(Group),
    Day(NaiveDate),
    Row(Option<DownloadId>),
}

fn line_keys(lines: &[Line], downloads: &[DownloadSummary]) -> Vec<LineKey> {
    lines
        .iter()
        .map(|line| match *line {
            Line::Heading(group, _) => LineKey::Heading(group),
            Line::Day(day) => LineKey::Day(day),
            Line::Row(index, ..) => LineKey::Row(downloads.get(index).map(|row| row.id)),
        })
        .collect()
}

/// Replaces the lines in `old` with `count` new ones, `drawn` before and `lines` after. A list
/// near its top stays at the top, so lines added above show; further down, the line at the top
/// stays there.
fn splice_keeping_place(
    state: &ListState,
    old: std::ops::Range<usize>,
    count: usize,
    drawn: &[LineKey],
    lines: &[LineKey],
) {
    // GPUI moves its anchor past lines inserted at or before it, even at the very top, and to the
    // start of the range when the range holds it.
    let top = state.logical_scroll_top();
    let near_top = top.item_ix < NEAR_TOP_LINES && old.start <= top.item_ix;
    let kept = old
        .contains(&top.item_ix)
        .then(|| drawn.get(top.item_ix))
        .flatten();
    state.splice(old, count);
    if near_top {
        state.scroll_to(ListOffset::default());
    } else if let Some(item_ix) = kept.and_then(|kept| lines.iter().position(|key| key == kept)) {
        state.scroll_to(ListOffset { item_ix, ..top });
    }
}

/// How tall a row is: its name alone, or over a second line when the View menu keeps the link or
/// the folder.
fn line_height(shown: ListLines) -> f32 {
    if has_second_line(shown) {
        TWO_LINES_HEIGHT
    } else {
        ONE_LINE_HEIGHT
    }
}

fn has_second_line(shown: ListLines) -> bool {
    shown.link || shown.folder
}

impl MainWindow {
    pub(super) fn list(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let laid = self.laid(cx);
        if laid.lines.is_empty() {
            return self.empty(cx).into_any_element();
        }
        self.sync_scroll(&laid, cx);
        let top = self.scroll.state.logical_scroll_top();
        list(
            self.scroll.state.clone(),
            cx.processor(move |this, index: usize, _, cx| {
                let shown = this.list_lines(cx);
                match laid.lines.get(index).copied() {
                    Some(Line::Heading(group, count)) => {
                        this.heading(group, count, laid.group_ids(group), cx)
                    }
                    Some(Line::Day(day)) => match laid.day(index) {
                        Some(facts) => this.day_heading(day, facts, cx),
                        None => div().into_any_element(),
                    },
                    Some(Line::Row(row, place, dated)) => {
                        this.row(row, place, dated, shown, InView { index, top }, cx)
                    }
                    None => div().into_any_element(),
                }
            }),
        )
        .flex_1()
        .pb_4()
        .into_any_element()
    }

    /// Keeps the list's state on `laid`: only the lines that changed are laid out again, so the
    /// scroll stays where it was, and every line is when the View menu changes the rows' height.
    fn sync_scroll(&mut self, laid: &Rc<Laid>, cx: &App) {
        let shown = self.list_lines(cx);
        let downloads = &self.library.read(cx).downloads;
        let scroll = &mut self.scroll;
        if !scroll
            .drawn
            .as_ref()
            .is_some_and(|drawn| Rc::ptr_eq(drawn, laid))
        {
            let drawn: &[Line] = scroll.drawn.as_ref().map_or(&[], |drawn| &drawn.lines);
            let lines = laid.lines.as_slice();
            let same_start = drawn.iter().zip(lines).take_while(|(a, b)| a == b).count();
            let same_end = drawn[same_start..]
                .iter()
                .rev()
                .zip(lines[same_start..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let keys = line_keys(lines, downloads);
            if same_start < drawn.len() || drawn.len() != lines.len() {
                splice_keeping_place(
                    &scroll.state,
                    same_start..drawn.len() - same_end,
                    lines.len() - same_start - same_end,
                    &scroll.drawn_keys,
                    &keys,
                );
            }
            scroll.drawn = Some(laid.clone());
            scroll.drawn_keys = keys;
        }
        if scroll.shown != Some(shown) {
            scroll.state.remeasure();
            scroll.shown = Some(shown);
        }
    }

    /// Scrolls download `id` into view, if the list shows it.
    pub(super) fn scroll_to_row(&mut self, id: DownloadId, cx: &App) {
        let laid = self.laid(cx);
        self.sync_scroll(&laid, cx);
        let downloads = &self.library.read(cx).downloads;
        if let Some(line) = laid
            .lines
            .iter()
            .position(|line| matches!(line, Line::Row(index, ..) if downloads[*index].id == id))
        {
            self.scroll.state.scroll_to_reveal_item(line);
        }
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

    /// The list's lines, laid out again only when something they depend on changed.
    pub(super) fn laid(&self, cx: &App) -> Rc<Laid> {
        let downloads = &self.library.read(cx).downloads;
        let asked = Asked {
            revision: downloads.revision(),
            view: self.view,
            needle: self.needle(cx),
            sort: self.sort,
            folded: Asked::folded(&self.folded),
        };
        self.laid
            .borrow_mut()
            .get(asked, downloads, &mut self.frozen.borrow_mut())
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
        command_for(group, self.library.read(cx).downloads.tally())
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

    /// A heading's circle, which ticks every download under it.
    fn heading_circle(
        &self,
        name: SharedString,
        ids: Rc<[DownloadId]>,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let picked = self.kept_pick(&ids, cx);
        circle_cell().child(
            SelectCircle::new(ElementId::Name(name), picked)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_rows(&ids, cx))),
        )
    }

    fn heading(
        &self,
        group: Group,
        count: usize,
        ids: Rc<[DownloadId]>,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let (quiet, loud) = (cx.theme().muted_foreground, cx.theme().foreground);
        let folded = self.folded.contains(&group);
        let command = self.group_command(group, cx);
        heading_line()
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
    fn day_heading(&self, day: NaiveDate, facts: &Day, cx: &mut Context<'_, Self>) -> AnyElement {
        let files = match facts.ids.len() {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        heading_line()
            .child(self.heading_circle(format!("pick-day-{day}").into(), facts.ids.clone(), cx))
            .child(
                div()
                    .text_role(Role::Strong, cx)
                    .child(text::day_heading(day)),
            )
            .child(
                div()
                    .text_role(Role::Meta, cx)
                    .child(format!("{files}, {}", format_bytes(facts.bytes))),
            )
            .into_any_element()
    }

    fn row(
        &self,
        index: usize,
        place: Option<usize>,
        dated: bool,
        shown: ListLines,
        at: InView,
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
        let compact = !has_second_line(shown);
        let shows_circle = hovered || (self.picked && self.selection_facts(cx).count > 0);
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
            .child(text_column(&row, shown, tone, cx).id(tip_id("text", id, at)))
            .child(
                div()
                    .id(tip_id("end", id, at))
                    .flex_shrink_0()
                    .child(row_end(&row, shows_actions, tone, cx)),
            )
            .when(self.cursor == Some(id), |line| {
                line.child(marker(cursor_bounds))
            })
            .on_drag(
                self.dragged_rows(id, &row.file_name, cx),
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
    let fix = (row.status == DownloadStatus::Failed)
        .then(|| fixes::offered(row))
        .flatten();
    match end_of(fix, shows_actions) {
        End::Actions => row_actions(row, cx).into_any_element(),
        End::Fix(fix, details) => fix_button(row.id, fix, details, cx).into_any_element(),
        End::Figure => figure(row, tone, cx).into_any_element(),
    }
}

/// What a row's end shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum End {
    /// The hover buttons.
    Actions,
    /// A failed row's fix, with the Details button while hovered.
    Fix(FixAction, bool),
    Figure,
}

/// A fix stays put on hover: the pointer reaching it hovers the row.
fn end_of(fix: Option<FixAction>, shows_actions: bool) -> End {
    match (fix, shows_actions) {
        (Some(fix), details) => End::Fix(fix, details),
        (None, true) => End::Actions,
        (None, false) => End::Figure,
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

/// A heading's line, in the rows' grid, with room above it to set it apart from the rows before.
fn heading_line() -> gpui_kit::Div {
    h_flex()
        .w_full()
        .h(px(HEADING_HEIGHT + HEADING_SPACE))
        .pt(px(HEADING_SPACE))
        .pl(px(PAD))
        .pr(px(PAD))
        .gap(px(GAP))
        .border_l(px(BORDER))
        .items_center()
}

/// The cell a circle sits in, kept when the circle is hidden so the columns stay put.
fn circle_cell() -> gpui_kit::Div {
    h_flex()
        .w(px(CIRCLE_WIDTH))
        .flex_shrink_0()
        .justify_center()
}

/// What a row's time column shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gutter {
    /// The time left of a running download.
    Left(u64),
    Paused,
    /// Its place in a queue's line.
    Place(usize),
    /// When it finished, failed or was added.
    Clock,
    /// Nothing the row's heading does not already say.
    Blank,
}

fn gutter_of(row: &DownloadSummary, place: Option<usize>) -> Gutter {
    match (Group::of(row.status), place) {
        (Group::Downloading, _) => row.eta_secs.map_or(Gutter::Blank, Gutter::Left),
        (Group::Paused, _) => Gutter::Paused,
        (_, Some(place)) => Gutter::Place(place),
        (Group::UpNext, None) => Gutter::Blank,
        _ => Gutter::Clock,
    }
}

/// When a row happened, to the second; a running download's time left; a paused one's state.
fn time_cell(row: &DownloadSummary, place: Option<usize>, dated: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let cell = v_flex().w(px(TIME_WIDTH)).flex_shrink_0().justify_center();
    let word = |said: String, color: Hsla| {
        div()
            .text_role(Role::Strong, cx)
            .text_color(color)
            .child(said)
    };
    let shown = match gutter_of(row, place) {
        Gutter::Left(left) => word(text::eta(left), theme.primary),
        Gutter::Paused => word(
            text::status_label(DownloadStatus::Paused).to_owned(),
            theme.warning,
        ),
        Gutter::Place(place) => word(place.to_string(), theme.muted_foreground),
        Gutter::Blank => return cell.into_any_element(),
        Gutter::Clock => {
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

/// The name, over one line of what matters for the row's state, then the link and the folder the
/// View menu keeps; a progress bar under them while work is under way or paused.
fn text_column(row: &DownloadSummary, shown: ListLines, tone: Hsla, cx: &App) -> gpui_kit::Div {
    let second_line = has_second_line(shown);
    let failed = row.status == DownloadStatus::Failed;
    v_flex()
        .flex_1()
        .min_w_0()
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
                .when(failed && !second_line, |line| {
                    line.child(
                        div()
                            .flex_shrink_0()
                            .text_role(Role::Meta, cx)
                            .text_color(cx.theme().danger)
                            .child("Failed"),
                    )
                }),
        )
        .when(second_line, |column| {
            column.child(second_line_of(row, shown, tone, cx))
        })
        .children(progress_bar(row, tone, cx))
}

/// The row's second line: its state while that matters, then the link's host (with its path once
/// the download is over) and the folder as a chip.
fn second_line_of(row: &DownloadSummary, shown: ListLines, tone: Hsla, cx: &App) -> AnyElement {
    let state = state_part(row, tone, cx);
    let dot = state.is_some() && shown.link;
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .overflow_hidden()
        .text_role(Role::Meta, cx)
        .children(state)
        .when(dot, |line| line.child(div().flex_shrink_0().child("·")))
        .when(shown.link, |line| line.child(link_part(row, cx)))
        .when(shown.folder, |line| line.child(folder_chip(row, cx)))
        .into_any_element()
}

/// What a row says of its state, when its state is not plain finished: a failure's first
/// sentence, a running download's speed and size, how far a paused one got, or a word for the rest.
fn state_part(row: &DownloadSummary, tone: Hsla, cx: &App) -> Option<AnyElement> {
    let part = || h_flex().min_w_0().gap_2();
    let said = match row.status {
        DownloadStatus::Completed | DownloadStatus::Cancelled => return None,
        DownloadStatus::Failed => {
            part()
                .text_color(cx.theme().danger)
                .child(div().truncate().child(row.error.as_ref().map_or_else(
                    || text::status_short(row),
                    |error| text::first_sentence(&error.message).to_owned(),
                )))
        }
        DownloadStatus::Downloading if row.speed > 0 && text::shown_wait(row).is_none() => part()
            .flex_shrink_0()
            .child(div().text_color(tone).child(text::speed(row.speed)))
            .child(text::size(row)),
        DownloadStatus::Paused => part().flex_shrink_0().child(text::paused_at(row)?),
        _ => part().child(div().truncate().child(text::status_short(row))),
    };
    Some(said.into_any_element())
}

/// The link's host, brighter, and once the download is over its path; hovering shows it whole.
fn link_part(row: &DownloadSummary, cx: &App) -> AnyElement {
    let link = row.url.clone();
    let host = text::host(&link).to_owned();
    let path = Group::of(row.status)
        .is_over()
        .then(|| link_path(&link).to_owned());
    h_flex()
        .id(row_id("link", row.id))
        .min_w_0()
        .overflow_hidden()
        .child(
            div()
                .flex_shrink_0()
                .text_color(cx.theme().foreground.opacity(0.8))
                .child(host),
        )
        .children(path.map(|path| div().min_w_0().truncate().child(path)))
        .tooltip(move |window, cx| whole(link.clone(), window, cx))
        .into_any_element()
}

/// What follows a link's host: its path, query and fragment.
fn link_path(link: &str) -> &str {
    let rest = link.split_once("://").map_or(link, |(_, rest)| rest);
    rest.find(['/', '?', '#']).map_or("", |at| &rest[at..])
}

/// The folder the file is saved in, by its last name; hovering shows it whole.
fn folder_chip(row: &DownloadSummary, _: &App) -> AnyElement {
    let folder = saved_folder(&row.path);
    Chip::new(row_id("folder", row.id), last_name(&folder).to_owned())
        .whole(folder)
        .into_any_element()
}

/// The folder a saved file is in, as Settings shows folders.
fn saved_folder(path: &str) -> String {
    std::path::Path::new(&text::folder(path))
        .parent()
        .map_or_else(String::new, |folder| folder.display().to_string())
}

/// A folder's last name: `Video` of `D:\Downloads\Video`.
fn last_name(folder: &str) -> &str {
    let trimmed = folder.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
}

/// A popover with a line's whole text, wrapped.
fn whole(said: String, window: &mut Window, cx: &mut App) -> gpui_kit::AnyView {
    tooltip::unless_in_menu(window, cx, |window, cx| {
        Tooltip::element(move |_, cx| {
            div()
                .max_w(px(420.))
                .text_role(Role::Body, cx)
                .child(said.clone())
        })
        .build(window, cx)
    })
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

/// The bar under a row's text while work is under way or paused.
fn progress_bar(row: &DownloadSummary, tone: Hsla, cx: &App) -> Option<impl IntoElement> {
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
            .mt_0p5()
            .w_full()
            .h(px(PROGRESS_HEIGHT))
            .rounded_full()
            .bg(cx.theme().muted_foreground.opacity(0.15))
            .child(div().h_full().w(relative(done)).rounded_full().bg(tone)),
    )
}

/// Records where the cursor's row is drawn, for the menu the keyboard opens.
fn marker(
    bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
) -> impl IntoElement {
    canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, (), _, _| {})
        .absolute()
        .size_full()
}

/// A failed row's fix, shown where its figure would be and wider when its label needs it.
fn fix_button(
    id: DownloadId,
    fix: FixAction,
    details: bool,
    cx: &mut Context<'_, MainWindow>,
) -> impl IntoElement {
    h_flex()
        .min_w(px(END_WIDTH))
        .flex_shrink_0()
        .gap_1()
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
        .when(details, |end| end.child(details_button(id, cx)))
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
                    div()
                        .id(ElementId::NamedInteger(
                            format!("{label}-tip").into(),
                            id.0.unsigned_abs(),
                        ))
                        .tooltip(tooltip::plain(label))
                        .child(
                            Button::new(ElementId::NamedInteger(label.into(), id.0.unsigned_abs()))
                                .small()
                                .ghost()
                                .icon(icon.clone())
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    cx.stop_propagation();
                                    action.run(id, window, cx);
                                })),
                        )
                }),
        )
        .child(details_button(id, cx))
}

fn details_button(id: DownloadId, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    div()
        .id(row_id("details-tip", id))
        .tooltip(tooltip::plain("Details"))
        .child(
            Button::new(row_id("details", id))
                .small()
                .ghost()
                .icon(IconName::Info)
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

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::downloads::download;

    /// A tooltip that counts how often it is drawn.
    struct Tip(Rc<std::cell::Cell<usize>>);

    impl gpui_kit::Render for Tip {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().child("tip")
        }
    }

    /// A list of downloads' lines with tooltips, holding them as the download list's rows do.
    struct Lines {
        state: ListState,
        ids: Vec<i64>,
        drawn: Rc<std::cell::Cell<usize>>,
        clicks: Rc<std::cell::Cell<usize>>,
    }

    impl Lines {
        /// Adds downloads `ids` at line `at`.
        fn add(&mut self, at: usize, ids: std::ops::Range<i64>, cx: &mut Context<'_, Self>) {
            let count = ids.clone().count();
            self.ids.splice(at..at, ids);
            self.state.splice(at..at, count);
            cx.notify();
        }
    }

    impl gpui_kit::Render for Lines {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            let (drawn, clicks) = (self.drawn.clone(), self.clicks.clone());
            let (ids, top) = (self.ids.clone(), self.state.logical_scroll_top());
            list(self.state.clone(), move |index, _, _| {
                let (drawn, clicks) = (drawn.clone(), clicks.clone());
                let id = DownloadId(ids.get(index).copied().unwrap_or_default());
                div()
                    .id(tip_id("text", id, InView { index, top }))
                    .child(
                        div()
                            .id("line")
                            .h(px(20.))
                            .w_full()
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                            .tooltip(move |_, cx| {
                                let drawn = drawn.clone();
                                gpui_kit::AppContext::new(cx, |_| Tip(drawn)).into()
                            }),
                    )
                    .into_any_element()
            })
            .size_full()
        }
    }

    const POINTER: gpui_kit::Point<gpui_kit::Pixels> = gpui_kit::point(px(5.), px(45.));

    /// How many times something happened, shared with the views that count it.
    type Count = Rc<std::cell::Cell<usize>>;

    /// A window with a list of 50 downloads' lines, and what it counts.
    fn lines_window(
        cx: &mut gpui_kit::TestAppContext,
    ) -> (
        gpui_kit::Entity<Lines>,
        &mut gpui_kit::VisualTestContext,
        Count,
        Count,
    ) {
        cx.update(gpui_kit::component::init);
        let drawn = Rc::new(std::cell::Cell::new(0));
        let clicks = Rc::new(std::cell::Cell::new(0));
        let (window, lines) = cx
            .update(|cx| {
                let (drawn, clicks) = (drawn.clone(), clicks.clone());
                gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |_, cx| {
                    gpui_kit::AppContext::new(cx, |_| Lines {
                        state: ListState::new(50, ListAlignment::Top, px(OVERDRAW)),
                        ids: (1..=50).collect(),
                        drawn,
                        clicks,
                    })
                })
            })
            .unwrap();
        let cx = gpui_kit::VisualTestContext::from_window(window, cx).into_mut();
        cx.run_until_parked();
        (lines, cx, drawn, clicks)
    }

    /// Whether a tooltip shown over a line is still drawn after `moved` changes the list.
    fn tooltip_survives(
        moved: impl FnOnce(&gpui_kit::Entity<Lines>, &mut gpui_kit::VisualTestContext),
        cx: &mut gpui_kit::TestAppContext,
    ) -> bool {
        let (lines, cx, drawn, _) = lines_window(cx);
        cx.simulate_mouse_move(POINTER, None, gpui_kit::Modifiers::none());
        cx.executor().advance_clock(TOOLTIP_DELAY_PASSED);
        cx.run_until_parked();
        assert!(drawn.get() > 0, "the tooltip never showed");
        moved(&lines, cx);
        cx.run_until_parked();
        drawn.set(0);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        drawn.get() > 0
    }

    /// Longer than GPUI waits before it shows a tooltip.
    const TOOLTIP_DELAY_PASSED: std::time::Duration = std::time::Duration::from_secs(2);

    #[gpui_kit::test]
    fn a_wheel_scroll_closes_a_row_tooltip(cx: &mut gpui_kit::TestAppContext) {
        assert!(!tooltip_survives(
            |_, cx| {
                cx.simulate_event(gpui_kit::ScrollEvent {
                    position: gpui_kit::point(px(5.), px(45.)),
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-100.))),
                    ..Default::default()
                });
            },
            cx
        ));
    }

    #[gpui_kit::test]
    fn a_scroll_to_another_row_closes_a_row_tooltip(cx: &mut gpui_kit::TestAppContext) {
        assert!(!tooltip_survives(
            |lines, cx| {
                lines.update(cx, |lines, cx| {
                    lines.state.scroll_to(ListOffset {
                        item_ix: 10,
                        offset_in_item: px(0.),
                    });
                    cx.notify();
                });
            },
            cx
        ));
    }

    #[gpui_kit::test]
    fn rows_added_above_close_a_row_tooltip(cx: &mut gpui_kit::TestAppContext) {
        assert!(!tooltip_survives(
            |lines, cx| {
                lines.update(cx, |lines, cx| lines.add(0, 100..105, cx));
            },
            cx
        ));
    }

    #[gpui_kit::test]
    fn rows_added_below_keep_a_row_tooltip(cx: &mut gpui_kit::TestAppContext) {
        assert!(tooltip_survives(
            |lines, cx| {
                lines.update(cx, |lines, cx| lines.add(10, 100..105, cx));
            },
            cx
        ));
    }

    #[gpui_kit::test]
    fn a_press_on_a_row_that_stays_still_clicks_when_lines_change_below(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (lines, cx, _, clicks) = lines_window(cx);
        let none = gpui_kit::Modifiers::none();
        cx.simulate_mouse_move(POINTER, None, none);
        cx.simulate_mouse_down(POINTER, MouseButton::Left, none);
        lines.update(cx, |lines, cx| lines.add(10, 100..105, cx));
        cx.run_until_parked();
        cx.simulate_mouse_up(POINTER, MouseButton::Left, none);

        assert_eq!(clicks.get(), 1);
    }

    #[test]
    fn a_group_without_downloads_offers_no_command() {
        let row = |status| DownloadSummary {
            status,
            ..download(1)
        };
        let label = |group, rows: &[DownloadSummary]| {
            command_for(group, &rows.iter().collect::<Tally>()).map(|(label, _)| label)
        };
        assert_eq!(label(Group::Downloading, &[]), None);
        assert_eq!(label(Group::Paused, &[]), None);
        assert_eq!(label(Group::UpNext, &[]), None);
        let paused = [row(DownloadStatus::Paused)];
        assert_eq!(label(Group::Paused, &paused), Some("Resume all"));
        let waiting = [row(DownloadStatus::Queued)];
        assert_eq!(label(Group::UpNext, &waiting), Some("Pause all"));
        let running = [row(DownloadStatus::Downloading)];
        assert_eq!(label(Group::Downloading, &running), Some("Pause all"));
    }

    /// One of a row's lines under its name, inside a row that counts its clicks.
    struct LineInRow {
        row: DownloadSummary,
        line: fn(&DownloadSummary, &App) -> AnyElement,
        clicks: usize,
    }

    impl gpui_kit::Render for LineInRow {
        fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
            v_flex()
                .id("row")
                .size_full()
                .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
                .child((self.line)(&self.row, cx))
        }
    }

    /// How many clicks the row got from a click on `line`, and what the clipboard then holds.
    fn click_on(
        line: fn(&DownloadSummary, &App) -> AnyElement,
        cx: &mut gpui_kit::TestAppContext,
    ) -> (usize, Option<String>) {
        use gpui_kit::AppContext as _;
        cx.update(gpui_kit::component::init);
        let (window, root) = cx
            .update(|cx| {
                gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |_, cx| {
                    cx.new(|_| LineInRow {
                        row: download(1),
                        line,
                        clicks: 0,
                    })
                })
            })
            .unwrap();
        let cx = gpui_kit::VisualTestContext::from_window(window, cx).into_mut();
        cx.run_until_parked();
        cx.simulate_click(gpui_kit::point(px(4.), px(4.)), gpui_kit::Modifiers::none());
        let copied = cx.read_from_clipboard().and_then(|copied| copied.text());
        (root.read_with(cx, |root, _| root.clicks), copied)
    }

    #[gpui_kit::test]
    fn a_click_on_the_link_selects_the_row_and_copies_nothing(cx: &mut gpui_kit::TestAppContext) {
        assert_eq!(click_on(link_part, cx), (1, None));
    }

    #[gpui_kit::test]
    fn a_click_on_the_folder_selects_the_row(cx: &mut gpui_kit::TestAppContext) {
        assert_eq!(click_on(folder_chip, cx), (1, None));
    }

    #[test]
    fn the_time_column_says_what_the_heading_does_not() {
        let row = |status, eta_secs| DownloadSummary {
            status,
            eta_secs,
            ..download(1)
        };
        let running = row(DownloadStatus::Downloading, Some(90));
        assert_eq!(gutter_of(&running, None), Gutter::Left(90));
        let connecting = row(DownloadStatus::Connecting, None);
        assert_eq!(gutter_of(&connecting, None), Gutter::Blank);
        let paused = row(DownloadStatus::Paused, None);
        assert_eq!(gutter_of(&paused, None), Gutter::Paused);
        let queued = row(DownloadStatus::Queued, None);
        assert_eq!(gutter_of(&queued, None), Gutter::Blank);
        assert_eq!(gutter_of(&queued, Some(2)), Gutter::Place(2));
        let finished = row(DownloadStatus::Completed, None);
        assert_eq!(gutter_of(&finished, None), Gutter::Clock);
        let failed = row(DownloadStatus::Failed, None);
        assert_eq!(gutter_of(&failed, None), Gutter::Clock);
    }

    #[test]
    fn a_folder_chip_names_the_last_folder() {
        assert_eq!(last_name(r"D:\Downloads\Video"), "Video");
        assert_eq!(last_name("/home/me/Downloads/"), "Downloads");
        assert_eq!(last_name("Downloads"), "Downloads");
        assert_eq!(last_name(r"C:\"), "C:");
        assert_eq!(last_name(""), "");
    }

    #[test]
    fn a_link_path_follows_its_host() {
        assert_eq!(link_path("https://example.com/f/a.zip?x=1"), "/f/a.zip?x=1");
        assert_eq!(link_path("https://example.com?x=1"), "?x=1");
        assert_eq!(link_path("https://example.com"), "");
        assert_eq!(link_path("example.com/a"), "/a");
    }

    /// A list of `keys` scrolled to `top`.
    fn scrolled(keys: &[LineKey], top: ListOffset) -> ListState {
        let state = ListState::new(keys.len(), ListAlignment::Top, px(OVERDRAW));
        state.scroll_to(top);
        state
    }

    fn rows(ids: std::ops::Range<i64>) -> Vec<LineKey> {
        ids.map(|id| LineKey::Row(Some(DownloadId(id)))).collect()
    }

    #[test]
    fn lines_added_above_a_list_near_its_top_show() {
        let drawn = rows(1..4);
        for top in [0, 1] {
            let state = scrolled(
                &drawn,
                ListOffset {
                    item_ix: top,
                    offset_in_item: px(4.),
                },
            );
            let lines = [rows(10..12), drawn.clone()].concat();

            splice_keeping_place(&state, 0..0, 2, &drawn, &lines);

            assert_eq!(state.logical_scroll_top(), ListOffset::default(), "{top}");
        }
    }

    #[test]
    fn lines_changed_below_a_list_near_its_top_leave_its_top() {
        let drawn = rows(1..11);
        let reading = ListOffset {
            item_ix: 1,
            offset_in_item: px(4.),
        };
        let state = scrolled(&drawn, reading);
        let lines = [rows(1..6), rows(20..22), rows(6..11)].concat();

        splice_keeping_place(&state, 5..5, 2, &drawn, &lines);

        assert_eq!(state.logical_scroll_top(), reading);
    }

    #[test]
    fn lines_added_above_a_scrolled_list_leave_the_lines_in_view() {
        let drawn = rows(1..11);
        let reading = ListOffset {
            item_ix: 5,
            offset_in_item: px(4.),
        };
        let state = scrolled(&drawn, reading);
        let lines = [rows(20..22), drawn.clone()].concat();

        splice_keeping_place(&state, 0..0, 2, &drawn, &lines);

        assert_eq!(
            state.logical_scroll_top(),
            ListOffset {
                item_ix: 7,
                ..reading
            }
        );
    }

    #[test]
    fn a_removal_that_renumbers_the_line_at_the_top_keeps_it_there() {
        let drawn = rows(1..11);
        let reading = ListOffset {
            item_ix: 5,
            offset_in_item: px(4.),
        };
        let state = scrolled(&drawn, reading);
        let lines = rows(2..11);

        // Every row after the removed one has a new library index, so all of them changed.
        splice_keeping_place(&state, 0..10, 9, &drawn, &lines);

        assert_eq!(
            state.logical_scroll_top(),
            ListOffset {
                item_ix: 4,
                ..reading
            }
        );
    }

    #[test]
    fn a_fix_stays_when_its_row_is_hovered() {
        let fix = FixAction::ChooseFolder;
        assert_eq!(end_of(Some(fix), true), End::Fix(fix, true));
        assert_eq!(end_of(Some(fix), false), End::Fix(fix, false));
        assert_eq!(end_of(None, true), End::Actions);
        assert_eq!(end_of(None, false), End::Figure);
    }
}
