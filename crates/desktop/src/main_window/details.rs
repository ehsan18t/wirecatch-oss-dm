//! One download's details, in a dialog over the list that keeps updating (D47, D245): what is
//! happening now, the short facts, where it is saved and where it came from, and what happened to
//! it, with its labelled actions at the foot.

use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Div, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px,
};
use wirecatch_core::{ActivityEvent, Happened, can_move_to_queue};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction};
use wirecatch_protocol::{DownloadId, QueueId, UnixMillis};
use wirecatch_shell::api;

use super::applies::RowAction;
use super::list::status_tone;
use super::{Group, applies, confirm, fixes, refresh_dialog};
use crate::backend::{Backend, act};
use crate::download_windows::run_record::RunRecord;
use crate::download_windows::{run_facts, run_panel};
use crate::library::Library;
use crate::ui::layout::{self, Rule};
use crate::ui::{Callout, PathText, Role, Typed as _, tokens};
use crate::{files, text};

const WIDTH: f32 = 600.;

/// The label column of Saved to and From.
const PLACE_LABEL_WIDTH: f32 = 80.;

/// The date column of Activity.
const ACTIVITY_LABEL_WIDTH: f32 = 148.;

const ACTIVITY_HEIGHT: f32 = 112.;

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
                details.record.watch(row, Instant::now());
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
    crate::ui::dialog::open_headless(window, cx, move |dialog, _, cx| {
        frame(dialog, details.clone(), cx)
    });
}

fn frame(dialog: Dialog, details: Entity<Details>, cx: &App) -> Dialog {
    let view = details.read(cx);
    let library = view.library.clone();
    let Some(row) = library.read(cx).downloads.find(view.id).cloned() else {
        return dialog;
    };
    dialog
        .w(px(WIDTH))
        .p_0()
        .gap_0()
        .title(header(&row, cx))
        .child(details)
        .footer(footer(&row, library, cx))
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let Some(row) = library.downloads.find(self.id).cloned() else {
            return div().into_any_element();
        };
        let queue = library.queue_name(row.queue).map(str::to_owned);
        let others: Vec<(QueueId, String)> = library
            .queues
            .iter()
            .filter(|q| can_move_to_queue(&row, q.id))
            .map(|q| (q.id, q.settings.name.clone()))
            .collect();
        v_flex()
            .id("details")
            .text_sm()
            .child(self.now_section(&row, queue.as_deref(), cx))
            .child(facts(
                &row,
                queue.as_deref().unwrap_or("Its queue"),
                &others,
                cx,
            ))
            .child(places(&row, self.library.clone(), cx))
            .child(activity(&row, &self.activity, cx))
            .into_any_element()
    }
}

/// The name, its status and where it comes from.
fn header(row: &DownloadSummary, cx: &App) -> impl IntoElement + use<> {
    let tone = status_tone(row, cx);
    div().px_4().pt_3().flex_shrink_0().child(
        crate::ui::dialog::Header::new(row.file_name.clone()).description(
            h_flex()
                .gap_2()
                .min_w_0()
                .text_role(Role::Meta, cx)
                .child(
                    h_flex()
                        .gap_1()
                        .text_color(tone)
                        .child(div().size(px(6.)).rounded_full().bg(tone))
                        .child(text::status_name(row)),
                )
                .child(div().truncate().child(text::host(&row.url).to_owned())),
        ),
    )
}

impl Details {
    /// What is happening now: the problem and its fix, or why it waits and the progress window's
    /// run panel; then that window's figures.
    fn now_section(
        &self,
        row: &DownloadSummary,
        queue: Option<&str>,
        cx: &mut Context<'_, Self>,
    ) -> Div {
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
            section = section.child(run_panel(&self.record, row, queue, cx));
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

/// The short facts in one strip. The queue is a menu when the download can move.
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
                        .child(div().text_role(Role::Caption, cx).child(label))
                        .child(if label == "Queue" && !others.is_empty() {
                            queue_picker(row.id, value, others.to_vec()).into_any_element()
                        } else {
                            div()
                                .text_role(Role::Meta, cx)
                                .text_color(theme.foreground)
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
                    .outline()
                    .label(text::fix_label(fix))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        fixes::run(id, fix, window, cx);
                    })),
            )
        },
    )
}

pub(super) fn copy_link_label(count: usize) -> &'static str {
    if count == 1 {
        "Copy link"
    } else {
        "Copy links"
    }
}

/// Puts downloads' `links` on the clipboard, one per line, and says so.
pub(super) fn copy_links(links: &[String], window: &mut Window, cx: &mut App) {
    if links.is_empty() {
        return;
    }
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
        .text_role(Role::Meta, cx)
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
            div()
                .id("details-source")
                .truncate()
                .child(text::readable_link(&link))
                .tooltip(crate::ui::tooltip::plain(text::readable_link(&link)))
                .into_any_element(),
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
        div()
            .min_w_0()
            .text_role(Role::Meta, cx)
            .text_color(cx.theme().foreground)
            .child(value),
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
        .text_role(Role::Meta, cx)
        .child(div().text_role(Role::Label, cx).child("Activity"))
        .child(
            div()
                .debug_selector(|| "details-activity".to_owned())
                .child(
                    v_flex()
                        .id("details-activity")
                        .debug_selector(|| "details-activity-entries".to_owned())
                        .max_h(px(ACTIVITY_HEIGHT))
                        .gap_2()
                        .children(lines.into_iter().map(|(at, what)| {
                            layout::label_row(ACTIVITY_LABEL_WIDTH, text::date(at), what, cx)
                                .items_start()
                                .flex_shrink_0()
                        }))
                        .overflow_y_scrollbar(),
                ),
        )
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

/// Every applicable action is visible; the state action is labelled on the right.
fn footer(row: &DownloadSummary, library: Entity<Library>, cx: &App) -> impl IntoElement + use<> {
    let id = row.id;
    let main = RowAction::offered(row.status).first().cloned();
    let cancellable = applies::can_cancel(row);
    let (cancelled, removed) = (library.clone(), library);
    layout::bar(Rule::Above, cx)
        .debug_selector(|| "details-footer".to_owned())
        .flex_shrink_0()
        .child(
            Button::new("details-window")
                .small()
                .ghost()
                .icon(Icon::new(Lucide::AppWindow))
                .accessibility_label("Progress window")
                .tooltip("Progress window")
                .on_click(move |_, _, cx| crate::download_windows::show_download(id, cx)),
        )
        .when(applies::has_file(row), |bar| {
            bar.child(
                Button::new("details-folder")
                    .small()
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .accessibility_label("Show in folder")
                    .tooltip("Show in folder")
                    .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            )
        })
        .child(
            Button::new("details-remove")
                .small()
                .ghost()
                .icon(Icon::new(Lucide::Trash))
                .accessibility_label("Remove download")
                .tooltip("Remove download")
                .on_click(move |_, window, cx| {
                    confirm::remove(&removed, &[id], &[], None, window, cx);
                }),
        )
        .child(div().flex_1())
        .when(cancellable, |bar| {
            bar.child(
                Button::new("details-cancel")
                    .small()
                    .outline()
                    .icon(IconName::CircleX)
                    .label("Cancel")
                    .accessibility_label("Cancel download")
                    .tooltip("Cancel download")
                    .on_click(move |_, window, cx| {
                        confirm::cancel(&cancelled, &[id], &[], None, window, cx);
                    }),
            )
        })
        .when_some(main, |bar, (action, label, icon)| {
            bar.child(
                div()
                    .flex_shrink_0()
                    .debug_selector(|| "details-state-action".to_owned())
                    .child(
                        Button::new("details-main")
                            .small()
                            .outline()
                            .icon(icon)
                            .label(if matches!(action, RowAction::Open) {
                                "Open file"
                            } else {
                                label
                            })
                            .on_click(move |_, window, cx| action.run(id, window, cx)),
                    ),
            )
        })
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Empty, TestAppContext, size};

    use super::*;

    fn details_with_history(status: DownloadStatus, cx: &mut App) -> Entity<Details> {
        let file_name =
            "a-long-file-name.with-many-parts-that-must-leave-the-close-button-free.zip";
        let row = DownloadSummary {
            file_name: file_name.into(),
            path: format!("C:/Downloads/a-long-project-folder/{file_name}"),
            status,
            total: Some(1000),
            done: 500,
            completed_at: (status == DownloadStatus::Completed).then_some(UnixMillis(10)),
            error: (status == DownloadStatus::Failed).then(|| wirecatch_protocol::app::UserError {
                message: "The link has expired. Get a new link to continue.".into(),
                fix: Some(FixAction::RefreshLink),
            }),
            ..wirecatch_test_support::downloads::download(1)
        };
        let id = row.id;
        let library = cx.new(|_| {
            let mut library = Library::empty();
            library.downloads.replace(vec![row]);
            library
        });
        cx.new(|_| Details {
            library,
            id,
            record: RunRecord::default(),
            activity: (1..30)
                .map(|at| Happened {
                    at: UnixMillis(at),
                    event: ActivityEvent::Paused,
                })
                .collect(),
            reads: (0, 0),
            _subscriptions: Vec::new(),
        })
    }

    #[gpui_kit::test]
    fn the_details_keep_their_footer_inside_a_short_window(cx: &mut TestAppContext) {
        struct Host;
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
                Empty
            }
        }
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            gpui_kit::component::Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(720.), px(600.)));
        for status in [
            DownloadStatus::Downloading,
            DownloadStatus::Paused,
            DownloadStatus::Completed,
            DownloadStatus::Failed,
        ] {
            cx.update(|window, cx| {
                let details = details_with_history(status, cx);
                crate::ui::dialog::open_headless(window, cx, move |dialog, _, cx| {
                    frame(dialog, details.clone(), cx)
                });
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let dialog = cx.debug_bounds("dialog-0").expect("the dialog is drawn");
            let footer = cx
                .debug_bounds("details-footer")
                .expect("the footer is drawn");
            assert!(
                footer.bottom() <= dialog.bottom() && footer.right() <= dialog.right(),
                "the footer runs outside the dialog for {status:?}: {footer:?}, {dialog:?}"
            );
            let title = cx.debug_bounds("dialog-title").expect("the title is drawn");
            let activity = cx
                .debug_bounds("details-activity")
                .expect("the activity is drawn");
            assert!(
                activity.size.height <= px(ACTIVITY_HEIGHT),
                "the activity grows beyond its allotted space: {activity:?}"
            );
            let entries = cx
                .debug_bounds("details-activity-entries")
                .expect("the activity entries are drawn");
            assert!(
                entries.size.height > activity.size.height,
                "a long history must overflow its viewport, keeping every event available"
            );
            let state_action = cx
                .debug_bounds("details-state-action")
                .expect("the state action is drawn");
            assert!(
                state_action.right() <= footer.right() - px(16.),
                "the labelled actions run outside the footer for {status:?}: {state_action:?}, {footer:?}"
            );
            assert!(
                title.right() <= dialog.right() - px(48.),
                "the title covers Close for {status:?}"
            );
            cx.update(gpui_kit::component::WindowExt::close_dialog);
            cx.run_until_parked();
        }
    }

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
