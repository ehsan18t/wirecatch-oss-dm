//! The navigation (D91): every download and each status group with their counts, then the queues,
//! each with its count or its speed while it downloads, what it is doing, and its own menu.

use chrono::Local;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity,
    div, px,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, Queue, Setting};
use wirecatch_protocol::proxy::ProxyUse;
use wirecatch_protocol::rules::AddDownloads;
use wirecatch_queues::schedule;
use wirecatch_shell::api;

use super::drag::{self, DraggedQueue, DraggedRows};
use super::{Group, MainWindow, Screen, View};
use crate::backend::act;
use crate::text;
use crate::ui::{Role, Typed as _, tokens};

pub(super) const WIDTH: f32 = 232.;
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

    /// The queues: the first few, the one on screen, and the rest behind "more queues".
    fn queues_section(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let queues = self.library.read(cx).queues.clone();
        let folded = queues.len() > QUEUES_SHOWN && !self.all_queues;
        let shown: Vec<AnyElement> = queues
            .iter()
            .enumerate()
            .filter(|(index, queue)| {
                !folded || *index < QUEUES_SHOWN || self.view == View::Queue(queue.id)
            })
            .map(|(index, queue)| self.queue_item(queue, index, queues.len(), cx))
            .collect();
        let hidden = queues.len() - shown.len();
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
        v_flex()
            .gap_px()
            .child(
                h_flex()
                    .pl_2()
                    .pb_1()
                    .justify_between()
                    .child(div().text_role(Role::Label, cx).child("Queues"))
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
            .children(shown)
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

    /// One queue: its name with its count or speed, what it is doing, and its menu.
    fn queue_item(
        &self,
        queue: &Queue,
        index: usize,
        total: usize,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let id = queue.id;
        let view = View::Queue(id);
        let current = self.view == view;
        let rows = || {
            self.library
                .read(cx)
                .downloads
                .iter()
                .filter(move |row| row.queue == id)
        };
        let speed: u64 = rows().map(|row| row.speed).sum();
        let count = rows().count();
        let status = self.describe(queue, cx);
        let (rules, proxy) = self.marks(queue, cx);
        let order: Vec<QueueId> = self.library.read(cx).queues.iter().map(|q| q.id).collect();
        let main = cx.entity().downgrade();
        let shown = queue.clone();
        let theme = cx.theme();
        h_flex()
            .id(ElementId::NamedInteger("queue".into(), id.0.unsigned_abs()))
            .px_2()
            .py_1p5()
            .gap_2()
            .items_start()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(rules == Some(false), |item| item.opacity(0.6))
            .when(current, |item| item.bg(theme.sidebar_accent))
            .when(!current, |item| {
                item.hover(|item| item.bg(theme.list_hover))
            })
            .child(
                Icon::new(Lucide::ListOrdered)
                    .with_size(tokens::ICON)
                    .mt_0p5()
                    .text_color(if current {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(name_line(&queue.settings.name, current, speed, count, cx))
                    .child(doing(queue.id, rules, proxy, status, cx)),
            )
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
            return format!("Starts at {:02}:{:02}", schedule.hour, schedule.minute);
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

/// A queue's first line: its name, with its speed while it downloads, else how many it holds.
fn name_line(name: &str, current: bool, speed: u64, count: usize, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (figure, color) = if speed > 0 {
        (text::speed(speed), theme.primary)
    } else if count > 0 {
        (count.to_string(), theme.muted_foreground)
    } else {
        (String::new(), theme.muted_foreground)
    };
    h_flex()
        .gap_2()
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(if current {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .when(current, |name| name.font_weight(FontWeight::MEDIUM))
                .child(name.to_owned()),
        )
        .child(
            div()
                .text_role(Role::Meta, cx)
                .text_color(color)
                .child(figure),
        )
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

/// A queue's second line: its rules mark, the saved proxy it uses (D73.13), and what it is doing.
fn doing(
    queue: QueueId,
    rules: Option<bool>,
    proxy: Option<String>,
    status: String,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .gap_1()
        .text_role(Role::Meta, cx)
        .when_some(rules, |line, enabled| {
            line.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.border)
                    .child(if enabled { "Rules" } else { "Rules off" }),
            )
        })
        .when_some(proxy, |line, proxy| {
            line.child(
                div()
                    .id(gpui_kit::ElementId::NamedInteger(
                        "queue-proxy".into(),
                        queue.0.unsigned_abs(),
                    ))
                    .child(Icon::new(IconName::Globe).with_size(tokens::ICON_SMALL))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(format!("Proxy: {proxy}"))
                            .build(window, cx)
                    }),
            )
        })
        .child(div().truncate().child(status))
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
