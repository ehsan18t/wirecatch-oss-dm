//! Above a queue's own list: its name and description, when it runs and its limits, one line on
//! what is happening now, and the queue's actions: Start now outside its hours, Pause or Resume,
//! Edit. Alt+Up and Alt+Down move the selection in its line.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Styled as _, Window, div,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadSummary, Queue, QueueSettings};
use wirecatch_shell::api;

use super::{Group, MainWindow, View, order, sidebar};
use crate::backend::act;
use crate::text;
use crate::ui::{Role, Typed as _};

impl MainWindow {
    /// The header of a queue's own view; `None` in other views.
    pub(super) fn queue_header(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let View::Queue(id) = self.view else {
            return None;
        };
        let queue = self.library.read(cx).queue(id)?.clone();
        let facts = facts(&queue);
        let summary = self.summary(&queue, cx);
        let header = h_flex()
            .px_5()
            .pt_4()
            .pb_2()
            .gap_4()
            .items_end()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .truncate()
                            .text_role(Role::PageTitle, cx)
                            .child(queue.settings.name.clone()),
                    )
                    .when(!queue.settings.description.is_empty(), |text| {
                        text.child(
                            div()
                                .truncate()
                                .text_role(Role::Body, cx)
                                .child(queue.settings.description.clone()),
                        )
                    })
                    .child(div().truncate().text_role(Role::Meta, cx).child(facts))
                    .child(div().truncate().text_role(Role::Meta, cx).child(summary)),
            )
            .child(queue_actions(&queue, cx));
        Some(header.into_any_element())
    }

    /// What is happening in the queue now: how many download and about how long the longest has
    /// left, and how many are up next; else how many it holds; or what a search found.
    fn summary(&self, queue: &Queue, cx: &App) -> String {
        let downloads = &self.library.read(cx).downloads;
        let rows: Vec<&DownloadSummary> = downloads
            .iter()
            .filter(|row| row.queue == queue.id)
            .collect();
        let needle = self.needle(cx);
        if !needle.is_empty() {
            let found = rows
                .iter()
                .filter(|row| {
                    row.file_name.to_lowercase().contains(&needle)
                        || row.url.to_lowercase().contains(&needle)
                })
                .count();
            return format!("{} matching \u{201c}{needle}\u{201d}", count(found));
        }
        let downloading: Vec<&&DownloadSummary> = rows
            .iter()
            .filter(|row| Group::of(row.status) == Group::Downloading)
            .collect();
        let waiting = rows
            .iter()
            .filter(|row| Group::of(row.status) == Group::UpNext)
            .count();
        let next = (waiting > 0).then(|| format!("{waiting} up next"));
        if downloading.is_empty() {
            return next.map_or_else(
                || count(rows.len()),
                |next| format!("Nothing downloading · {next}"),
            );
        }
        let longest = downloading.iter().filter_map(|row| row.eta_secs).max();
        [
            Some(format!("{} downloading", downloading.len())),
            longest.map(|left| format!("about {} left", text::eta(left))),
            next,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }

    /// Alt+Up and Alt+Down: the selection moves one place in its queue's line.
    pub(super) fn nudge(&mut self, up: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        let View::Queue(queue) = self.view else {
            return;
        };
        let downloads = &self.library.read(cx).downloads;
        let line: Vec<DownloadId> = self
            .queue_line(cx)
            .into_iter()
            .map(|index| downloads[index].id)
            .collect();
        let chosen: Vec<DownloadId> = self.selected.iter().copied().collect();
        if let Some(order) = order::nudged(&line, &chosen, up) {
            act(window.window_handle(), cx, move |manager| {
                api::reorder(manager, queue, &order)
            });
        }
    }
}

/// Start now outside the queue's hours, Pause or Resume, and Edit.
fn queue_actions(queue: &Queue, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    let id = queue.id;
    let outside = sidebar::outside_schedule(queue).is_some() && !queue.status.paused;
    let paused = queue.status.paused;
    h_flex()
        .flex_shrink_0()
        .gap_1()
        .when(outside, |bar| {
            bar.child(
                Button::new("queue-start-now")
                    .small()
                    .ghost()
                    .icon(IconName::Play)
                    .label("Start now")
                    .on_click(move |_, window, cx| {
                        act(window.window_handle(), cx, move |manager| {
                            api::start_queue_now(manager, id)
                        });
                    }),
            )
        })
        .child(
            Button::new("queue-pause")
                .small()
                .ghost()
                .icon(if paused {
                    IconName::Play
                } else {
                    IconName::Pause
                })
                .label(if paused {
                    "Resume queue"
                } else {
                    "Pause queue"
                })
                .on_click(move |_, window, cx| {
                    act(window.window_handle(), cx, move |manager| {
                        api::set_queue_paused(manager, id, !paused)
                    });
                }),
        )
        .child(
            Button::new("queue-edit")
                .small()
                .ghost()
                .icon(Icon::new(Lucide::Pencil))
                .label("Edit queue")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_queue(Some(id), window, cx);
                })),
        )
}

/// When the queue runs and its own limits, said once.
fn facts(queue: &Queue) -> String {
    let when = if queue.status.paused {
        "Paused".to_owned()
    } else {
        text::schedule(queue.settings.schedule.as_ref())
    };
    std::iter::once(when)
        .chain(limits(&queue.settings))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A queue's own limits, one short phrase each, or that it follows the app's when it has none.
fn limits(settings: &QueueSettings) -> Vec<String> {
    let concurrent = settings.max_concurrent.own_ref().copied();
    let speed = settings.speed_limit.own_ref().copied();
    let connections = settings.connections.own_ref().copied();
    let mut facts: Vec<String> = [
        concurrent.map(|n| format!("{n} at once")),
        speed.map(|limit| format!("Up to {}", text::speed(limit))),
        connections.map(|n| match n {
            1 => "1 connection each".to_owned(),
            n => format!("{n} connections each"),
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    if facts.is_empty() {
        return vec!["Uses global limits".to_owned()];
    }
    if concurrent.is_none() || speed.is_none() || connections.is_none() {
        facts.push("Other limits are global".to_owned());
    }
    facts
}

fn count(n: usize) -> String {
    match n {
        1 => "1 download".to_owned(),
        n => format!("{n} downloads"),
    }
}
