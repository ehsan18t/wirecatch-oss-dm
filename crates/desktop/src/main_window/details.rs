//! One download's details, in a dialog over the list that keeps updating (D47): what is happening
//! now, the short facts, where it is saved and where it came from, and what happened to it, with
//! its actions at the foot.

use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use wirecatch_core::{ActivityEvent, Happened};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction, PER_MILLE_DONE};
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use super::applies::RowAction;
use super::list::status_tone;
use super::{Group, applies, confirm, fixes, refresh_dialog};
use crate::backend::{Backend, act};
use crate::library::Library;
use crate::ui::layout::{self, Rule};
use crate::ui::speed_graph::{SpeedHistory, speed_graph};
use crate::ui::{Callout, PathText, Role, Typed as _, tokens};
use crate::{files, text};

const WIDTH: f32 = 600.;

/// The label column of Saved to and From.
const PLACE_LABEL_WIDTH: f32 = 112.;

/// The date column of Activity.
const ACTIVITY_LABEL_WIDTH: f32 = 132.;

/// The tallest the details grow before their middle scrolls.
const MAX_HEIGHT: f32 = 640.;

/// The details of one download, kept current while the dialog is open.
pub struct Details {
    library: Entity<Library>,
    id: DownloadId,
    /// The speeds seen while the dialog is open, oldest first, for the graph.
    speeds: SpeedHistory,
    /// What happened to it, read from the manager whenever `changes` do.
    activity: Vec<Happened>,
    changes: Option<Changes>,
    _subscriptions: Vec<Subscription>,
}

/// What the activity records, so a change to any of it reads the activity again.
#[derive(PartialEq, Eq)]
struct Changes {
    status: DownloadStatus,
    url: String,
    path: String,
}

impl Changes {
    fn of(row: &DownloadSummary) -> Self {
        Self {
            status: row.status,
            url: row.url.clone(),
            path: row.path.clone(),
        }
    }
}

impl Details {
    fn read_activity(&mut self, cx: &mut Context<'_, Self>) {
        let id = self.id;
        let read = Backend::run(
            cx,
            move |manager| async move { api::activity(&manager, id) },
        );
        cx.spawn(async move |details, cx| {
            let activity = match read.await {
                Ok(Ok(activity)) => activity,
                Ok(Err(unopened)) => {
                    tracing::debug!(
                        ?unopened,
                        "no activity to show: the download list is not open"
                    );
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "reading a download's activity failed");
                    return;
                }
            };
            let shown = details.update(cx, |details, cx| {
                details.activity = activity;
                cx.notify();
            });
            if shown.is_err() {
                tracing::debug!("the details closed before their activity was read");
            }
        })
        .detach();
    }
}

const GRAPH_HEIGHT: f32 = 40.;

/// Opens the details of download `id` in a dialog over `window`. It closes by itself once the
/// download is removed.
pub(super) fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let details = cx.new(|cx| {
        let follow = cx.observe_in(
            &library,
            window,
            |details: &mut Details, library, window, cx| {
                let row = library
                    .read(cx)
                    .downloads
                    .find(details.id)
                    .map(|row| (row.status, row.speed, Changes::of(row)));
                match row {
                    Some((status, speed, changes)) => {
                        if Group::of(status) == Group::Downloading {
                            details.speeds.keep(speed, Instant::now());
                        }
                        if details.changes.as_ref() != Some(&changes) {
                            details.changes = Some(changes);
                            details.read_activity(cx);
                        }
                        cx.notify();
                    }
                    None => window.close_dialog(cx),
                }
            },
        );
        let mut details = Details {
            changes: library.read(cx).downloads.find(id).map(Changes::of),
            library,
            id,
            speeds: SpeedHistory::default(),
            activity: Vec::new(),
            _subscriptions: vec![follow],
        };
        details.read_activity(cx);
        details
    });
    crate::ui::dialog::open(window, cx, move |dialog, _, _| {
        dialog.w(px(WIDTH)).p_0().child(details.clone())
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let Some(row) = library.downloads.find(self.id).cloned() else {
            return div().into_any_element();
        };
        let queue = library
            .queue(row.queue)
            .map_or_else(|| "Its queue".to_owned(), |q| q.settings.name.clone());
        let others: Vec<(QueueId, String)> = library
            .queues
            .iter()
            .filter(|q| q.id != row.queue)
            .map(|q| (q.id, q.settings.name.clone()))
            .collect();
        let speeds = self.speeds.speeds();
        v_flex()
            .id("details")
            .max_h(px(MAX_HEIGHT))
            .text_sm()
            .child(header(&row, cx))
            .child(
                v_flex()
                    .id("details-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(now_section(&row, &queue, &speeds, cx))
                    .child(facts(&row, &queue, &others, cx))
                    .child(places(&row, self.library.clone(), cx))
                    .child(activity(&row, &self.activity, cx)),
            )
            .child(footer(&row, self.library.clone(), cx))
            .into_any_element()
    }
}

/// The name, its status and where it comes from.
fn header(row: &DownloadSummary, cx: &mut Context<'_, Details>) -> impl IntoElement {
    let tone = status_tone(row, cx);
    layout::bar(Rule::Below, cx).items_start().child(
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .text_role(Role::Title, cx)
                    .child(row.file_name.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_role(Role::Meta, cx)
                    .child(
                        h_flex()
                            .gap_1()
                            .text_color(tone)
                            .child(div().size(px(6.)).rounded_full().bg(tone))
                            .child(text::status_name(row)),
                    )
                    .child(div().child(text::host(&row.url).to_owned())),
            ),
    )
}

/// What is happening now: the problem and its fix, or why it waits; the speed and progress.
fn now_section(
    row: &DownloadSummary,
    queue: &str,
    speeds: &[u64],
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let failed = match (row.status, &row.error) {
        (DownloadStatus::Failed, Some(error)) => {
            Some(failure(row.id, &error.message, error.fix, cx))
        }
        _ => None,
    };
    let theme = cx.theme();
    let live = Group::of(row.status) == Group::Downloading;
    let mut section = layout::section(Rule::None, cx).gap_3();
    if let Some(failed) = failed {
        section = section.child(failed);
    } else if let Some(detail) = text::status_detail(row, queue) {
        section = section.child(
            div()
                .p_3()
                .rounded(theme.radius)
                .bg(theme.muted)
                .text_role(Role::Body, cx)
                .child(detail),
        );
    }
    if live && speeds.len() > 1 {
        section = section.child(
            speed_graph(speeds.to_vec(), cx.theme().primary)
                .w_full()
                .h(px(GRAPH_HEIGHT)),
        );
    }
    if row.done > 0 || live || row.status == DownloadStatus::Completed {
        let size = match (row.status, text::percent(row)) {
            (DownloadStatus::Completed, _) => text::size(row),
            (_, Some(percent)) if row.done > 0 => format!("{} · {percent}", text::size(row)),
            _ => text::size(row),
        };
        section = section.child(
            h_flex()
                .gap_3()
                .items_baseline()
                .when(live && row.speed > 0, |line| {
                    line.child(
                        div()
                            .text_role(Role::Figure, cx)
                            .child(text::speed(row.speed)),
                    )
                })
                .child(div().text_role(Role::Secondary, cx).child(size))
                .when_some(
                    row.eta_secs.filter(|_| live && row.speed > 0),
                    |line, left| {
                        line.child(
                            div()
                                .ml_auto()
                                .text_role(Role::Secondary, cx)
                                .child(format!("{} left", text::eta(left))),
                        )
                    },
                ),
        );
    }
    if live || row.status == DownloadStatus::Paused || row.status == DownloadStatus::Failed {
        let per_mille = text::per_mille(row);
        section = section.child(
            Progress::new("details-progress")
                .color(status_tone(row, cx))
                .loading(per_mille.is_none())
                .value(per_mille.map_or(0., |done| {
                    f32::from(done) * 100. / f32::from(PER_MILLE_DONE)
                })),
        );
    }
    if let Some(media) = &row.media {
        section = section.child(
            div()
                .text_role(Role::Secondary, cx)
                .child(text::media_line(media)),
        );
    }
    section
}

/// The short facts in one strip, each one the rest of the dialog does not already say. With other
/// queues, the queue is a menu that moves the download.
fn facts(
    row: &DownloadSummary,
    queue: &str,
    others: &[(QueueId, String)],
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let not_started = row.done == 0
        && row.status != DownloadStatus::Completed
        && Group::of(row.status) != Group::Downloading;
    let mut facts: Vec<(&'static str, String)> = vec![("Queue", queue.to_owned())];
    if let Some(finished) = row.completed_at {
        facts.push(("Finished", text::date(finished)));
    }
    if not_started {
        facts.push((
            "Size",
            row.total.map_or_else(
                || "Unknown".to_owned(),
                wirecatch_shell::activity::format_bytes,
            ),
        ));
    }
    facts.push(("Kind", text::file_kind(&row.file_name, row.category)));
    facts.push(("Added", text::date(row.added_at)));
    if let Some(proxy) = &row.proxy {
        facts.push(("Proxy", proxy.clone()));
    }
    h_flex()
        .flex_wrap()
        .border_y_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .children(
            facts
                .into_iter()
                .enumerate()
                .map(|(index, (label, value))| {
                    v_flex()
                        .flex_1()
                        .min_w(px(96.))
                        .px_4()
                        .py_2p5()
                        .gap_0p5()
                        .when(index > 0, |fact| {
                            fact.border_l_1().border_color(theme.border)
                        })
                        .child(div().text_role(Role::Meta, cx).child(label))
                        .child(if label == "Queue" && !others.is_empty() {
                            queue_picker(row.id, value, others.to_vec()).into_any_element()
                        } else {
                            div()
                                .text_role(Role::Strong, cx)
                                .child(value)
                                .into_any_element()
                        })
                }),
        )
}

/// The download's queue as a menu of the others, which moves it.
fn queue_picker(id: DownloadId, queue: String, others: Vec<(QueueId, String)>) -> impl IntoElement {
    Button::new("details-queue")
        .xsmall()
        .outline()
        .label(queue)
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _, _| {
            others.iter().fold(menu, |menu, (other, name)| {
                let other = *other;
                menu.item(PopupMenuItem::new(format!("Move to {name}")).on_click(
                    move |_, window, cx| {
                        act(window.window_handle(), cx, move |manager| {
                            api::move_to_queue(manager, &[id], other)
                        });
                    },
                ))
            })
        })
}

/// Whether the details offer `fix`: Try again stands beside it, and the queue field is right here.
fn offered_here(fix: FixAction) -> bool {
    !matches!(fix, FixAction::Retry | FixAction::ChangeQueue)
}

/// Why it failed, with its fix and Try again.
fn failure(
    id: DownloadId,
    message: &str,
    fix: Option<FixAction>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement + use<> {
    Callout::danger(message.to_owned())
        .when_some(fix.filter(|fix| offered_here(*fix)), |callout, fix| {
            callout.action(
                Button::new("details-fix")
                    .small()
                    .primary()
                    .label(text::fix_label(fix))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        fixes::run(id, fix, window, cx);
                    })),
            )
        })
        .action(
            Button::new("details-retry")
                .small()
                .outline()
                .label("Try again")
                .on_click(cx.listener(move |_, _, window, cx| {
                    act(window.window_handle(), cx, move |manager| {
                        api::retry(manager, id)
                    });
                })),
        )
}

/// Puts downloads' `links` on the clipboard, one per line, and says so.
pub(super) fn copy_links(links: &[String], window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(links.join("\n")));
    window.push_notification(
        if links.len() == 1 {
            "Link copied."
        } else {
            "Links copied."
        },
        cx,
    );
}

/// Where it is saved, with Change until it finishes, and where it came from, with Copy.
fn places(
    row: &DownloadSummary,
    library: Entity<Library>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = row.id;
    let link = row.url.clone();
    let copied = link.clone();
    layout::section(Rule::None, cx)
        .text_sm()
        .child(place(
            IconName::FolderOpen,
            "Saved to",
            PathText::new(row.path.clone())
                .tooltip("details-saved-to")
                .into_any_element(),
            (row.status != DownloadStatus::Completed).then(|| {
                Button::new("details-change-folder")
                    .xsmall()
                    .outline()
                    .label("Change")
                    .on_click(cx.listener(move |_, _, window, cx| {
                        fixes::choose_folder(id, false, window, cx);
                    }))
                    .into_any_element()
            }),
            cx,
        ))
        .child(place(
            IconName::Globe,
            "From",
            div().truncate().child(link).into_any_element(),
            Some(
                Button::new("details-copy-link")
                    .xsmall()
                    .outline()
                    .label("Copy")
                    .on_click(move |_, window, cx| {
                        copy_links(std::slice::from_ref(&copied), window, cx);
                    })
                    .into_any_element(),
            ),
            cx,
        ))
        .when(applies::link_may_have_expired(row), |list| {
            list.child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme.muted_foreground)
                            .child("Stuck? The link may have expired."),
                    )
                    .child(
                        Button::new("details-new-link")
                            .xsmall()
                            .outline()
                            .label("Get a new link")
                            .on_click(move |_, window, cx| {
                                refresh_dialog::open(library.clone(), id, window, cx);
                            }),
                    ),
            )
        })
}

fn place(
    icon: IconName,
    label: &'static str,
    value: AnyElement,
    action: Option<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    layout::label_row(
        PLACE_LABEL_WIDTH,
        h_flex()
            .gap_1p5()
            .child(Icon::new(icon).with_size(tokens::ICON_SMALL))
            .child(label),
        div().min_w_0().text_role(Role::Strong, cx).child(value),
        cx,
    )
    .py_2()
    .children(action)
}

/// What happened to it.
fn activity(row: &DownloadSummary, happened: &[Happened], cx: &App) -> impl IntoElement {
    let mut lines = vec![(row.added_at, format!("Added from {}", text::host(&row.url)))];
    lines.extend(happened.iter().map(|h| (h.at, said(&h.event))));
    if let Some(finished) = row.completed_at {
        lines.push((finished, "Finished".to_owned()));
    }
    lines.sort_by_key(|(at, _)| *at);
    layout::section(Rule::Above, cx)
        .gap_2()
        .child(div().text_role(Role::Label, cx).child("Activity"))
        .children(lines.into_iter().map(|(at, what)| {
            layout::label_row(ACTIVITY_LABEL_WIDTH, text::date(at), what, cx).items_start()
        }))
}

/// One line of the activity, as the person reads it.
fn said(event: &ActivityEvent) -> String {
    match event {
        ActivityEvent::Failed { message } if message.is_empty() => "Failed".to_owned(),
        ActivityEvent::Failed { message } => format!("Failed: {message}"),
        ActivityEvent::Paused => "Paused".to_owned(),
        ActivityEvent::Resumed => "Resumed".to_owned(),
        ActivityEvent::Retried => "Tried again".to_owned(),
        ActivityEvent::Cancelled => "Cancelled".to_owned(),
        ActivityEvent::LinkChanged { host } => format!("Link changed to {host}"),
        ActivityEvent::Moved { folder } => format!("Moved to {folder}"),
    }
}

/// The actions at the foot: quiet icon buttons on the left, and the one action its state calls for
/// on the right.
fn footer(
    row: &DownloadSummary,
    library: Entity<Library>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let id = row.id;
    let main = RowAction::offered(row.status).first().cloned();
    let cancellable = applies::can_cancel(row);
    let quiet = |name: &'static str, icon: IconName, tip: &'static str| {
        Button::new(name).ghost().icon(icon).tooltip(tip)
    };
    let cancelled = library.clone();
    layout::bar(Rule::Above, cx)
        .when(applies::has_file(row), |bar| {
            bar.child(
                quiet("details-folder", IconName::FolderOpen, "Show in folder")
                    .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            )
        })
        .child(
            quiet(
                "details-window",
                IconName::PanelBottomOpen,
                "Show progress window",
            )
            .on_click(move |_, _, cx| crate::download_windows::show_download(id, cx)),
        )
        .when(cancellable, |bar| {
            bar.child(
                quiet("details-cancel", IconName::CircleX, "Cancel download").on_click(
                    move |_, window, cx| confirm::cancel(&cancelled, &[id], &[], window, cx),
                ),
            )
        })
        .child(
            quiet("details-remove", IconName::Delete, "Remove")
                .on_click(move |_, window, cx| confirm::remove(&library, &[id], &[], window, cx)),
        )
        .child(div().flex_1())
        .when_some(main, |bar, (action, label, icon)| {
            bar.child(
                Button::new("details-main")
                    .primary()
                    .icon(icon)
                    .label(label)
                    .on_click(cx.listener(move |_, _, window, cx| action.run(id, window, cx))),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_details_do_not_offer_a_fix_that_would_reopen_them() {
        assert!(!offered_here(FixAction::ChangeQueue));
        assert!(!offered_here(FixAction::Retry));
        assert!(offered_here(FixAction::ChooseFolder));
    }
}
