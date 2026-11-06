//! One download's details, in a dialog over the list that keeps updating (D47, D245): what is
//! happening now, the short facts, where it is saved and where it came from, and what happened to
//! it, with its labelled actions at the foot.

use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Div, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use wirecatch_core::{ActivityEvent, Happened};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction};
use wirecatch_protocol::{DownloadId, QueueId, UnixMillis};
use wirecatch_shell::api;

use super::applies::RowAction;
use super::list::status_tone;
use super::{Group, applies, confirm, fixes, refresh_dialog};
use crate::backend::{Backend, act};
use crate::download_windows::run_record::{Look, RunRecord};
use crate::download_windows::{run_facts, run_panel};
use crate::library::Library;
use crate::ui::layout::{self, Rule};
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
    /// Its average and peak while the dialog is open.
    record: RunRecord,
    /// What happened to it, read from the manager whenever the list changes: a try again that
    /// fails between two of the list's updates leaves the row as it was.
    activity: Vec<Happened>,
    /// Reads asked and the newest applied, so an earlier read that ends later is dropped.
    reads: (u64, u64),
    _subscriptions: Vec<Subscription>,
}

impl Details {
    fn read_activity(&mut self, cx: &mut Context<'_, Self>) {
        self.reads.0 += 1;
        let read_no = self.reads.0;
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
                if read_no > details.reads.1 {
                    details.reads.1 = read_no;
                    if details.activity != activity {
                        details.activity = activity;
                        cx.notify();
                    }
                }
            });
            if shown.is_err() {
                tracing::debug!("the details closed before their activity was read");
            }
        })
        .detach();
    }
}

/// Opens the details of download `id` in a dialog over `window`. It closes by itself once the
/// download is removed.
pub(super) fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let details = cx.new(|cx| {
        let follow = cx.observe_in(
            &library,
            window,
            |details: &mut Details, library, window, cx| {
                let Some(row) = library.read(cx).downloads.find(details.id) else {
                    window.close_dialog(cx);
                    return;
                };
                let now = Instant::now();
                details.record.see(
                    Look {
                        per_mille: text::per_mille(row),
                        speed: row.speed,
                        done: row.done,
                        running: row.status == DownloadStatus::Downloading,
                    },
                    now,
                );
                details.read_activity(cx);
                cx.notify();
            },
        );
        let mut details = Details {
            library,
            id,
            record: RunRecord::default(),
            activity: Vec::new(),
            reads: (0, 0),
            _subscriptions: vec![follow],
        };
        details.read_activity(cx);
        details
    });
    crate::ui::dialog::open_headless(window, cx, move |dialog, _, _| {
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
                    .child(self.now_section(&row, &queue, cx))
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

impl Details {
    /// What is happening now: the problem and its fix, or why it waits and the progress window's
    /// run panel; then that window's figures.
    fn now_section(&self, row: &DownloadSummary, queue: &str, cx: &mut Context<'_, Self>) -> Div {
        let theme = cx.theme();
        let mut section = layout::section(Rule::None, cx).gap_3();
        if let (DownloadStatus::Failed, Some(error)) = (row.status, &row.error) {
            section = section.child(failure(row.id, &error.message, error.fix, cx));
        } else {
            if let Some(detail) = text::status_detail(row, queue) {
                section = section.child(
                    div()
                        .p_3()
                        .rounded(theme.radius)
                        .bg(theme.muted)
                        .text_role(Role::Body, cx)
                        .child(detail),
                );
            }
            section = section.child(run_panel(&self.record, row, cx));
        }
        section = section.children(run_facts(&self.record, row, cx));
        if let Some(media) = &row.media {
            section = section.child(
                div()
                    .text_role(Role::Secondary, cx)
                    .child(text::media_line(media)),
            );
        }
        section
    }
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
                        .flex_grow(1.)
                        .flex_shrink_0()
                        .min_w(px(96.))
                        .whitespace_nowrap()
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
        .small()
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

/// Whether the details offer `fix`: Try again is the foot's own action, and the queue field is
/// right here.
fn offered_here(fix: FixAction) -> bool {
    !matches!(fix, FixAction::Retry | FixAction::ChangeQueue)
}

/// Why it failed, with its fix.
fn failure(
    id: DownloadId,
    message: &str,
    fix: Option<FixAction>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement + use<> {
    Callout::danger(message.to_owned()).when_some(
        fix.filter(|fix| offered_here(*fix)),
        |callout, fix| {
            callout.action(
                Button::new("details-fix")
                    .small()
                    .primary()
                    .label(text::fix_label(fix))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        fixes::run(id, fix, window, cx);
                    })),
            )
        },
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

fn copy_path(path: &str, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(path.to_owned()));
    window.push_notification("Location copied.", cx);
}

/// Where it is saved, with Change until it finishes and Copy, and where it came from, with Copy.
fn places(
    row: &DownloadSummary,
    library: Entity<Library>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = row.id;
    let link = row.url.clone();
    let copied = link.clone();
    let path = row.path.clone();
    layout::section(Rule::None, cx)
        .text_sm()
        .child(place(
            IconName::FolderOpen,
            "Saved to",
            PathText::new(row.path.clone())
                .tooltip("details-saved-to")
                .into_any_element(),
            Some(
                h_flex()
                    .gap_1()
                    .when(row.status != DownloadStatus::Completed, |actions| {
                        actions.child(
                            Button::new("details-change-folder")
                                .small()
                                .outline()
                                .label("Change")
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    fixes::choose_folder(id, false, window, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new("details-copy-path")
                            .small()
                            .outline()
                            .label("Copy")
                            .on_click(move |_, window, cx| copy_path(&path, window, cx)),
                    )
                    .into_any_element(),
            ),
            cx,
        ))
        .child(place(
            IconName::Globe,
            "From",
            div().truncate().child(link).into_any_element(),
            Some(
                Button::new("details-copy-link")
                    .small()
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
                            .small()
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
    let lines = activity_lines(row, happened);
    layout::section(Rule::Above, cx)
        .gap_2()
        .child(div().text_role(Role::Label, cx).child("Activity"))
        .children(lines.into_iter().map(|(at, what)| {
            layout::label_row(ACTIVITY_LABEL_WIDTH, text::date(at), what, cx).items_start()
        }))
}

/// The activity's lines in time order, from its adding to its finish.
fn activity_lines(row: &DownloadSummary, happened: &[Happened]) -> Vec<(UnixMillis, String)> {
    // The host it was added from is the one before its first new link.
    let first_host = happened
        .iter()
        .find_map(|h| match &h.event {
            ActivityEvent::LinkChanged { from, .. } => Some(from.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| text::host(&row.url));
    let mut lines = vec![(row.added_at, format!("Added from {first_host}"))];
    lines.extend(happened.iter().map(|h| (h.at, said(&h.event))));
    if let Some(finished) = row.completed_at {
        lines.push((finished, "Finished".to_owned()));
    }
    lines.sort_by_key(|(at, _)| *at);
    lines
}

/// One line of the activity, as the person reads it.
fn said(event: &ActivityEvent) -> String {
    match event {
        ActivityEvent::Failed { message } if message.is_empty() => "Failed".to_owned(),
        ActivityEvent::Failed { message } => format!("Failed: {message}"),
        ActivityEvent::Paused => "Paused".to_owned(),
        ActivityEvent::Started => "Started".to_owned(),
        ActivityEvent::Resumed => "Resumed".to_owned(),
        ActivityEvent::Retried => "Tried again".to_owned(),
        ActivityEvent::Cancelled => "Cancelled".to_owned(),
        ActivityEvent::LinkChanged { to, .. } => format!("Link changed to {to}"),
        ActivityEvent::Moved { folder } => format!("Moved to {folder}"),
    }
}

/// The labelled actions at the foot, in the row menu's order: Progress window and Show in folder,
/// More with Cancel and Remove, and on the right the one action its state calls for.
fn footer(
    row: &DownloadSummary,
    library: Entity<Library>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let id = row.id;
    let main = RowAction::offered(row.status).first().cloned();
    let cancellable = applies::can_cancel(row);
    layout::bar(Rule::Above, cx)
        .child(
            Button::new("details-window")
                .small()
                .outline()
                .label("Progress window")
                .on_click(move |_, _, cx| crate::download_windows::show_download(id, cx)),
        )
        .when(applies::has_file(row), |bar| {
            bar.child(
                Button::new("details-folder")
                    .small()
                    .outline()
                    .label("Show in folder")
                    .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            )
        })
        .child(
            Button::new("details-more")
                .small()
                .outline()
                .label("More")
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    let (cancelled, removed) = (library.clone(), library.clone());
                    menu.when(cancellable, |menu| {
                        menu.item(
                            PopupMenuItem::new("Cancel")
                                .icon(IconName::CircleX)
                                .on_click(move |_, window, cx| {
                                    confirm::cancel(&cancelled, &[id], &[], window, cx);
                                }),
                        )
                    })
                    .item(
                        PopupMenuItem::new("Remove")
                            .icon(Icon::new(Lucide::Trash))
                            .on_click(move |_, window, cx| {
                                confirm::remove(&removed, &[id], &[], window, cx);
                            }),
                    )
                }),
        )
        .child(div().flex_1())
        .when_some(main, |bar, (action, label, icon)| {
            bar.child(
                Button::new("details-main")
                    .small()
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
    fn the_details_do_not_offer_a_fix_that_another_button_offers() {
        assert!(!offered_here(FixAction::ChangeQueue));
        assert!(!offered_here(FixAction::Retry));
        assert!(offered_here(FixAction::ChooseFolder));
    }

    #[test]
    fn the_activity_names_the_host_it_was_added_from_after_a_new_link() {
        let row = DownloadSummary {
            url: "https://files.example.org/a.bin".into(),
            added_at: UnixMillis(1),
            completed_at: Some(UnixMillis(9)),
            ..wirecatch_test_support::downloads::download(1)
        };
        let happened = [Happened {
            at: UnixMillis(5),
            event: ActivityEvent::LinkChanged {
                from: "example.com".into(),
                to: "files.example.org".into(),
            },
        }];
        let lines: Vec<String> = activity_lines(&row, &happened)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        assert_eq!(
            lines,
            [
                "Added from example.com",
                "Link changed to files.example.org",
                "Finished"
            ]
        );
    }
}
