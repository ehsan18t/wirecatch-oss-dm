//! Removing and cancelling downloads, each after a question that says what happens to their files.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_shell::api;

use crate::backend::command;
use crate::library::Library;
use crate::ui::{Role, Typed as _};

const WIDTH: f32 = 440.;

/// What the downloads asked about are, read when the question opens.
struct Asked {
    ids: Vec<DownloadId>,
    /// The name when there is one download.
    one: Option<String>,
    finished: usize,
    started: bool,
    running: bool,
}

impl Asked {
    fn read(library: &Entity<Library>, ids: &[DownloadId], cx: &App) -> Self {
        let rows: Vec<_> = library
            .read(cx)
            .downloads
            .iter()
            .filter(|row| ids.contains(&row.id))
            .collect();
        Self {
            ids: rows.iter().map(|row| row.id).collect(),
            one: (rows.len() == 1).then(|| rows[0].file_name.clone()),
            finished: rows
                .iter()
                .filter(|row| row.status == DownloadStatus::Completed)
                .count(),
            started: rows.iter().any(|row| row.done > 0),
            running: rows.iter().any(|row| {
                matches!(
                    row.status,
                    DownloadStatus::Downloading | DownloadStatus::Connecting
                )
            }),
        }
    }

    fn count(&self) -> usize {
        self.ids.len()
    }
}

/// The Remove question's form: whether finished files go too.
struct RemoveForm {
    asked: Asked,
    delete_files: bool,
}

impl Render for RemoveForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let _theme = cx.theme();
        let asked = &self.asked;
        let unfinished = asked.count() - asked.finished;
        let consequence = if unfinished == 0 {
            None
        } else if unfinished == asked.count() {
            Some(if asked.count() == 1 {
                "It stops, and the part already downloaded is deleted.".to_owned()
            } else {
                "They stop, and the parts already downloaded are deleted.".to_owned()
            })
        } else {
            Some(format!(
                "{unfinished} of them are unfinished: they stop, and their partial files are deleted."
            ))
        };
        v_flex()
            .gap_4()
            .text_sm()
            .when_some(asked.one.clone(), |form, name| {
                form.child(div().text_role(Role::Strong, cx).child(name))
            })
            .when_some(consequence, |form, said| {
                form.child(div().text_role(Role::Secondary, cx).child(said))
            })
            .when(asked.finished > 0, |form| {
                form.child(
                    Checkbox::new("delete-files")
                        .label(if asked.finished == 1 {
                            "Also delete the file from the disk"
                        } else {
                            "Also delete the finished files from the disk"
                        })
                        .checked(self.delete_files)
                        .on_click(cx.listener(|form, checked: &bool, _, cx| {
                            form.delete_files = *checked;
                            cx.notify();
                        })),
                )
            })
    }
}

/// Asks before removing `ids` from the list.
pub(super) fn remove(
    library: &Entity<Library>,
    ids: &[DownloadId],
    window: &mut Window,
    cx: &mut App,
) {
    let asked = Asked::read(library, ids, cx);
    if asked.count() == 0 {
        return;
    }
    let title = if asked.one.is_some() {
        "Remove this download?".to_owned()
    } else {
        format!("Remove {} downloads?", asked.count())
    };
    let form = cx.new(|_| RemoveForm {
        asked,
        delete_files: false,
    });
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let deletes = {
            let form = form.read(cx);
            form.delete_files && form.asked.finished > 0
        };
        let removing = form.clone();
        dialog
            .title(title.clone())
            .w(px(WIDTH))
            .child(form.clone())
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("keep")
                            .label("Keep")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("remove")
                            .danger()
                            .label(if deletes {
                                "Remove and delete"
                            } else {
                                "Remove from list"
                            })
                            .on_click(move |_, window, cx| {
                                let (ids, delete) = {
                                    let form = removing.read(cx);
                                    (
                                        form.asked.ids.clone(),
                                        form.delete_files && form.asked.finished > 0,
                                    )
                                };
                                window.close_dialog(cx);
                                command(window.window_handle(), cx, move |manager| async move {
                                    api::remove(&manager, &ids, delete).await
                                });
                            }),
                    ),
            )
    });
}

/// Asks before cancelling `ids`: they stop and lose what was downloaded, but stay in the list.
pub(super) fn cancel(
    library: &Entity<Library>,
    ids: &[DownloadId],
    window: &mut Window,
    cx: &mut App,
) {
    let asked = Asked::read(library, ids, cx);
    if asked.count() == 0 {
        return;
    }
    let one = asked.one.is_some();
    let title = if one {
        "Cancel this download?".to_owned()
    } else {
        format!("Cancel {} downloads?", asked.count())
    };
    let said = match (one, asked.started) {
        (true, true) => {
            "It stops, and the part already downloaded is deleted. It stays in the list, so you can start it over later."
        }
        (true, false) => "It will not start. It stays in the list, so you can start it over later.",
        (false, true) => {
            "They stop, and the parts already downloaded are deleted. They stay in the list, so you can start them over later."
        }
        (false, false) => {
            "They will not start. They stay in the list, so you can start them over later."
        }
    };
    let keep = if asked.running {
        "Keep downloading"
    } else if one {
        "Keep it"
    } else {
        "Keep them"
    };
    let name = asked.one.clone();
    let ids = asked.ids;
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let ids = ids.clone();
        let _theme = cx.theme();
        dialog
            .title(title.clone())
            .w(px(WIDTH))
            .child(
                v_flex()
                    .gap_4()
                    .text_sm()
                    .when_some(name.clone(), |body, name| {
                        body.child(div().text_role(Role::Strong, cx).child(name))
                    })
                    .child(div().text_role(Role::Secondary, cx).child(said)),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("keep")
                            .label(keep)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("cancel-download")
                            .danger()
                            .label("Cancel download")
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let ids = ids.clone();
                                command(window.window_handle(), cx, move |manager| async move {
                                    ids.iter().try_for_each(|id| api::cancel(&manager, *id))
                                });
                            }),
                    ),
            )
    });
}
