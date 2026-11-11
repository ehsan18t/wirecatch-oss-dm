//! Get a new link from a browser capture or by pasting it (D43.5).

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Task, Window, div, px,
};
use wirecatch_core::LinkFrom;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::extension::MediaKind;
use wirecatch_shell::api;

use crate::backend::{Backend, said};
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _};
use crate::{links, text};

const WIDTH: f32 = 520.;
const LINK_SAVED: &str =
    "New link saved. The download continues where it stopped if the server allows it.";

enum Mode {
    Opening,
    Watching,
    Paste,
    Saved,
}

/// The form inside the dialog.
pub struct RefreshForm {
    library: Entity<Library>,
    id: DownloadId,
    link: Entity<InputState>,
    read_link: links::Kept,
    busy: bool,
    problem: Option<String>,
    mode: Mode,
    watching: Option<Task<()>>,
    _edited: Subscription,
}

/// Opens Get a new link for download `id`.
pub fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| RefreshForm::new(library, id, window, cx));
    form.update(cx, |form, cx| form.watch(window, cx));
    crate::ui::dialog::open(
        window,
        cx,
        |_| "Get a new link".into(),
        move |dialog, _, cx| {
            let form_state = form.read(cx);
            let found = form_state.found(cx).is_some();
            let busy = form_state.busy;
            let paste = matches!(form_state.mode, Mode::Paste);
            let complete = matches!(form_state.mode, Mode::Saved);
            let (saved, entered) = (form.clone(), form.clone());
            let manual = form.clone();
            let (cancelled, closed) = (form.clone(), form.clone());
            dialog
                .w(px(WIDTH))
                .child(form.clone())
                .on_ok(move |_, window, cx| {
                    entered.update(cx, |form, cx| form.save(window, cx));
                    false
                })
                .on_close(move |_, _, cx| {
                    closed.update(cx, |form, _| form.watching = None);
                })
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("refresh-cancel")
                                .label(if complete { "Close" } else { "Cancel" })
                                .on_click(move |_, window, cx| {
                                    cancelled.update(cx, |form, _| form.watching = None);
                                    window.close_dialog(cx);
                                }),
                        )
                        .children((!paste && !complete).then(|| {
                            Button::new("refresh-paste").label("Paste a link").on_click(
                                move |_, window, cx| {
                                    manual.update(cx, |form, cx| form.paste(window, cx));
                                },
                            )
                        }))
                        .children(paste.then(|| {
                            Button::new("refresh-save")
                                .primary()
                                .label("Use this link")
                                .disabled(!found)
                                .loading(busy)
                                .on_click(move |_, window, cx| {
                                    saved.update(cx, |form, cx| form.save(window, cx));
                                })
                        })),
                )
        },
    );
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
            read_link: links::Kept::default(),
            busy: false,
            problem: None,
            mode: Mode::Opening,
            watching: None,
            _edited: edited,
        }
    }

    fn watch(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let id = self.id;
        let (saved, result) = tokio::sync::oneshot::channel();
        let reading = Backend::run(cx, move |manager| async move {
            api::watch_link(&manager, id, saved)
        });
        self.watching = Some(cx.spawn_in(window, async move |form, cx| {
            let page = match said(reading.await) {
                Some(Ok(Some(page))) => page,
                answer => {
                    crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                        if let Some(Err(problem)) = answer {
                            form.problem = Some(problem);
                        }
                        form.show_paste(window, cx);
                    }));
                    return;
                }
            };
            let opened = form.update_in(cx, |form, _, cx| {
                form.mode = Mode::Watching;
                cx.open_url(page.as_str());
                cx.notify();
            });
            if opened.is_err() {
                crate::closing::may_have_closed(opened);
                return;
            }
            let saved = result.await.is_ok();
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                if saved {
                    form.mode = Mode::Saved;
                    cx.notify();
                } else {
                    form.problem =
                        Some("The browser watch ended. You can paste a new link.".into());
                    form.show_paste(window, cx);
                }
            }));
        }));
    }

    fn paste(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.watching = None;
        self.show_paste(window, cx);
    }

    fn show_paste(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.mode = Mode::Paste;
        crate::ui::dialog::focus_after_opening(&self.link, window, cx);
        cx.notify();
    }

    /// The link in the field, if it holds one.
    fn found(&self, cx: &App) -> Option<String> {
        let found = self.read_link.of(&self.link.read(cx).value());
        found.whole.clone().or_else(|| found.urls.first().cloned())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(url) = self
            .found(cx)
            .filter(|_| !self.busy && matches!(self.mode, Mode::Paste))
        else {
            return;
        };
        self.busy = true;
        cx.notify();
        let id = self.id;
        let saved = Backend::run(cx, move |manager| async move {
            api::refresh_link(&manager, id, &url, LinkFrom::Person)
        });
        cx.spawn_in(window, async move |form, cx| {
            let Some(answer) = said(saved.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                form.busy = false;
                match answer {
                    Ok(()) => {
                        window.close_dialog(cx);
                        window.push_notification(LINK_SAVED, cx);
                    }
                    Err(message) => form.problem = Some(message),
                }
                cx.notify();
            }));
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
        let paste_note = if let Some(problem) = &self.problem {
            problem.clone()
        } else if wrong {
            "Paste the whole link, starting with https://.".to_owned()
        } else {
            let page_video = row
                .as_ref()
                .and_then(|row| row.media.as_ref())
                .is_some_and(|media| media.source == MediaKind::Page);
            let kept = if page_video {
                "A video read from a page starts over if the link is a different one."
            } else {
                "What is already downloaded is kept if the server can continue the download; otherwise it starts over."
            };
            format!(
                "Links from some sites stop working after a while. Start the download again on its page, copy the new link and paste it here. {kept}"
            )
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
            .children(matches!(self.mode, Mode::Paste).then(|| {
                Field::new("New link", Input::new(&self.link))
                    .note(paste_note)
                    .wrong(wrong || self.problem.is_some())
            }))
            .children(match self.mode {
                Mode::Opening => Some("Opening the download’s source page…"),
                Mode::Watching => Some("Start the same download again on the page that opened. Keep browser capture enabled while waiting for the new link."),
                Mode::Saved => Some(LINK_SAVED),
                Mode::Paste => None,
            }.map(|note| div().text_role(Role::Secondary, cx).child(note)))
    }
}
