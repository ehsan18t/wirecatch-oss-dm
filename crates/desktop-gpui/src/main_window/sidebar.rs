//! The navigation (D91): every download and each status group with their counts, then the queues,
//! each with its count or its speed while it downloads, what it is doing, and its own menu.

use chrono::Local;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{DownloadStatus, Queue, Setting};
use wirecatch_protocol::rules::AddDownloads;
use wirecatch_queues::schedule;
use wirecatch_shell::api;

use super::{Group, MainWindow, View};
use crate::components::heading;
use crate::icons::AppIcon;
use crate::text;

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
            Icon::new(AppIcon::Layers),
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
                    .child(v_flex().gap_px().children(downloads))
                    .child(queues),
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
            .child(heading::section("Queues", cx).px_2().pb_1())
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
                    .hover(|item| item.bg(theme.sidebar_accent.opacity(0.6)))
            })
            .child(
                icon.small()
                    .when(current, |icon| icon.text_color(theme.primary)),
            )
            .child(div().flex_1().truncate().child(label.into()))
            .when(count > 0, |item| {
                item.child(
                    div()
                        .text_xs()
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
        let rules = match &queue.settings.add_downloads {
            AddDownloads::Auto(rules) => Some(rules.enabled),
            AddDownloads::Manual => None,
        };
        let menu = queue_menu(queue, index, total, self.library.read(cx).queues.clone());
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
                item.hover(|item| item.bg(theme.sidebar_accent.opacity(0.6)))
            })
            .child(
                Icon::new(AppIcon::LayoutList)
                    .small()
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
                    .child(doing(rules, status, cx)),
            )
            .when(current || self.hovered_queue == Some(id), |item| {
                item.child(menu)
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_queue = Some(id);
                } else if this.hovered_queue == Some(id) {
                    this.hovered_queue = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
            .into_any_element()
    }

    /// What a queue is doing right now, and when idle how many it may run at once when it has
    /// its own limit.
    fn describe(&self, queue: &Queue, cx: &App) -> String {
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
                .when(current, |name| name.font_weight(FontWeight::MEDIUM))
                .child(name.to_owned()),
        )
        .child(div().text_xs().text_color(color).child(figure))
}

/// A queue's second line: its rules mark and what it is doing.
fn doing(rules: Option<bool>, status: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .gap_1()
        .text_xs()
        .text_color(theme.muted_foreground)
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
        .child(div().truncate().child(status))
}

/// When a queue outside its schedule starts next; `None` while it may run.
fn outside_schedule(queue: &Queue) -> Option<wirecatch_protocol::app::TimeOfDay> {
    let schedule = queue.settings.schedule.as_ref()?;
    (!queue.status.running_outside_schedule
        && !schedule::allows(schedule, Local::now().naive_local()))
    .then_some(schedule.start)
}

/// A queue's own menu: start now outside its hours, pause or resume, and moving it, the default
/// queue staying first (D72.9).
fn queue_menu(queue: &Queue, index: usize, total: usize, order: Vec<Queue>) -> impl IntoElement {
    let id = queue.id;
    let paused = queue.status.paused;
    let outside = outside_schedule(queue).is_some() && !paused;
    let last = total.saturating_sub(1);
    Button::new(ElementId::NamedInteger(
        "queue-menu".into(),
        id.0.unsigned_abs(),
    ))
    .xsmall()
    .ghost()
    .icon(IconName::Ellipsis)
    .tooltip(format!("Actions for {}", queue.settings.name))
    .dropdown_menu(move |menu, _, _| {
        let order: Vec<QueueId> = order.iter().map(|q| q.id).collect();
        let moved = move |to: usize| {
            let mut order = order.clone();
            let from = index;
            let queue = order.remove(from);
            order.insert(to, queue);
            order
        };
        let up = moved.clone();
        menu.when(outside, |menu| {
            menu.item(
                PopupMenuItem::new("Start now (ignore schedule)")
                    .icon(IconName::Play)
                    .on_click(move |_, window, cx| {
                        run(window, cx, move |manager| api::start_queue_now(manager, id));
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
                run(window, cx, move |manager| {
                    api::set_queue_paused(manager, id, !paused)
                });
            }),
        )
        .when(
            index > 0 && last > 1,
            gpui_kit::component::menu::PopupMenu::separator,
        )
        .when(index > 1, |menu| {
            let order = up(index - 1);
            menu.item(
                PopupMenuItem::new("Move up")
                    .icon(AppIcon::ArrowUp)
                    .on_click(move |_, window, cx| {
                        let order = order.clone();
                        run(window, cx, move |manager| {
                            api::set_queue_order(manager, &order)
                        });
                    }),
            )
        })
        .when(index > 0 && index < last, |menu| {
            let order = moved(index + 1);
            menu.item(
                PopupMenuItem::new("Move down")
                    .icon(IconName::ArrowDown)
                    .on_click(move |_, window, cx| {
                        let order = order.clone();
                        run(window, cx, move |manager| {
                            api::set_queue_order(manager, &order)
                        });
                    }),
            )
        })
    })
}

/// Runs a queue command; a refusal shows as a notification.
fn run(
    window: &mut gpui_kit::Window,
    cx: &mut App,
    work: impl FnOnce(&wirecatch_core::Manager) -> Result<(), wirecatch_protocol::wire::ProtocolError>
    + Send
    + 'static,
) {
    let done = crate::backend::Backend::run(cx, move |manager| async move { work(&manager) });
    window
        .spawn(cx, async move |cx| {
            let refused = match done.await {
                Ok(Ok(Ok(()))) | Err(_) => return,
                Ok(Ok(Err(error))) => error.message,
                Ok(Err(unopened)) => unopened.reason.message,
            };
            _ = cx.update(|window, cx| {
                use gpui_kit::component::WindowExt as _;
                window.push_notification(refused, cx);
            });
        })
        .detach();
}

fn group_icon(group: Group) -> Icon {
    match group {
        Group::Attention => Icon::new(IconName::CircleAlert),
        Group::Downloading => Icon::new(AppIcon::Download),
        Group::UpNext => Icon::new(AppIcon::Clock),
        Group::Paused => Icon::new(AppIcon::CirclePause),
        Group::Finished => Icon::new(IconName::CircleCheck),
        Group::Cancelled => Icon::new(AppIcon::Ban),
    }
}
