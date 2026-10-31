//! The navigation (D91): every download and each status group with their counts, then the queues,
//! each with its count or its speed while it downloads, what it is doing, and its own menu.

use chrono::Local;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity,
    div, px, relative,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, Queue, Setting};
use wirecatch_protocol::proxy::ProxyUse;
use wirecatch_protocol::rules::AddDownloads;
use wirecatch_queues::schedule;
use wirecatch_shell::api;

use super::drag::{self, DraggedQueue, DraggedRows};
use super::{Group, MainWindow, Screen, View};
use crate::backend::act;
use crate::text;
use crate::ui::speed_graph::{SpeedHistory, speed_graph};
use crate::ui::{QueueTile, Role, Typed as _, tokens};

pub(super) const WIDTH: f32 = 256.;
/// Queues shown before the rest fold away under "more queues".
const QUEUES_SHOWN: usize = 3;

impl MainWindow {
    pub(super) fn sidebar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let count = |view: View| {
            library
                .downloads
                .iter()
                .filter(|row| view.shows(row))
                .count()
        };
        let all = count(View::All);
        let counts = Group::ALL.map(|group| (group, count(View::Group(group))));
        let mut downloads = vec![self.nav_item(
            "view-all",
            Icon::new(Lucide::Layers),
            "All downloads",
            all,
            View::All,
            cx,
        )];
        for (group, shown) in counts {
            downloads.push(self.nav_item(
                ElementId::Name(SharedString::new(format!("view-{group:?}"))),
                group_icon(group),
                group.label(),
                shown,
                View::Group(group),
                cx,
            ));
        }
        let waiting = self.waiting_questions(cx);
        let queues = self.queues_section(cx);
        let theme = cx.theme();
        v_flex()
            .id("sidebar")
            .w(px(WIDTH))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                v_flex()
                    .id("sidebar-views")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_4()
                    .child(v_flex().gap_px().children(downloads).children(waiting))
                    .child(queues),
            )
            .child(self.screen_switch(cx))
    }

    /// The questions waiting about browser downloads, which brings all their windows up.
    fn waiting_questions(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let open = self.library.read(cx).open_questions.clone();
        if open.is_empty() {
            return None;
        }
        let label = match open.len() {
            1 => "1 question waiting".to_owned(),
            n => format!("{n} questions waiting"),
        };
        let theme = cx.theme();
        Some(
            h_flex()
                .id("waiting-questions")
                .h(px(30.))
                .px_2()
                .gap_2()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_color(theme.warning)
                .hover(|item| item.bg(theme.list_hover))
                .child(Icon::new(Lucide::CircleQuestionMark).with_size(tokens::ICON))
                .child(div().flex_1().truncate().child(label))
                .on_click(move |_, _, cx| crate::download_windows::show_all(&open, cx))
                .into_any_element(),
        )
    }

    /// The foot of the navigation: Settings, or the way back to the downloads from there.
    pub(super) fn screen_switch(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let (icon, label) = match self.screen {
            Screen::Downloads => (Icon::new(IconName::Settings), "Settings"),
            Screen::Settings => (Icon::new(Lucide::Download), "Downloads"),
        };
        let theme = cx.theme();
        div()
            .p_2()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .id("screen-switch")
                    .h(px(30.))
                    .px_2()
                    .gap_2()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(|item| item.bg(theme.list_hover).text_color(theme.foreground))
                    .child(icon.with_size(tokens::ICON))
                    .child(label)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_settings(cx))),
            )
    }

    /// The queues, in one panel: the first few, the one on screen, and the rest behind "more
    /// queues".
    fn queues_section(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let queues = self.library.read(cx).queues.clone();
        let folded = queues.len() > QUEUES_SHOWN && !self.all_queues;
        let shown: Vec<AnyElement> = queues
            .iter()
            .enumerate()
            .filter(|(index, queue)| {
                !folded || *index < QUEUES_SHOWN || self.view == View::Queue(queue.id)
            })
            .enumerate()
            .map(|(place, (index, queue))| {
                self.queue_item(queue, index, place == 0, queues.len(), cx)
            })
            .collect();
        let hidden = queues.len() - shown.len();
        let speed: u64 = self
            .library
            .read(cx)
            .downloads
            .iter()
            .map(|row| row.speed)
            .sum();
        let toggle = (queues.len() > QUEUES_SHOWN).then(|| {
            Button::new("more-queues")
                .xsmall()
                .ghost()
                .label(match (folded, hidden) {
                    (false, _) => "Fewer queues".to_owned(),
                    (true, 1) => "1 more queue".to_owned(),
                    (true, hidden) => format!("{hidden} more queues"),
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.all_queues = !this.all_queues;
                    cx.notify();
                }))
        });
        let theme = cx.theme();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .pl_2()
                    .gap_2()
                    .child(div().text_role(Role::Label, cx).child("Queues"))
                    .when(speed > 0, |header| {
                        header.child(
                            div()
                                .text_role(Role::Meta, cx)
                                .text_color(theme.primary)
                                .child(text::speed(speed)),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("new-queue")
                            .xsmall()
                            .ghost()
                            .icon(IconName::Plus)
                            .tooltip("New queue")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.edit_queue(None, window, cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .rounded(theme.radius_lg)
                    .overflow_hidden()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .children(shown),
            )
            .children(toggle)
    }

    fn nav_item(
        &self,
        id: impl Into<ElementId>,
        icon: Icon,
        label: impl Into<SharedString>,
        count: usize,
        view: View,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let current = self.view == view;
        let alarming = view == View::Group(Group::Attention) && count > 0;
        h_flex()
            .id(id)
            .h(px(30.))
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(current, |item| {
                item.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!current, |item| {
                item.text_color(theme.muted_foreground)
                    .hover(|item| item.bg(theme.list_hover))
            })
            .child(
                icon.with_size(tokens::ICON)
                    .when(current, |icon| icon.text_color(theme.primary)),
            )
            .child(div().flex_1().truncate().child(label.into()))
            .when(count > 0, |item| {
                item.child(
                    div()
                        .text_role(Role::Meta, cx)
                        .text_color(if alarming {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(count.to_string()),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
            .into_any_element()
    }

    /// One queue: its state and rules on a tile, its name with its speed or count, one line on
    /// what it is doing, and while it downloads its speed graph and progress.
    fn queue_item(
        &self,
        queue: &Queue,
        index: usize,
        first: bool,
        total: usize,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let id = queue.id;
        let view = View::Queue(id);
        let current = self.view == view;
        let library = self.library.read(cx);
        let facts = QueueFacts::of(library.downloads.iter().filter(|row| row.queue == id));
        let order: Vec<QueueId> = library.queues.iter().map(|q| q.id).collect();
        let (rules, proxy) = self.marks(queue, cx);
        let (alarm, line) = summary(queue, &facts, proxy.as_deref());
        let graph = self
            .queue_speeds
            .get(&id)
            .filter(|history| facts.running > 0 && history.drawable())
            .map(SpeedHistory::speeds);
        let main = cx.entity().downgrade();
        let shown = queue.clone();
        let theme = cx.theme();
        let panel = theme.background;
        let hovered = panel.blend(theme.list_hover);
        let behind = if current {
            panel.blend(theme.sidebar_accent)
        } else {
            panel
        };
        let group = SharedString::new(format!("queue-{}", id.0));
        let tile = queue_tile(queue, &facts, behind, cx)
            .rules(rules)
            .when(!current, |tile| tile.hover_behind(group.clone(), hovered));
        h_flex()
            .id(ElementId::NamedInteger("queue".into(), id.0.unsigned_abs()))
            .group(group)
            .relative()
            .pl(px(11.))
            .pr_3()
            .py(px(10.))
            .gap(px(11.))
            .cursor_pointer()
            .bg(behind)
            .when(!first, |item| item.border_t_1().border_color(theme.border))
            .when(!current, |item| item.hover(move |item| item.bg(hovered)))
            .when(current, |item| {
                item.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(9.))
                        .bottom(px(9.))
                        .w(px(3.))
                        .rounded_r(px(3.))
                        .bg(theme.primary),
                )
            })
            .child(tile)
            .child(queue_body(
                &queue.settings.name,
                &facts,
                (alarm, line),
                graph,
                cx,
            ))
            .map(|item| drops(item, id, index, &queue.settings.name, cx))
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
            .context_menu(move |menu, _, _| {
                queue_menu(menu, &shown, index, total, order.clone(), main.clone())
            })
            .into_any_element()
    }

    /// A queue's marks: whether its rules are on, if it has rules, and the saved proxy it uses.
    fn marks(&self, queue: &Queue, cx: &App) -> (Option<bool>, Option<String>) {
        let rules = match &queue.settings.add_downloads {
            AddDownloads::Auto(rules) => Some(rules.enabled),
            AddDownloads::Manual => None,
        };
        let proxy = match queue.settings.proxy.own_ref() {
            Some(ProxyUse::Server(id)) => self
                .library
                .read(cx)
                .proxies
                .iter()
                .find(|server| server.id == *id)
                .map(|server| server.settings.name.clone()),
            _ => None,
        };
        (rules, proxy)
    }

    /// What a queue is doing right now, and when idle how many it may run at once when it has
    /// its own limit.
    pub(super) fn describe(&self, queue: &Queue, cx: &App) -> String {
        let rows: Vec<_> = self
            .library
            .read(cx)
            .downloads
            .iter()
            .filter(|row| row.queue == queue.id)
            .collect();
        let running = rows
            .iter()
            .filter(|row| Group::of(row.status) == Group::Downloading)
            .count();
        let waiting = rows
            .iter()
            .filter(|row| row.status == DownloadStatus::Queued)
            .count();
        let paused = rows
            .iter()
            .filter(|row| row.status == DownloadStatus::Paused)
            .count();
        let at_once = match queue.settings.max_concurrent {
            Setting::Own(limit) => Some(limit),
            Setting::Global => None,
        };
        if queue.status.paused {
            return "Paused".to_owned();
        }
        if let Some(schedule) = outside_schedule(queue) {
            return format!("Starts at {}", text::time(schedule));
        }
        if queue.status.running_outside_schedule {
            return "Started manually, ignoring schedule".to_owned();
        }
        match (running, waiting, paused, at_once) {
            (running, ..) if running > 0 => match at_once {
                Some(limit) => format!("{running} running, at most {limit}"),
                None => format!("{running} running"),
            },
            (_, waiting, ..) if waiting > 0 => format!("{waiting} waiting"),
            (_, _, paused, _) if paused > 0 => format!("{paused} paused"),
            (.., Some(limit)) => format!("Idle, up to {limit} at once"),
            _ => "Idle".to_owned(),
        }
    }
}

/// A queue's drag and drops: it drags to a new place, the default queue staying first, and takes
/// queues dropped before it and downloads dropped into it.
fn drops(
    item: gpui_kit::Stateful<gpui_kit::Div>,
    id: QueueId,
    index: usize,
    name: &str,
    cx: &mut Context<'_, MainWindow>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    item.when(index > 0, |item| {
        item.on_drag(
            DraggedQueue::new(id, name.to_owned()),
            |dragged, _, _, cx| gpui_kit::AppContext::new(cx, |_| dragged.clone()),
        )
    })
    .drag_over::<DraggedRows>(|style, _, _, cx| drag::drop_target(style, cx))
    .drag_over::<DraggedQueue>(|style, _, _, cx| drag::drop_target(style, cx))
    .on_drop(move |dragged: &DraggedRows, window, cx| {
        MainWindow::drop_rows_on_queue(id, dragged, window, cx);
    })
    .on_drop(
        cx.listener(move |this, dragged: &DraggedQueue, window, cx| {
            this.drop_queue_on(id, dragged, window, cx);
        }),
    )
}

/// A queue's tile: paused, downloading, waiting for its hours, or holding.
fn queue_tile(queue: &Queue, facts: &QueueFacts, behind: Hsla, cx: &App) -> QueueTile {
    let theme = cx.theme();
    if queue.status.paused {
        QueueTile::new(IconName::Pause, behind)
    } else if facts.running > 0 {
        QueueTile::new(IconName::Play, behind).tone(theme.primary)
    } else if outside_schedule(queue).is_some() {
        QueueTile::new(Icon::new(Lucide::Moon), behind).tone(theme.warning)
    } else {
        QueueTile::new(Icon::new(Lucide::ListOrdered), behind)
    }
}

/// A queue's text: its name with a failure dot and its speed or count, its line with the speed
/// graph, and while it downloads its progress.
fn queue_body(
    name: &str,
    facts: &QueueFacts,
    (alarm, line): (Option<String>, String),
    graph: Option<Vec<u64>>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let figure = if facts.speed > 0 {
        Some((text::speed(facts.speed), theme.primary))
    } else {
        (facts.count > 0).then(|| (facts.count.to_string(), theme.muted_foreground))
    };
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(3.))
        .child(
            h_flex()
                .gap_1p5()
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_role(Role::Strong, cx)
                        .child(name.to_owned()),
                )
                .when(facts.failed > 0, |line| {
                    line.child(div().size(px(6.)).rounded_full().bg(theme.danger))
                })
                .when_some(figure, |line, (figure, color)| {
                    line.child(
                        div()
                            .text_role(Role::Meta, cx)
                            .text_color(color)
                            .child(figure),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .text_role(Role::Meta, cx)
                .when_some(alarm, |line, alarm| {
                    line.child(div().text_color(theme.danger).child(alarm))
                })
                .child(div().flex_1().truncate().child(line))
                .when_some(graph, |line, speeds| {
                    line.child(
                        speed_graph(speeds, theme.primary)
                            .w(px(48.))
                            .h(px(14.))
                            .flex_shrink_0(),
                    )
                }),
        )
        .when_some(
            facts.per_mille.filter(|_| facts.running > 0),
            |body, done| {
                body.child(
                    div()
                        .mt_1()
                        .h(px(3.))
                        .rounded_full()
                        .bg(theme.secondary)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(theme.primary)
                                .w(relative(f32::from(done) / 1000.)),
                        ),
                )
            },
        )
}

/// What a queue's row tells, counted from its downloads.
#[derive(Default)]
struct QueueFacts {
    count: usize,
    running: usize,
    waiting: usize,
    completed: usize,
    failed: usize,
    speed: u64,
    /// Bytes left of what runs and waits, when every size is known.
    left: Option<u64>,
    /// How much of what runs and waits is done.
    per_mille: Option<u16>,
}

impl QueueFacts {
    fn of<'a>(rows: impl Iterator<Item = &'a DownloadSummary>) -> Self {
        let mut facts = Self::default();
        let (mut done, mut size, mut sized) = (0_u64, 0_u64, true);
        for row in rows {
            facts.count += 1;
            facts.speed += row.speed;
            let group = Group::of(row.status);
            match row.status {
                DownloadStatus::Completed => facts.completed += 1,
                DownloadStatus::Failed => facts.failed += 1,
                DownloadStatus::Queued => facts.waiting += 1,
                _ if group == Group::Downloading => facts.running += 1,
                _ => {}
            }
            if group == Group::Downloading || row.status == DownloadStatus::Queued {
                match row.total {
                    Some(total) => {
                        size += total;
                        done += row.done.min(total);
                    }
                    None => sized = false,
                }
            }
        }
        if sized && size > 0 {
            facts.left = Some(size - done);
            facts.per_mille = u16::try_from(u128::from(done) * 1000 / u128::from(size)).ok();
        }
        facts
    }
}

/// A queue's line: what failed, in red, then what it is doing, and when quiet what sets it apart.
fn summary(queue: &Queue, facts: &QueueFacts, proxy: Option<&str>) -> (Option<String>, String) {
    let mark = || {
        if let Some(proxy) = proxy {
            Some(format!("via {proxy}"))
        } else if let Setting::Own(limit) = queue.settings.speed_limit {
            Some(format!("{} cap", text::speed(limit)))
        } else if let Setting::Own(limit) = queue.settings.max_concurrent {
            Some(format!("{limit} at once"))
        } else {
            None
        }
    };
    let marked = |state: String| match mark() {
        Some(mark) => format!("{state} · {mark}"),
        None => state,
    };
    if queue.status.paused {
        let line = if facts.count == 0 {
            "Paused".to_owned()
        } else {
            format!("Paused · {} of {} done", facts.completed, facts.count)
        };
        return (None, line);
    }
    if facts.running > 0 {
        let of = format!("{} of {}", facts.running, facts.running + facts.waiting);
        let left = facts
            .left
            .filter(|_| facts.speed > 0)
            .map(|left| format!(" · {} left", text::eta(left / facts.speed)));
        return (None, format!("{of}{}", left.unwrap_or_default()));
    }
    if let Some(start) = outside_schedule(queue) {
        let line = marked(format!("Starts {}", text::time(start)));
        return (None, line);
    }
    if facts.waiting > 0 {
        return (None, format!("{} waiting", facts.waiting));
    }
    if facts.failed > 0 {
        return (
            Some(format!("{} failed", facts.failed)),
            format!("· {} done", facts.completed),
        );
    }
    let state = match facts.count {
        0 => "Empty".to_owned(),
        count if facts.completed == count => "All done".to_owned(),
        count => format!("{} of {count} done", facts.completed),
    };
    (None, marked(state))
}

/// When a queue outside its schedule starts next; `None` while it may run.
pub(super) fn outside_schedule(queue: &Queue) -> Option<wirecatch_protocol::app::TimeOfDay> {
    let schedule = queue.settings.schedule.as_ref()?;
    (!queue.status.running_outside_schedule
        && !schedule::allows(schedule, Local::now().naive_local()))
    .then_some(schedule.start)
}

/// A queue's own menu, opened by right-clicking it: start now outside its hours, pause or
/// resume, moving it, the default queue staying first (D72.9), and deleting it.
fn queue_menu(
    menu: PopupMenu,
    queue: &Queue,
    index: usize,
    total: usize,
    order: Vec<QueueId>,
    main: WeakEntity<MainWindow>,
) -> PopupMenu {
    let id = queue.id;
    let paused = queue.status.paused;
    let outside = outside_schedule(queue).is_some() && !paused;
    let last = total.saturating_sub(1);
    let moved = move |to: usize| {
        let mut order = order.clone();
        let queue = order.remove(index);
        order.insert(to, queue);
        order
    };
    let up = moved.clone();
    let delete = main.clone();
    menu.item(
        PopupMenuItem::new("Edit queue")
            .icon(Icon::new(Lucide::Pencil))
            .on_click(move |_, window, cx| {
                _ = main.update(cx, |main, cx| main.edit_queue(Some(id), window, cx));
            }),
    )
    .separator()
    .when(outside, |menu| {
        menu.item(
            PopupMenuItem::new("Start now (ignore schedule)")
                .icon(IconName::Play)
                .on_click(move |_, window, cx| {
                    act(window.window_handle(), cx, move |manager| {
                        api::start_queue_now(manager, id)
                    });
                }),
        )
    })
    .item(
        PopupMenuItem::new(if paused {
            "Resume the queue"
        } else {
            "Pause the queue"
        })
        .icon(if paused {
            IconName::Play
        } else {
            IconName::Pause
        })
        .on_click(move |_, window, cx| {
            act(window.window_handle(), cx, move |manager| {
                api::set_queue_paused(manager, id, !paused)
            });
        }),
    )
    .when(index > 0 && last > 1, PopupMenu::separator)
    .when(index > 1, |menu| {
        menu.item(reorder(
            "Move up",
            Icon::new(Lucide::ArrowUp),
            up(index - 1),
        ))
    })
    .when(index > 0 && index < last, |menu| {
        menu.item(reorder(
            "Move down",
            Icon::new(IconName::ArrowDown),
            moved(index + 1),
        ))
    })
    .when(!queue.is_default, |menu| {
        menu.separator().item(
            PopupMenuItem::new("Delete queue")
                .icon(Icon::new(Lucide::Trash))
                .on_click(move |_, window, cx| {
                    _ = delete.update(cx, |main, cx| main.delete_queue(id, window, cx));
                }),
        )
    })
}

/// A menu item that puts the queues in `order`.
fn reorder(label: &'static str, icon: Icon, order: Vec<QueueId>) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, window, cx| {
            let order = order.clone();
            act(window.window_handle(), cx, move |manager| {
                api::set_queue_order(manager, &order)
            });
        })
}

pub(super) fn group_icon(group: Group) -> Icon {
    match group {
        Group::Attention => Icon::new(IconName::CircleAlert),
        Group::Downloading => Icon::new(Lucide::Download),
        Group::UpNext => Icon::new(Lucide::Clock),
        Group::Paused => Icon::new(Lucide::CirclePause),
        Group::Finished => Icon::new(IconName::CircleCheck),
        Group::Cancelled => Icon::new(Lucide::Ban),
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::app::{QueueSettings, QueueStatus};

    use super::*;

    fn queue(paused: bool) -> Queue {
        Queue {
            id: QueueId(2),
            is_default: false,
            settings: QueueSettings::new("Night"),
            status: QueueStatus {
                paused,
                ..QueueStatus::default()
            },
        }
    }

    #[test]
    fn a_running_queue_tells_what_runs_and_the_time_left() {
        let facts = QueueFacts {
            count: 5,
            running: 2,
            waiting: 3,
            speed: 1000,
            left: Some(240_000),
            ..QueueFacts::default()
        };
        assert_eq!(
            summary(&queue(false), &facts, None),
            (None, "2 of 5 · 4 min left".to_owned())
        );
    }

    #[test]
    fn a_quiet_queue_tells_failures_first_and_else_what_sets_it_apart() {
        let failed = QueueFacts {
            count: 4,
            completed: 3,
            failed: 1,
            ..QueueFacts::default()
        };
        assert_eq!(
            summary(&queue(false), &failed, None),
            (Some("1 failed".to_owned()), "· 3 done".to_owned())
        );
        let done = QueueFacts {
            count: 3,
            completed: 3,
            ..QueueFacts::default()
        };
        assert_eq!(
            summary(&queue(false), &done, Some("Home")).1,
            "All done · via Home"
        );
        assert_eq!(summary(&queue(true), &done, None).1, "Paused · 3 of 3 done");
    }
}
