//! Get a new link: a download whose link stopped working takes a fresh one, pasted from the page
//! it came from, and continues where it stopped. From the main window the link is the person's
//! own (D43.5).

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div, px,
};
use wirecatch_core::LinkFrom;
use wirecatch_protocol::DownloadId;
use wirecatch_shell::api;

use crate::backend::Backend;
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _};
use crate::{links, text};

const WIDTH: f32 = 520.;

/// The form inside the dialog.
pub struct RefreshForm {
    library: Entity<Library>,
    id: DownloadId,
    link: Entity<InputState>,
    busy: bool,
    problem: Option<String>,
    _edited: Subscription,
}

/// Opens Get a new link for download `id`.
pub fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| RefreshForm::new(library, id, window, cx));
    let link = form.read(cx).link.clone();
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let found = form.read(cx).found(cx).is_some();
        let busy = form.read(cx).busy;
        let (saved, entered) = (form.clone(), form.clone());
        dialog
            .title("Get a new link")
            .w(px(WIDTH))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                entered.update(cx, |form, cx| form.save(window, cx));
                false
            })
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("refresh-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("refresh-save")
                            .primary()
                            .label("Use this link")
                            .disabled(!found)
                            .loading(busy)
                            .on_click(move |_, window, cx| {
                                saved.update(cx, |form, cx| form.save(window, cx));
                            }),
                    ),
            )
    });
    crate::ui::dialog::focus_after_opening(&link, window, cx);
}

impl RefreshForm {
    fn new(
        library: Entity<Library>,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let link = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let edited = cx.subscribe(&link, |form: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                form.problem = None;
                cx.notify();
            }
        });
        Self {
            library,
            id,
            link,
            busy: false,
            problem: None,
            _edited: edited,
        }
    }

    /// The link in the field, if it holds one.
    fn found(&self, cx: &App) -> Option<String> {
        links::extract(&self.link.read(cx).value())
            .into_iter()
            .next()
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(url) = self.found(cx).filter(|_| !self.busy) else {
            return;
        };
        self.busy = true;
        cx.notify();
        let id = self.id;
        let saved = Backend::run(cx, move |manager| async move {
            api::refresh_link(&manager, id, &url, LinkFrom::Person)
        });
        cx.spawn_in(window, async move |form, cx| {
            let answer = match saved.await {
                Ok(Ok(answer)) => answer.map_err(|e| e.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = form.update_in(cx, |form, window, cx| {
                form.busy = false;
                match answer {
                    Ok(()) => {
                        window.close_dialog(cx);
                        window.push_notification(
                            "New link saved. The download continues where it stopped.",
                            cx,
                        );
                    }
                    Err(message) => form.problem = Some(message),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for RefreshForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let typed = !self.link.read(cx).value().trim().is_empty();
        let wrong = typed && self.found(cx).is_none();
        let row = self
            .library
            .read(cx)
            .downloads
            .iter()
            .find(|row| row.id == self.id)
            .cloned();
        let note = if let Some(problem) = &self.problem {
            problem.clone()
        } else if wrong {
            "Paste the whole link, starting with https://.".to_owned()
        } else {
            "Links from some sites stop working after a while. Start the download again on its page, copy the new link and paste it here. What is already downloaded is kept.".to_owned()
        };
        v_flex()
            .gap_4()
            .pb_1()
            .children(row.map(|row| {
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_role(Role::Strong, cx)
                            .child(row.file_name.clone()),
                    )
                    .child(
                        div()
                            .text_role(Role::Meta, cx)
                            .child(format!("from {}", text::host(&row.url))),
                    )
            }))
            .child(
                Field::new("New link", Input::new(&self.link))
                    .note(note)
                    .wrong(wrong || self.problem.is_some()),
            )
    }
}
