//! Above a queue's own list, on one line: its name, what its rules add, when it runs and its
//! limits, what is happening now, and its actions (Rules on or off for a queue with rules, Start
//! now outside its hours, Pause or Resume, Edit); its description under it. Alt+Up and Alt+Down
//! move the selection in its line.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, IntoElement, ParentElement as _, Styled as _, Window, div, relative,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadSummary, Queue, QueueSettings};
use wirecatch_protocol::rules::AddDownloads;
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
        let header = v_flex()
            .px_5()
            .pt_3()
            .pb_1()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(relative(0.4))
                            .truncate()
                            .text_role(Role::PageTitle, cx)
                            .child(queue.settings.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_role(Role::Meta, cx)
                            .child(format!("{facts} · {summary}")),
                    )
                    .child(queue_actions(&queue, cx)),
            )
            .when(!queue.settings.description.is_empty(), |header| {
                header.child(
                    div()
                        .truncate()
                        .text_role(Role::Secondary, cx)
                        .child(queue.settings.description.clone()),
                )
            });
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
            order::apply(&self.library, queue, order, window, cx);
        }
    }
}

/// Whether a queue's rules are on; `None` when it has none.
pub(super) fn rules_on(queue: &Queue) -> Option<bool> {
    match &queue.settings.add_downloads {
        AddDownloads::Auto(rules) => Some(rules.enabled),
        AddDownloads::Manual => None,
    }
}

/// Turns a queue's rules on or off; off keeps them for when they are on again (D72.4).
pub(super) fn set_rules(queue: &Queue, on: bool, window: &Window, cx: &mut App) {
    let mut settings = queue.settings.clone();
    let AddDownloads::Auto(rules) = &mut settings.add_downloads else {
        return;
    };
    rules.enabled = on;
    let id = queue.id;
    act(window.window_handle(), cx, move |manager| {
        api::update_queue(manager, id, settings)
    });
}

/// Rules on or off, Start now outside the queue's hours, Pause or Resume, and Edit.
fn queue_actions(queue: &Queue, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    let id = queue.id;
    let outside = sidebar::outside_schedule(queue).is_some() && !queue.status.paused;
    let paused = queue.status.paused;
    h_flex()
        .flex_shrink_0()
        .gap_2()
        .when_some(rules_on(queue), |bar, on| {
            let queue = queue.clone();
            bar.child(
                Button::new("queue-rules")
                    .small()
                    .outline()
                    .when(on, ButtonVariants::primary)
                    .icon(Icon::new(if on { Lucide::Zap } else { Lucide::ZapOff }))
                    .label(if on { "Rules on" } else { "Rules off" })
                    .tooltip(if on {
                        "New downloads that match this queue's rules go here. Click to turn them off."
                    } else {
                        "New downloads skip this queue. Click to turn its rules on."
                    })
                    .on_click(move |_, window, cx| set_rules(&queue, !on, window, cx)),
            )
        })
        .when(outside, |bar| {
            bar.child(
                Button::new("queue-start-now")
                    .small()
                    .outline()
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
                .outline()
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
                .outline()
                .icon(Icon::new(Lucide::Pencil))
                .label("Edit queue")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_queue(Some(id), window, cx);
                })),
        )
}

/// What its rules add, when it runs and its own limits, said once.
fn facts(queue: &Queue) -> String {
    let when = if queue.status.paused {
        "Paused".to_owned()
    } else {
        text::schedule(queue.settings.schedule.as_ref())
    };
    let rules = match rules_on(queue) {
        Some(false) => Some("New downloads skip this queue".to_owned()),
        _ => text::rules_fact(&queue.settings.add_downloads),
    };
    rules
        .into_iter()
        .chain(std::iter::once(when))
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
