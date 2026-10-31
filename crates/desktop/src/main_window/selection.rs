//! Choosing downloads in the list: a click picks one and shows its details, Ctrl+click adds or
//! takes one away, Shift+click and Shift with the arrows take a range, and the arrows move a
//! cursor. A selection built on purpose gets a bar over the list with what it can do.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, IntoElement, Modifiers, ParentElement as _, Styled as _, Window, div,
    px,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadSummary;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api;

use super::applies::{self, Applicable};
use super::{MainWindow, confirm, details, list};
use crate::backend::act;
use crate::files;
use crate::ui::select_circle::{Picked, SelectCircle};
use crate::ui::{Role, Typed as _};

/// How far Page Up and Page Down move the cursor.
const PAGE: usize = 10;

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
    pub(super) fn shown_ids(&self, cx: &App) -> Vec<DownloadId> {
        let downloads = &self.library.read(cx).downloads;
        self.shown_rows(cx)
            .into_iter()
            .filter_map(|index| downloads.get(index).map(|row| row.id))
            .collect()
    }

    /// The selected downloads, in the list's order.
    pub(super) fn selected_rows(&self, cx: &App) -> Vec<DownloadSummary> {
        let downloads = &self.library.read(cx).downloads;
        self.shown_ids(cx)
            .into_iter()
            .filter(|id| self.selected.contains(id))
            .filter_map(|id| downloads.iter().find(|row| row.id == id).cloned())
            .collect()
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
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.anchor = Some(id);
            self.cursor = Some(id);
            self.picked = true;
        } else {
            self.pick(id);
            details::open(self.library.clone(), id, window, cx);
        }
        cx.notify();
    }

    /// A circle's click: ticks every one of `ids` while any is unticked, else unticks them all. A
    /// row only clicked is let go first, since its circle never showed it ticked.
    pub(super) fn toggle_rows(&mut self, ids: &[DownloadId], cx: &mut Context<'_, Self>) {
        if !self.picked {
            self.selected.clear();
        }
        if ids.iter().all(|id| self.selected.contains(id)) {
            for id in ids {
                self.selected.remove(id);
            }
        } else {
            self.selected.extend(ids.iter().copied());
        }
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
        self.selected = shown[low..=high].iter().copied().collect();
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
        self.selected = shown.iter().copied().collect();
        self.anchor = shown.first().copied();
        self.cursor = shown.last().copied();
        self.picked = !shown.is_empty();
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

    /// Enter: opens the selected file, when one finished download is selected.
    pub(super) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let [row] = self.selected_rows(cx).as_slice()
            && row.status == wirecatch_protocol::app::DownloadStatus::Completed
        {
            files::open(row.id, window, cx);
        }
    }

    /// Alt+Enter: the details of the download under the cursor.
    pub(super) fn details_of_cursor(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(id) = self.cursor.filter(|id| self.selected.contains(id)) {
            details::open(self.library.clone(), id, window, cx);
        }
    }

    /// Delete: asks before removing the selection.
    pub(super) fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let ids: Vec<DownloadId> = self.selected_rows(cx).iter().map(|r| r.id).collect();
        confirm::remove(&self.library, &ids, window, cx);
    }

    /// The bar's circle, in the list's column of circles: ticks every download the list shows,
    /// or unticks them when all are.
    fn every_circle(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let every = self.picked_of(&self.shown_ids(cx));
        h_flex().w(px(list::CIRCLE_WIDTH)).justify_center().child(
            SelectCircle::new("sel-every", every).on_click(cx.listener(move |this, _, _, cx| {
                if every == Picked::All {
                    this.clear_selection();
                    cx.notify();
                } else {
                    this.select_all(cx);
                }
            })),
        )
    }

    /// The bar over the list while a selection is built up on purpose: several downloads, or one
    /// added with Ctrl. A row only clicked has its details instead.
    pub(super) fn selection_bar(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let rows = self.selected_rows(cx);
        if rows.len() < 2 && !(self.picked && rows.len() == 1) {
            return None;
        }
        let refs: Vec<&DownloadSummary> = rows.iter().collect();
        let can = Applicable::to(&refs);
        let ids: Vec<DownloadId> = rows.iter().map(|row| row.id).collect();
        let queues: Vec<(wirecatch_protocol::QueueId, String)> = self
            .library
            .read(cx)
            .queues
            .iter()
            .filter(|queue| applies::can_move_to(&refs, queue.id))
            .map(|queue| (queue.id, queue.settings.name.clone()))
            .collect();
        let many_queues = self.library.read(cx).queues.len() > 1;
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
                    .child(format!("{} selected", rows.len())),
            )
            .child(div().flex_1())
            .children(state_buttons(&rows, &can, cx))
            .when(many_queues && !queues.is_empty(), |bar| {
                let ids = ids.clone();
                bar.child(
                    bar_button(
                        "sel-move",
                        "Move to queue",
                        Icon::new(Lucide::ArrowRightLeft),
                    )
                    .dropdown_menu(move |menu, _, _| {
                        queues.iter().fold(menu, |menu, (queue, name)| {
                            let (queue, ids) = (*queue, ids.clone());
                            menu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, window, cx| {
                                    let ids = ids.clone();
                                    act(window.window_handle(), cx, move |manager| {
                                        api::move_to_queue(manager, &ids, queue)
                                    });
                                },
                            ))
                        })
                    }),
                )
            })
            .child(
                bar_button("sel-remove", "Remove", Icon::new(Lucide::Trash)).on_click(
                    move |_, window, cx| {
                        confirm::remove(&library, &ids, window, cx);
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

/// Pause, Resume and Try again, each while some of `rows` takes it.
fn state_buttons(
    rows: &[DownloadSummary],
    can: &Applicable,
    cx: &mut Context<'_, MainWindow>,
) -> Vec<Button> {
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
            label: "Try again",
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
                let rows = rows.to_vec();
                bar_button(id, label, Icon::new(icon)).on_click(cx.listener(
                    move |_, _, window, cx| {
                        let rows: Vec<&DownloadSummary> = rows.iter().collect();
                        each(&rows, takes, command, window, cx);
                    },
                ))
            },
        )
        .collect()
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
    act(window.window_handle(), cx, move |manager| {
        let mut refused = None;
        for id in ids {
            if let Err(e) = command(manager, id) {
                refused.get_or_insert(e);
            }
        }
        refused.map_or(Ok(()), Err)
    });
}
