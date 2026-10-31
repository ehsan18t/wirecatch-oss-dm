//! Deleting a queue, after a question that says what happens to its downloads: they move to the
//! default queue, or leave the list, with their files if the person asks. A queue is deleted only
//! once its name is typed exactly, case included: its settings take time to set up again.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};
use wirecatch_core::RemoveFiles;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use super::{MainWindow, View};
use crate::backend::command;
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _};

const WIDTH: f32 = 440.;

struct DeleteForm {
    queue: QueueId,
    name: String,
    /// Where the downloads go when they stay.
    fallback: String,
    downloads: Vec<DownloadId>,
    finished: usize,
    remove_downloads: bool,
    delete_files: bool,
    typed: Entity<InputState>,
    _typing: Subscription,
}

impl DeleteForm {
    fn files(&self) -> RemoveFiles {
        if self.remove_downloads && self.delete_files && self.finished > 0 {
            RemoveFiles::Delete
        } else {
            RemoveFiles::Keep
        }
    }

    fn confirmed(&self, cx: &App) -> bool {
        self.typed.read(cx).value() == self.name.as_str()
    }

    fn what_happens(&self) -> String {
        let count = self.downloads.len();
        match (count, self.remove_downloads) {
            (0, _) => "It holds no downloads.".to_owned(),
            (1, false) => format!("Its download moves to {}.", self.fallback),
            (n, false) => format!("Its {n} downloads move to {}.", self.fallback),
            (1, true) => "Its download is removed from the list; unfinished, it stops and its partial file is deleted.".to_owned(),
            (n, true) => format!("Its {n} downloads are removed from the list; unfinished ones stop and their partial files are deleted."),
        }
    }
}

impl Render for DeleteForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let count = self.downloads.len();
        v_flex()
            .gap_4()
            .text_sm()
            .child(
                div()
                    .text_role(Role::Secondary, cx)
                    .child(self.what_happens()),
            )
            .when(count > 0, |form| {
                form.child(
                    Checkbox::new("remove-downloads")
                        .label(if count == 1 {
                            "Also remove its download from the list".to_owned()
                        } else {
                            format!("Also remove its {count} downloads from the list")
                        })
                        .checked(self.remove_downloads)
                        .on_click(cx.listener(|form, checked: &bool, _, cx| {
                            form.remove_downloads = *checked;
                            if !*checked {
                                form.delete_files = false;
                            }
                            cx.notify();
                        })),
                )
            })
            .when(self.finished > 0, |form| {
                form.child(
                    Checkbox::new("delete-files")
                        .label(if self.finished == 1 {
                            "And delete the finished file from the disk"
                        } else {
                            "And delete the finished files from the disk"
                        })
                        .checked(self.delete_files)
                        .disabled(!self.remove_downloads)
                        .on_click(cx.listener(|form, checked: &bool, _, cx| {
                            form.delete_files = *checked;
                            cx.notify();
                        })),
                )
            })
            .child(Field::new(
                format!("Type {} to confirm", self.name),
                Input::new(&self.typed),
            ))
    }
}

/// Asks before deleting queue `id`, which must not be the default one.
pub(super) fn ask(
    library: &Entity<Library>,
    id: QueueId,
    main: WeakEntity<MainWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let library = library.read(cx);
    let Some(queue) = library.queue(id).filter(|queue| !queue.is_default) else {
        return;
    };
    let name = queue.settings.name.clone();
    let fallback = library
        .queues
        .iter()
        .find(|queue| queue.is_default)
        .map_or_else(
            || "the default queue".to_owned(),
            |queue| queue.settings.name.clone(),
        );
    let rows: Vec<_> = library
        .downloads
        .iter()
        .filter(|row| row.queue == id)
        .collect();
    let downloads: Vec<DownloadId> = rows.iter().map(|row| row.id).collect();
    let finished = rows
        .iter()
        .filter(|row| row.status == DownloadStatus::Completed)
        .count();
    let typed = cx.new(|cx| InputState::new(window, cx));
    let field = typed.clone();
    let form = cx.new(|cx| {
        let typing = cx.subscribe(&typed, |_: &mut DeleteForm, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        DeleteForm {
            queue: id,
            name: name.clone(),
            fallback,
            downloads,
            finished,
            remove_downloads: false,
            delete_files: false,
            typed,
            _typing: typing,
        }
    });
    let title = format!("Delete {name}?");
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let confirmed = form.read(cx).confirmed(cx);
        let deleting = form.clone();
        let main = main.clone();
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
                        Button::new("keep-queue")
                            .label("Keep queue")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("delete-queue")
                            .danger()
                            .label("Delete queue")
                            .disabled(!confirmed)
                            .on_click(move |_, window, cx| {
                                delete(&deleting, &main, window, cx);
                            }),
                    ),
            )
    });
    crate::ui::dialog::focus_after_opening(&field, window, cx);
}

fn delete(
    form: &Entity<DeleteForm>,
    main: &WeakEntity<MainWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let (id, removed, files) = {
        let form = form.read(cx);
        if !form.confirmed(cx) {
            return;
        }
        let removed = if form.remove_downloads {
            form.downloads.clone()
        } else {
            Vec::new()
        };
        (form.queue, removed, form.files())
    };
    window.close_dialog(cx);
    _ = main.update(cx, |main, cx| {
        if main.view == View::Queue(id) {
            main.show(View::All, window, cx);
        }
    });
    command(window.window_handle(), cx, move |manager| async move {
        if !removed.is_empty() {
            api::remove(&manager, &removed, files).await?;
        }
        api::delete_queue(&manager, id)
    });
}
