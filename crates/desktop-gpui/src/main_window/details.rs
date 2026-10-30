//! One download's details, in a dialog over the list that keeps updating (D47): what is happening
//! now, the short facts, where it is saved and where it came from, and what happened to it, with
//! its actions at the foot.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction, PER_MILLE_DONE};
use wirecatch_shell::api;

use super::list::status_tone;
use super::{Group, confirm};
use crate::backend::{act, command};
use crate::library::Library;
use crate::ui::layout::{self, Rule};
use crate::ui::{Callout, Role, Typed as _, tokens};
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
    _subscriptions: Vec<Subscription>,
}

/// Opens the details of download `id` in a dialog over `window`. It closes by itself once the
/// download is removed.
pub(super) fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let details = cx.new(|cx| {
        let follow = cx.observe_in(
            &library,
            window,
            |details: &mut Details, library, window, cx| {
                if library
                    .read(cx)
                    .downloads
                    .iter()
                    .any(|row| row.id == details.id)
                {
                    cx.notify();
                } else {
                    window.close_dialog(cx);
                }
            },
        );
        Details {
            library,
            id,
            _subscriptions: vec![follow],
        }
    });
    crate::ui::dialog::open(window, cx, move |dialog, _, _| {
        dialog.w(px(WIDTH)).p_0().child(details.clone())
    });
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let Some(row) = library
            .downloads
            .iter()
            .find(|row| row.id == self.id)
            .cloned()
        else {
            return div().into_any_element();
        };
        let queue = library
            .queue(row.queue)
            .map_or_else(|| "Its queue".to_owned(), |q| q.settings.name.clone());
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
                    .child(now_section(&row, &queue, cx))
                    .child(facts(&row, &queue, cx))
                    .child(places(&row, cx))
                    .child(activity(&row, cx)),
            )
            .child(footer(&row, self.library.clone(), cx))
            .into_any_element()
    }
}

/// The name, its status and where it comes from.
fn header(row: &DownloadSummary, cx: &mut Context<'_, Details>) -> impl IntoElement {
    let _theme = cx.theme();
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
                            .child(text::status_label(row.status)),
                    )
                    .child(div().child(text::host(&row.url).to_owned())),
            ),
    )
}

/// What is happening now: the problem and its fix, or why it waits; the speed and progress.
fn now_section(
    row: &DownloadSummary,
    queue: &str,
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

/// The short facts in one strip, each one the rest of the dialog does not already say.
fn facts(row: &DownloadSummary, queue: &str, cx: &App) -> impl IntoElement {
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
                        .child(div().text_role(Role::Strong, cx).child(value))
                }),
        )
}

/// Why it failed, with its fix and Try again.
fn failure(
    id: DownloadId,
    message: &str,
    fix: Option<FixAction>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement + use<> {
    Callout::danger(message.to_owned())
        .when_some(
            fix.filter(|fix| *fix != FixAction::Retry),
            |callout, fix| {
                callout.action(
                    Button::new("details-fix")
                        .small()
                        .primary()
                        .label(text::fix_label(fix))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            run_fix(id, fix, window, cx);
                        })),
                )
            },
        )
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

/// Where it is saved, with Change until it finishes, and where it came from, with Copy.
fn places(row: &DownloadSummary, cx: &mut Context<'_, Details>) -> impl IntoElement {
    let theme = cx.theme();
    let id = row.id;
    let link = text::link_for_display(&row.url);
    let copied = link.clone();
    let expired = matches!(row.status, DownloadStatus::Paused)
        || matches!(
            row.waiting,
            Some(wirecatch_protocol::app::WaitReason::ServerRefused { .. })
        );
    layout::section(Rule::None, cx)
        .text_sm()
        .child(place(
            IconName::FolderOpen,
            "Saved to",
            row.path.clone(),
            (row.status != DownloadStatus::Completed).then(|| {
                Button::new("details-change-folder")
                    .xsmall()
                    .ghost()
                    .label("Change")
                    .on_click(
                        cx.listener(move |_, _, window, cx| choose_folder(id, false, window, cx)),
                    )
                    .into_any_element()
            }),
            cx,
        ))
        .child(place(
            IconName::Globe,
            "From",
            link,
            Some(
                Button::new("details-copy-link")
                    .xsmall()
                    .ghost()
                    .label("Copy")
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        window.push_notification("Link copied.", cx);
                    })
                    .into_any_element(),
            ),
            cx,
        ))
        .when(expired && row.status != DownloadStatus::Failed, |list| {
            list.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("Stuck? The link may have expired."),
            )
        })
}

fn place(
    icon: IconName,
    label: &'static str,
    value: String,
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
            .text_role(Role::Strong, cx)
            .truncate()
            .child(SharedString::from(value)),
        cx,
    )
    .py_2()
    .children(action)
}

/// What happened to it.
fn activity(row: &DownloadSummary, cx: &App) -> impl IntoElement {
    let line = |when: String, what: String| layout::label_row(ACTIVITY_LABEL_WIDTH, when, what, cx);
    layout::section(Rule::Above, cx)
        .gap_2()
        .child(div().text_role(Role::Label, cx).child("Activity"))
        .child(line(
            text::date(row.added_at),
            format!("Added from {}", text::host(&row.url)),
        ))
        .when_some(row.completed_at, |list, finished| {
            list.child(line(text::date(finished), "Finished".to_owned()))
        })
}

/// The actions at the foot: the main one for its state and Show in folder; More holds the link
/// and cancelling, and Remove stands apart.
fn footer(
    row: &DownloadSummary,
    library: Entity<Library>,
    cx: &mut Context<'_, Details>,
) -> impl IntoElement {
    let id = row.id;
    let main: Option<(&'static str, IconName, bool)> = match row.status {
        DownloadStatus::Completed => Some(("Open", IconName::ExternalLink, true)),
        DownloadStatus::Paused => Some(("Resume", IconName::Play, true)),
        DownloadStatus::Cancelled => Some(("Start over", IconName::RotateCw, true)),
        DownloadStatus::Failed => None,
        _ => Some(("Pause", IconName::Pause, false)),
    };
    let status = row.status;
    let cancellable = !matches!(
        row.status,
        DownloadStatus::Completed | DownloadStatus::Cancelled
    );
    let link = text::link_for_display(&row.url);
    layout::bar(Rule::Above, cx)
        .when_some(main, |bar, (label, icon, primary)| {
            let button = Button::new("details-main").small().icon(icon).label(label);
            let button = if primary {
                button.primary()
            } else {
                button.outline()
            };
            bar.child(
                button.on_click(cx.listener(move |_, _, window, cx| match status {
                    DownloadStatus::Completed => files::open(id, window, cx),
                    DownloadStatus::Paused => {
                        act(window.window_handle(), cx, move |manager| {
                            api::resume(manager, id)
                        });
                    }
                    DownloadStatus::Cancelled => {
                        act(window.window_handle(), cx, move |manager| {
                            api::retry(manager, id)
                        });
                    }
                    _ => act(window.window_handle(), cx, move |manager| {
                        api::pause(manager, id)
                    }),
                })),
            )
        })
        .child(
            Button::new("details-folder")
                .small()
                .ghost()
                .icon(IconName::FolderOpen)
                .label("Show in folder")
                .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
        )
        .child(div().flex_1())
        .child(more(id, link, cancellable, library.clone()))
        .child(
            Button::new("details-remove")
                .small()
                .ghost()
                .icon(IconName::Delete)
                .label("Remove")
                .on_click(move |_, window, cx| confirm::remove(&library, &[id], window, cx)),
        )
}

/// More: copying the link, and cancelling while there is something to cancel.
fn more(
    id: DownloadId,
    link: String,
    cancellable: bool,
    library: Entity<Library>,
) -> impl IntoElement {
    Button::new("details-more")
        .small()
        .ghost()
        .icon(IconName::Ellipsis)
        .tooltip("More actions")
        .dropdown_menu(move |menu, _, _| {
            let copied = link.clone();
            let cancelled = library.clone();
            menu.item(
                PopupMenuItem::new("Copy link")
                    .icon(IconName::Copy)
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        window.push_notification("Link copied.", cx);
                    }),
            )
            .when(cancellable, |menu| {
                menu.item(
                    PopupMenuItem::new("Cancel download")
                        .icon(IconName::CircleX)
                        .on_click(move |_, window, cx| {
                            confirm::cancel(&cancelled, &[id], window, cx);
                        }),
                )
            })
        })
}

/// A failure's fix, where the panel can do it itself; the rest come with their own screens.
fn run_fix(id: DownloadId, fix: FixAction, window: &mut Window, cx: &mut Context<'_, Details>) {
    match fix {
        FixAction::Retry => act(window.window_handle(), cx, move |manager| {
            api::retry(manager, id)
        }),
        FixAction::ChooseFolder => choose_folder(id, true, window, cx),
        FixAction::FreeDiskSpace => files::show_in_folder(id, window, cx),
        FixAction::KeepOriginal => act(window.window_handle(), cx, move |manager| {
            api::keep_original(manager, id)
        }),
        FixAction::RefreshLink | FixAction::ProxySettings | FixAction::ChangeQueue => {
            window.push_notification("This comes with a later version of this window.", cx);
        }
    }
}

/// Asks for a new folder for download `id` and saves it there, then tries it again when `retry`.
fn choose_folder(id: DownloadId, retry: bool, window: &mut Window, cx: &mut Context<'_, Details>) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Save here".into()),
    });
    cx.spawn_in(window, async move |_, cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let Some(folder) = paths.into_iter().next() else {
            return;
        };
        let folder = folder.to_string_lossy().into_owned();
        _ = cx.update(|window, cx| {
            command(window.window_handle(), cx, move |manager| async move {
                api::set_download_folder(&manager, id, &folder).await?;
                if retry {
                    api::retry(&manager, id)
                } else {
                    Ok(())
                }
            });
        });
    })
    .detach();
}
