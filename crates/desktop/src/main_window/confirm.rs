//! Removing and cancelling downloads, each after a question that says what happens to their files.

use std::collections::HashSet;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px,
};
use wirecatch_core::RemoveFiles;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_shell::api;

use super::{applies, selection};
use crate::backend::command;
use crate::library::Library;
use crate::rows::Rows;
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

/// Those of `ids` that can be cancelled: a selection may hold finished rows too.
fn cancellable(downloads: &Rows, ids: &[DownloadId]) -> Vec<DownloadId> {
    ids.iter()
        .copied()
        .filter(|id| downloads.find(*id).is_some_and(applies::can_cancel))
        .collect()
}

impl Asked {
    fn read(library: &Entity<Library>, ids: &[DownloadId], cx: &App) -> Self {
        let ids: HashSet<DownloadId> = ids.iter().copied().collect();
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
    /// How many of the downloads are in collapsed groups, out of sight.
    folded: usize,
    delete_files: bool,
}

/// Says how much of what is being removed the list does not show.
fn folded_note(count: usize, folded: usize) -> Option<String> {
    match (count, folded) {
        (_, 0) => None,
        (1, _) => Some("It is in a collapsed group.".to_owned()),
        (_, 1) => Some("1 of them is in a collapsed group.".to_owned()),
        (_, folded) => Some(format!("{folded} of them are in collapsed groups.")),
    }
}

impl Render for RemoveForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
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
            .when_some(folded_note(asked.count(), self.folded), |form, said| {
                form.child(div().text_role(Role::Secondary, cx).child(said))
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

/// The remove button's label and look: the danger colour only when files are deleted.
fn remove_button(deletes: bool) -> (&'static str, ButtonVariant) {
    if deletes {
        ("Remove and delete", ButtonVariant::Danger)
    } else {
        ("Remove from list", ButtonVariant::Primary)
    }
}

/// Asks before removing `ids` from the list; `folded` are the ones the list does not show.
pub(super) fn remove(
    library: &Entity<Library>,
    ids: &[DownloadId],
    folded: &[DownloadId],
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
    let folded = asked.ids.iter().filter(|id| folded.contains(id)).count();
    let form = cx.new(|_| RemoveForm {
        asked,
        folded,
        delete_files: false,
    });
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let deletes = {
            let form = form.read(cx);
            form.delete_files && form.asked.finished > 0
        };
        let (label, variant) = remove_button(deletes);
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
                            .with_variant(variant)
                            .label(label)
                            .on_click(move |_, window, cx| {
                                let (ids, files) = {
                                    let form = removing.read(cx);
                                    (
                                        form.asked.ids.clone(),
                                        if form.delete_files && form.asked.finished > 0 {
                                            RemoveFiles::Delete
                                        } else {
                                            RemoveFiles::Keep
                                        },
                                    )
                                };
                                window.close_dialog(cx);
                                command(window.window_handle(), cx, move |manager| async move {
                                    api::remove(&manager, &ids, files).await
                                });
                            }),
                    ),
            )
    });
}

/// Asks before cancelling `ids`: they stop and lose what was downloaded, but stay in the list.
/// `folded` are the ones the list does not show.
pub fn cancel(
    library: &Entity<Library>,
    ids: &[DownloadId],
    folded: &[DownloadId],
    window: &mut Window,
    cx: &mut App,
) {
    let asked = Asked::read(library, &cancellable(&library.read(cx).downloads, ids), cx);
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
    let folded = asked.ids.iter().filter(|id| folded.contains(id)).count();
    let note = folded_note(asked.count(), folded);
    let name = asked.one.clone();
    let ids = asked.ids;
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let ids = ids.clone();
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
                    .when_some(note.clone(), |body, note| {
                        body.child(div().text_role(Role::Secondary, cx).child(note))
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
                                selection::each_id(ids.clone(), api::cancel, window, cx);
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::DownloadSummary;
    use wirecatch_test_support::downloads::download;

    #[test]
    fn only_removing_with_the_files_is_marked_as_danger() {
        assert_eq!(remove_button(false).1, ButtonVariant::Primary);
        assert_eq!(remove_button(true).1, ButtonVariant::Danger);
    }

    #[test]
    fn removing_says_how_much_of_it_is_out_of_sight() {
        assert_eq!(folded_note(5, 0), None);
        assert_eq!(folded_note(1, 1).unwrap(), "It is in a collapsed group.");
        assert_eq!(
            folded_note(5, 1).unwrap(),
            "1 of them is in a collapsed group."
        );
        assert_eq!(
            folded_note(5, 3).unwrap(),
            "3 of them are in collapsed groups."
        );
    }

    #[test]
    fn cancel_leaves_out_finished_empty_and_unknown_rows() {
        let row = |id, status| DownloadSummary {
            status,
            ..download(id)
        };
        let mut rows = Rows::default();
        rows.replace(vec![
            row(1, DownloadStatus::Downloading),
            row(2, DownloadStatus::Completed),
            row(3, DownloadStatus::Cancelled),
            DownloadSummary {
                done: 1,
                ..row(4, DownloadStatus::Failed)
            },
            row(6, DownloadStatus::Failed),
        ]);
        let ids = [1, 2, 3, 4, 5, 6].map(DownloadId);
        assert_eq!(cancellable(&rows, &ids), [DownloadId(1), DownloadId(4)]);
    }
}
