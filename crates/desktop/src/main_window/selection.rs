//! Choosing downloads in the list: a click picks one and shows its details, Ctrl+click adds or
//! takes one away, Shift+click and Shift with the arrows take a range, and the arrows move a
//! cursor. A selection built on purpose gets a bar over the list with what it can do.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, IntoElement, Modifiers, ParentElement as _, Styled as _, Window, div,
    px,
};
use wirecatch_protocol::app::DownloadSummary;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use super::applies::{self, Applicable};
use super::laid::Laid;
use super::{MainWindow, confirm, details, list};
use crate::backend::act;
use crate::files;
use crate::ui::select_circle::{Picked, SelectCircle};
use crate::ui::{Role, Typed as _};

/// How far Page Up and Page Down move the cursor.
const PAGE: usize = 10;

/// The selected downloads. Reaching the set to change it moves the revision, so nothing kept from
/// the selection can miss a change.
#[derive(Default)]
pub(super) struct Selected {
    ids: HashSet<DownloadId>,
    revision: u64,
}

impl Deref for Selected {
    type Target = HashSet<DownloadId>;

    fn deref(&self) -> &Self::Target {
        &self.ids
    }
}

impl DerefMut for Selected {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.revision += 1;
        &mut self.ids
    }
}

/// What the selection comes to in the list, kept until the lines, the selection or the queues
/// change: status and queue, all it reads, move only with the lines.
pub(super) struct Facts {
    from: FactsFrom,
    /// How many selected rows the commands reach.
    pub count: usize,
    /// Those of them in collapsed groups.
    folded: Rc<[DownloadId]>,
    can: Applicable,
    pub ids: Rc<[DownloadId]>,
    queues: Rc<[(QueueId, String)]>,
    many_queues: bool,
    /// How much of each heading's rows is ticked, by the kept list of those rows.
    picks: RefCell<HashMap<*const DownloadId, Picked>>,
}

/// All that [`Facts`] depend on.
struct FactsFrom {
    laid: Rc<Laid>,
    selected: u64,
    picked: bool,
    queue_list: u64,
}

impl PartialEq for FactsFrom {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.laid, &other.laid)
            && self.selected == other.selected
            && self.picked == other.picked
            && self.queue_list == other.queue_list
    }
}

/// Where the cursor goes.
#[derive(Clone, Copy)]
pub(super) enum Step {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
}

impl MainWindow {
    /// The downloads the list shows, in its order.
    pub(super) fn shown_ids(&self, cx: &App) -> Rc<[DownloadId]> {
        self.laid(cx).ids.clone()
    }

    /// The selected downloads, in the list's order.
    pub(super) fn selected_rows(&self, cx: &App) -> Vec<DownloadSummary> {
        self.chosen(&self.laid(cx), cx)
            .into_iter()
            .cloned()
            .collect()
    }

    /// The rows of `laid` the selection's commands reach.
    fn chosen<'a>(&self, laid: &Laid, cx: &'a App) -> Vec<&'a DownloadSummary> {
        selected_among(
            laid.under_commands(self.picked),
            &self.library.read(cx).downloads,
            &self.selected,
        )
    }

    /// The selection's facts, worked out again only when they may differ.
    pub(super) fn selection_facts(&self, cx: &App) -> Rc<Facts> {
        let library = self.library.read(cx);
        let from = FactsFrom {
            laid: self.laid(cx),
            selected: self.selected.revision,
            picked: self.picked,
            queue_list: library.revisions.queues,
        };
        let mut kept = self.selection_facts.borrow_mut();
        if let Some(facts) = kept.as_ref().filter(|facts| facts.from == from) {
            return facts.clone();
        }
        let rows = self.chosen(&from.laid, cx);
        let ids: Vec<DownloadId> = rows.iter().map(|row| row.id).collect();
        let facts = Rc::new(Facts {
            count: rows.len(),
            can: Applicable::to(&rows),
            ids: ids.clone().into(),
            folded: folded_among(&ids, &from.laid.ids).into(),
            queues: library
                .queues
                .iter()
                .filter(|queue| applies::can_move_to(&rows, queue.id))
                .map(|queue| (queue.id, queue.settings.name.clone()))
                .collect(),
            many_queues: library.queues.len() > 1,
            picks: RefCell::default(),
            from,
        });
        *kept = Some(facts.clone());
        facts
    }

    /// How much of `ids`, a list kept by the lines, is ticked.
    pub(super) fn kept_pick(&self, ids: &Rc<[DownloadId]>, cx: &App) -> Picked {
        let facts = self.selection_facts(cx);
        let mut picks = facts.picks.borrow_mut();
        *picks
            .entry(Rc::as_ptr(ids).cast())
            .or_insert_with(|| self.picked_of(ids))
    }

    /// Lets go of the selection, the cursor with it.
    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.cursor = None;
        self.picked = false;
    }

    /// A click on row `id`: Ctrl adds or takes it away, Shift takes the range from the last row
    /// chosen, and a plain click picks it alone and shows its details.
    pub(super) fn click_row(
        &mut self,
        id: DownloadId,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if modifiers.shift {
            self.select_range_to(id, cx);
        } else if modifiers.secondary(cx) {
            let shown = self.shown_ids(cx);
            ctrl_clicked(&mut self.selected, self.picked, id, &shown);
            self.anchor = Some(id);
            self.cursor = Some(id);
            self.picked = true;
        } else {
            self.pick(id);
            details::open(self.library.clone(), id, window, cx);
        }
        cx.notify();
    }

    /// A circle's click on `ids`; see [`circle_clicked`].
    pub(super) fn toggle_rows(&mut self, ids: &[DownloadId], cx: &mut Context<'_, Self>) {
        circle_clicked(&mut self.selected, self.picked, ids);
        self.picked = !self.selected.is_empty();
        self.anchor = ids.first().copied().filter(|_| self.picked);
        self.cursor = self.anchor;
        cx.notify();
    }

    /// Row `id` alone, as the place a range starts from.
    pub(super) fn pick(&mut self, id: DownloadId) {
        self.selected.clear();
        self.selected.insert(id);
        self.anchor = Some(id);
        self.cursor = Some(id);
        self.picked = false;
    }

    /// Selects from the anchor to `id`, in the list's order.
    fn select_range_to(&mut self, id: DownloadId, cx: &App) {
        let shown = self.shown_ids(cx);
        let anchor = self.anchor.filter(|a| shown.contains(a)).unwrap_or(id);
        let (Some(from), Some(to)) = (
            shown.iter().position(|s| *s == anchor),
            shown.iter().position(|s| *s == id),
        ) else {
            return;
        };
        let (low, high) = (from.min(to), from.max(to));
        *self.selected = shown[low..=high].iter().copied().collect();
        self.anchor = Some(anchor);
        self.cursor = Some(id);
        self.picked = true;
    }

    /// Moves the cursor a `step`; with `extend`, the selection grows from the anchor to it.
    pub(super) fn step(&mut self, step: Step, extend: bool, cx: &mut Context<'_, Self>) {
        let shown = self.shown_ids(cx);
        let Some(last) = shown.len().checked_sub(1) else {
            return;
        };
        let at = self
            .cursor
            .and_then(|cursor| shown.iter().position(|id| *id == cursor));
        let to = match (step, at) {
            (Step::First, _) | (Step::Down | Step::PageDown, None) => 0,
            (Step::Last, _) | (Step::Up | Step::PageUp, None) => last,
            (Step::Up, Some(at)) => at.saturating_sub(1),
            (Step::Down, Some(at)) => (at + 1).min(last),
            (Step::PageUp, Some(at)) => at.saturating_sub(PAGE),
            (Step::PageDown, Some(at)) => (at + PAGE).min(last),
        };
        let id = shown[to];
        if extend {
            self.select_range_to(id, cx);
        } else {
            self.pick(id);
        }
        self.scroll_to_row(id, cx);
        cx.notify();
    }

    /// Selects every download the list shows.
    pub(super) fn select_all(&mut self, cx: &mut Context<'_, Self>) {
        let shown = self.shown_ids(cx);
        if shown.is_empty() {
            return;
        }
        *self.selected = shown.iter().copied().collect();
        self.anchor = shown.first().copied();
        self.cursor = shown.last().copied();
        self.picked = true;
        cx.notify();
    }

    /// Space: pauses what of the selection runs or waits, else resumes what is paused.
    pub(super) fn pause_or_resume(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let rows = self.selected_rows(cx);
        let rows: Vec<&DownloadSummary> = rows.iter().collect();
        if Applicable::to(&rows).pause {
            each(&rows, applies::can_pause, api::pause, window, cx);
        } else {
            each(&rows, applies::can_resume, api::resume, window, cx);
        }
    }

    /// Enter: opens the selected file, when one finished download is selected and on show.
    pub(super) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(id) = to_open(&self.selected_rows(cx), &self.shown_ids(cx)) {
            files::open(id, window, cx);
        }
    }

    /// Alt+Enter: the details of the download under the cursor.
    pub(super) fn details_of_cursor(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(id) = self.cursor_row(cx) {
            details::open(self.library.clone(), id, window, cx);
        }
    }

    /// The cursor's download, while it is selected and the list shows it.
    pub(super) fn cursor_row(&self, cx: &App) -> Option<DownloadId> {
        let facts = self.selection_facts(cx);
        cursor_on_show(self.cursor, &facts.ids, &self.shown_ids(cx))
    }

    /// Delete: asks before removing the selection.
    pub(super) fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let ids: Vec<DownloadId> = self.selected_rows(cx).iter().map(|r| r.id).collect();
        let folded = folded_among(&ids, &self.shown_ids(cx));
        confirm::remove(&self.library, &ids, &folded, window, cx);
    }

    /// The bar's circle, in the list's column of circles: ticks every download the list shows,
    /// or unticks them when all are. Ticks in folded groups stay (D160).
    fn every_circle(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let shown = self.shown_ids(cx);
        let every = bar_pick(
            self.kept_pick(&shown, cx),
            self.selection_facts(cx).folded.len(),
        );
        h_flex().w(px(list::CIRCLE_WIDTH)).justify_center().child(
            SelectCircle::new("sel-every", every)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_rows(&shown, cx))),
        )
    }

    /// The bar over the list while a selection is built up on purpose: several downloads, or one
    /// added with Ctrl. A row only clicked has its details instead.
    pub(super) fn selection_bar(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        if self.selected.len() < 2 && !self.picked {
            return None;
        }
        let facts = self.selection_facts(cx);
        let count = facts.count;
        if count < 2 && !(self.picked && count == 1) {
            return None;
        }
        let folded = facts.folded.clone();
        let ids = facts.ids.clone();
        let theme = cx.theme();
        let library = self.library.clone();
        let bar = h_flex()
            .mx_1()
            .mb_2()
            .pl(px(list::PAD - 2.))
            .pr_2()
            .py_1()
            .gap_1()
            .rounded(theme.radius)
            .bg(theme.list_active)
            .child(self.every_circle(cx))
            .child(
                div()
                    .px_2()
                    .text_role(Role::Strong, cx)
                    .child(format!("{count} selected")),
            )
            .child(div().flex_1())
            .children(state_buttons(&facts.can, cx))
            .when(facts.many_queues && !facts.queues.is_empty(), |bar| {
                bar.child(move_button(facts.queues.clone(), ids.clone()))
            })
            .child(
                bar_button("sel-remove", "Remove", Icon::new(Lucide::Trash)).on_click(
                    move |_, window, cx| {
                        confirm::remove(&library, &ids, &folded, window, cx);
                    },
                ),
            )
            .child(
                Button::new("sel-clear")
                    .small()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip("Clear the selection")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clear_selection();
                        cx.notify();
                    })),
            );
        Some(bar.into_any_element())
    }
}

/// A command the bar offers when some of the selection takes it.
pub(super) type Command = fn(&wirecatch_core::Manager, DownloadId) -> Result<(), ProtocolError>;

/// One of the bar's buttons, `shown` while some of the selection `takes` its command.
struct Offer {
    shown: bool,
    id: &'static str,
    label: &'static str,
    icon: IconName,
    takes: fn(&DownloadSummary) -> bool,
    command: Command,
}

/// Pause, Resume and Try again, each while some of the selection takes it.
fn state_buttons(can: &Applicable, cx: &mut Context<'_, MainWindow>) -> Vec<Button> {
    let offered = [
        Offer {
            shown: can.pause,
            id: "sel-pause",
            label: "Pause",
            icon: IconName::Pause,
            takes: applies::can_pause,
            command: api::pause,
        },
        Offer {
            shown: can.resume,
            id: "sel-resume",
            label: "Resume",
            icon: IconName::Play,
            takes: applies::can_resume,
            command: api::resume,
        },
        Offer {
            shown: can.retry,
            id: "sel-retry",
            label: can.retry_word,
            icon: IconName::RotateCw,
            takes: applies::can_retry,
            command: api::retry,
        },
    ];
    offered
        .into_iter()
        .filter(|offer| offer.shown)
        .map(
            |Offer {
                 id,
                 label,
                 icon,
                 takes,
                 command,
                 ..
             }| {
                bar_button(id, label, Icon::new(icon)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        let rows = this.selected_rows(cx);
                        let rows: Vec<&DownloadSummary> = rows.iter().collect();
                        each(&rows, takes, command, window, cx);
                    },
                ))
            },
        )
        .collect()
}

/// Move to queue, with the `queues` some of `ids` are not in.
fn move_button(queues: Rc<[(QueueId, String)]>, ids: Rc<[DownloadId]>) -> impl IntoElement {
    bar_button(
        "sel-move",
        "Move to queue",
        Icon::new(Lucide::ArrowRightLeft),
    )
    .dropdown_menu(move |menu, _, _| {
        queues.iter().fold(menu, |menu, (queue, name)| {
            let (queue, ids) = (*queue, ids.clone());
            menu.item(
                PopupMenuItem::new(name.clone()).on_click(move |_, window, cx| {
                    let ids = ids.to_vec();
                    act(window.window_handle(), cx, move |manager| {
                        api::move_to_queue(manager, &ids, queue)
                    });
                }),
            )
        })
    })
}

fn bar_button(id: &'static str, label: &'static str, icon: Icon) -> Button {
    Button::new(id).small().ghost().icon(icon).label(label)
}

/// Runs `command` on each of `rows` that `takes` it; the first refusal shows in the window.
pub(super) fn each(
    rows: &[&DownloadSummary],
    takes: fn(&DownloadSummary) -> bool,
    command: Command,
    window: &mut Window,
    cx: &mut App,
) {
    let ids: Vec<DownloadId> = rows.iter().filter(|r| takes(r)).map(|r| r.id).collect();
    each_id(ids, command, window, cx);
}

/// Runs `command` on each of `ids`; the first refusal shows in the window.
pub(super) fn each_id(ids: Vec<DownloadId>, command: Command, window: &mut Window, cx: &mut App) {
    act(window.window_handle(), cx, move |manager| {
        through_every(ids, |id| command(manager, id))
    });
}

/// Runs `run` on every one of `ids`, past a refusal, and answers with the first refusal.
fn through_every<E>(
    ids: Vec<DownloadId>,
    mut run: impl FnMut(DownloadId) -> Result<(), E>,
) -> Result<(), E> {
    let mut refused = None;
    for id in ids {
        if let Err(e) = run(id) {
            refused.get_or_insert(e);
        }
    }
    refused.map_or(Ok(()), Err)
}

/// Those of `ids` the list does not show, because their group is collapsed.
pub(super) fn folded_among(ids: &[DownloadId], shown: &[DownloadId]) -> Vec<DownloadId> {
    let shown: HashSet<DownloadId> = shown.iter().copied().collect();
    ids.iter()
        .copied()
        .filter(|id| !shown.contains(id))
        .collect()
}

/// What Enter opens: the one selected download, when it finished and is on show.
fn to_open(rows: &[DownloadSummary], shown: &[DownloadId]) -> Option<DownloadId> {
    match rows {
        [row]
            if row.status == wirecatch_protocol::app::DownloadStatus::Completed
                && shown.contains(&row.id) =>
        {
            Some(row.id)
        }
        _ => None,
    }
}

/// Ctrl+click on `id`: adds it or takes it away. A row only clicked stays beside it while on show; one folded away is let go, since nothing shows it is held.
fn ctrl_clicked(
    selected: &mut HashSet<DownloadId>,
    picked: bool,
    id: DownloadId,
    shown: &[DownloadId],
) {
    if !picked {
        selected.retain(|held| shown.contains(held));
    }
    if !selected.remove(&id) {
        selected.insert(id);
    }
}

/// A circle's click: ticks every one of `ids` while any is unticked, else unticks them all, leaving
/// other ticks as they are. A row only clicked is let go first, since its circle never showed it
/// ticked.
fn circle_clicked(selected: &mut HashSet<DownloadId>, picked: bool, ids: &[DownloadId]) {
    if !picked {
        selected.clear();
    }
    if ids.iter().all(|id| selected.contains(id)) {
        for id in ids {
            selected.remove(id);
        }
    } else {
        selected.extend(ids.iter().copied());
    }
}

/// What the bar's circle shows: how much of the list is ticked, and never empty while ticks in
/// folded groups are in the bar's count.
fn bar_pick(shown: Picked, folded: usize) -> Picked {
    if folded > 0 && shown == Picked::None {
        Picked::Some
    } else {
        shown
    }
}

/// The cursor, while it is on a selected row the list shows.
fn cursor_on_show(
    cursor: Option<DownloadId>,
    selected: &[DownloadId],
    shown: &[DownloadId],
) -> Option<DownloadId> {
    cursor.filter(|id| selected.contains(id) && shown.contains(id))
}

/// The rows at `shown`, in that order, that `selected` holds: it may hold rows the view hides.
fn selected_among<'a>(
    shown: &[usize],
    downloads: &'a [DownloadSummary],
    selected: &HashSet<DownloadId>,
) -> Vec<&'a DownloadSummary> {
    if selected.is_empty() {
        return Vec::new();
    }
    shown
        .iter()
        .filter_map(|index| downloads.get(*index))
        .filter(|row| selected.contains(&row.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::laid::Asked;
    use super::super::{View, sort};
    use super::*;

    #[test]
    fn a_refusal_does_not_stop_the_rest_and_the_first_is_told() {
        let mut ran = Vec::new();
        let answer = through_every(vec![DownloadId(1), DownloadId(2), DownloadId(3)], |id| {
            ran.push(id.0);
            if id.0 < 3 { Err(id.0) } else { Ok(()) }
        });
        assert_eq!(answer, Err(1));
        assert_eq!(ran, [1, 2, 3]);
    }

    fn ids(list: &[i64]) -> HashSet<DownloadId> {
        list.iter().copied().map(DownloadId).collect()
    }

    #[test]
    fn ctrl_click_lets_go_of_a_row_only_clicked_once_it_is_folded_away() {
        let (a, b) = (DownloadId(1), DownloadId(2));
        let mut selected = ids(&[1]);
        ctrl_clicked(&mut selected, false, b, &[b]);
        assert_eq!(selected, ids(&[2]), "row 1 is off screen");

        let mut selected = ids(&[1]);
        ctrl_clicked(&mut selected, false, b, &[a, b]);
        assert_eq!(selected, ids(&[1, 2]), "row 1 is on screen");

        let mut ticked = ids(&[1]);
        ctrl_clicked(&mut ticked, true, b, &[b]);
        assert_eq!(ticked, ids(&[1, 2]), "row 1 was ticked on purpose");
    }

    #[test]
    fn the_bars_circle_leaves_ticks_in_folded_groups() {
        let shown = [DownloadId(1), DownloadId(2)];
        let mut selected = ids(&[9]);
        circle_clicked(&mut selected, true, &shown);
        assert_eq!(selected, ids(&[1, 2, 9]));
        circle_clicked(&mut selected, true, &shown);
        assert_eq!(selected, ids(&[9]));
        circle_clicked(&mut selected, true, &[]);
        assert_eq!(selected, ids(&[9]), "every group folded");
    }

    #[test]
    fn folded_rows_are_those_the_list_does_not_show() {
        let (a, b, c) = (DownloadId(1), DownloadId(2), DownloadId(3));
        assert_eq!(folded_among(&[a, b, c], &[b]), [a, c]);
        assert_eq!(folded_among(&[a, b], &[a, b]), []);
        assert_eq!(folded_among(&[a], &[]), [a]);
    }

    #[test]
    fn enter_opens_only_a_finished_download_on_show() {
        use wirecatch_protocol::app::DownloadStatus;
        let row = |id, status| DownloadSummary {
            status,
            ..wirecatch_test_support::downloads::download(id)
        };
        let done = row(1, DownloadStatus::Completed);
        let id = done.id;
        assert_eq!(to_open(std::slice::from_ref(&done), &[id]), Some(id));
        assert_eq!(to_open(std::slice::from_ref(&done), &[]), None, "collapsed");
        let running = row(1, DownloadStatus::Downloading);
        assert_eq!(to_open(&[running], &[id]), None, "not finished");
        let other = row(2, DownloadStatus::Completed);
        assert_eq!(to_open(&[done, other], &[id, DownloadId(2)]), None, "two");
    }

    #[test]
    fn the_bars_circle_is_not_empty_while_folded_ticks_count() {
        assert_eq!(bar_pick(Picked::None, 3), Picked::Some);
        assert_eq!(bar_pick(Picked::None, 0), Picked::None);
        assert_eq!(bar_pick(Picked::Some, 3), Picked::Some);
        assert_eq!(bar_pick(Picked::All, 3), Picked::All);
    }

    #[test]
    fn the_cursor_acts_only_on_a_selected_row_on_show() {
        let (a, b) = (DownloadId(1), DownloadId(2));
        assert_eq!(cursor_on_show(Some(a), &[a, b], &[a]), Some(a));
        assert_eq!(cursor_on_show(Some(b), &[a, b], &[a]), None, "folded away");
        assert_eq!(cursor_on_show(Some(a), &[b], &[a]), None, "not selected");
        assert_eq!(cursor_on_show(None, &[a], &[a]), None);
    }

    #[test]
    fn the_selection_acts_on_shown_rows_in_the_lists_order() {
        let downloads = [
            wirecatch_test_support::downloads::download(1),
            wirecatch_test_support::downloads::download(2),
            wirecatch_test_support::downloads::download(3),
        ];
        let selected = [DownloadId(1), DownloadId(2), DownloadId(3)]
            .into_iter()
            .collect();
        let rows = selected_among(&[2, 0], &downloads, &selected);
        let ids: Vec<i64> = rows.iter().map(|row| row.id.0).collect();
        assert_eq!(ids, [3, 1], "row 2 is hidden");
    }

    #[test]
    fn every_change_of_the_selection_moves_its_revision() {
        let mut selected = Selected::default();
        let changes: [fn(&mut Selected); 5] = [
            |selected| {
                selected.insert(DownloadId(1));
            },
            |selected| {
                selected.remove(&DownloadId(1));
            },
            |selected| selected.extend([DownloadId(2)]),
            |selected| **selected = [DownloadId(3)].into_iter().collect(),
            |selected| selected.clear(),
        ];
        for change in changes {
            let before = selected.revision;
            change(&mut selected);
            assert_ne!(selected.revision, before);
        }
        let before = selected.revision;
        assert!(!selected.contains(&DownloadId(3)));
        assert_eq!(selected.revision, before, "reading moves nothing");
    }

    fn laid() -> Rc<Laid> {
        let asked = Asked {
            revision: 0,
            view: View::All,
            needle: String::new(),
            sort: sort::Sort::default(),
            folded: Asked::folded(&HashSet::new()),
        };
        Rc::new(Laid::of(asked, &[], &mut sort::Frozen::default()))
    }

    #[test]
    fn facts_hold_only_while_all_they_depend_on_holds() {
        let lines = laid();
        let from = || FactsFrom {
            laid: lines.clone(),
            selected: 1,
            picked: false,
            queue_list: 1,
        };
        assert!(from() == from());
        let others = [
            FactsFrom {
                laid: laid(),
                ..from()
            },
            FactsFrom {
                selected: 2,
                ..from()
            },
            FactsFrom {
                picked: true,
                ..from()
            },
            FactsFrom {
                queue_list: 2,
                ..from()
            },
        ];
        for other in others {
            assert!(other != from());
        }
    }
}
