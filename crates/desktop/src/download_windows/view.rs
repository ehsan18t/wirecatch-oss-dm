//! What a download window shows: the question it asks, read from the core, or a download's
//! progress. A window shows text websites chose, so it acts only on its own question or download
//! (D43). It takes the height its content needs.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, TITLE_BAR_HEIGHT, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, canvas, div, px, size,
};
use wirecatch_protocol::app::{
    AddonRequestItem, CaptureConfirmation, DownloadStatus, DownloadSummary, FileExistsAnswer,
    FileExistsChoice, FileExistsQuestion, MediaChoice, MediaSelection, PER_MILLE_DONE,
    PendingCapture, Question,
};
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_protocol::{DownloadId, QuestionId};
use wirecatch_shell::api;
use wirecatch_shell::download_windows::{Shows, fitted_height};

use crate::backend::{Backend, act, command};
use crate::library::Library;
use crate::ui::small_title_bar::small_title_bar;
use crate::ui::{Callout, Field, Role, Typed as _};
use crate::{files, text};

/// One download window.
pub struct DownloadWindow {
    library: Entity<Library>,
    label: String,
    shows: Shows,
    /// The question, once read; `None` while reading and once it closed.
    question: Option<Question>,
    /// The question is no longer open in the core.
    over: bool,
    /// The file name the person may change.
    name: Entity<InputState>,
    /// The quality picked for a video.
    picked: Option<String>,
    /// Do the same every time, for a file already in the folder.
    remember: bool,
    busy: bool,
    problem: Option<String>,
    /// The content's height when last drawn, for fitting the window to it.
    content: Rc<Cell<Option<Pixels>>>,
    _follow: Subscription,
}

impl DownloadWindow {
    pub fn new(
        library: Entity<Library>,
        label: String,
        shows: Shows,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let follow = cx.observe(&library, |_, _, cx| cx.notify());
        let mut view = Self {
            library,
            label,
            shows,
            question: None,
            over: false,
            name: cx.new(|cx| InputState::new(window, cx)),
            picked: None,
            remember: false,
            busy: false,
            problem: None,
            content: Rc::default(),
            _follow: follow,
        };
        view.reload(cx);
        view
    }

    /// Reads the question again: it moved on to its next step, or closed.
    pub fn reload(&mut self, cx: &mut Context<'_, Self>) {
        let Shows::Question(id) = self.shows else {
            cx.notify();
            return;
        };
        let read = Backend::run(
            cx,
            move |manager| async move { api::question(&manager, id) },
        );
        cx.spawn(async move |view, cx| {
            let Ok(Ok(question)) = read.await else {
                return;
            };
            _ = view.update(cx, |view, cx| {
                view.over = question.is_none();
                if let Some(Question::Capture(capture)) = &question {
                    view.name_from(&capture.file_name, cx);
                }
                if let Some(Question::Choice(choice)) = &question {
                    view.picked = Some(choice.start.option_id.clone());
                }
                view.question = question;
                cx.notify();
            });
        })
        .detach();
    }

    fn name_from(&mut self, name: &str, cx: &mut Context<'_, Self>) {
        let name = name.to_owned();
        let state = self.name.clone();
        cx.defer(move |cx| {
            if let Some(window) = cx.active_window() {
                _ = window.update(cx, |_, window, cx| {
                    state.update(cx, |state, cx| state.set_value(name, window, cx));
                });
            }
        });
    }

    /// Runs `work` on the manager; the answer turns the window into the download's progress, or
    /// shows why it was refused.
    fn answer<F, Fut>(&mut self, work: F, cx: &mut Context<'_, Self>)
    where
        F: FnOnce(wirecatch_core::Manager) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Option<DownloadId>, wirecatch_protocol::wire::ProtocolError>>
            + Send
            + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        self.problem = None;
        cx.notify();
        let done = Backend::run(cx, work);
        cx.spawn(async move |view, cx| {
            let answer = match done.await {
                Ok(Ok(answer)) => answer.map_err(|e| e.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = view.update(cx, |view, cx| {
                view.busy = false;
                match answer {
                    Ok(Some(download)) => view.became(download, cx),
                    Ok(None) => view.close(cx),
                    Err(message) => view.problem = Some(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The question became download `id`: the window shows its progress while the setting asks
    /// for progress windows, else closes.
    fn became(&mut self, id: DownloadId, cx: &mut Context<'_, Self>) {
        let shows_progress = self
            .library
            .read(cx)
            .settings
            .as_ref()
            .is_some_and(|settings| settings.show_progress_windows);
        if !shows_progress {
            return self.close(cx);
        }
        self.shows = Shows::Download(id);
        self.question = None;
        let label = self.label.clone();
        cx.defer(move |cx| super::became_download(&label, id, cx));
    }

    fn close(&mut self, cx: &mut Context<'_, Self>) {
        let label = self.label.clone();
        cx.defer(move |cx| {
            let handle = cx
                .try_global::<super::Windows>()
                .and_then(|windows| windows.open.get(&label).map(|(handle, _)| *handle));
            super::gone(&label, cx);
            if let Some(handle) = handle {
                _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
    }

    fn download(&self, id: DownloadId, cx: &App) -> Option<DownloadSummary> {
        self.library
            .read(cx)
            .downloads
            .iter()
            .find(|row| row.id == id)
            .cloned()
    }

    /// Makes the window as tall as its content, within the screen.
    fn fit(&self, window: &mut Window, cx: &App) {
        let Some(height) = self.content.get() else {
            return;
        };
        // The title bar and its rule above the measured body.
        let wanted = f64::from(f32::from(height + TITLE_BAR_HEIGHT)) + 1.;
        let screen = window
            .display(cx)
            .map(|d| f64::from(f32::from(d.visible_bounds().size.height)));
        let fitted = fitted_height(wanted, screen);
        let now = window.viewport_size();
        let target = px(super::logical(fitted));
        if (now.height - target).abs() > px(1.) {
            window.resize(size(now.width, target));
        }
    }
}

impl Render for DownloadWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        self.fit(window, cx);
        let content = match (self.shows, &self.question) {
            (Shows::Download(id), _) => self.progress(id, cx),
            (Shows::Question(_), _) if self.over => closed_note(cx),
            (Shows::Question(_), None) => div().into_any_element(),
            (Shows::Question(_), Some(question)) => match question.clone() {
                Question::Capture(capture) => self.capture(&capture, cx),
                Question::Reading(reading) => reading_view(reading.id, &reading.url, cx),
                Question::Choice(choice) => self.choice(&choice, cx),
                Question::Addon(item) => addon_view(&item, cx),
                Question::FileExists(question) => self.file_exists(&question, cx),
            },
        };
        let measured = self.content.clone();
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_role(Role::Body, cx)
            .child(small_title_bar(
                div()
                    .text_role(Role::Strong, cx)
                    .child(wirecatch_shell::host::CHANNEL.name()),
                window,
                cx,
            ))
            .child(
                v_flex()
                    .id("download-window-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .relative()
                            .p_4()
                            .gap_4()
                            .when_some(self.problem.clone(), |body, problem| {
                                body.child(Callout::danger(problem))
                            })
                            .child(content)
                            .child(
                                canvas(
                                    move |bounds, _, _| measured.set(Some(bounds.size.height)),
                                    |_, (), _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            ),
                    ),
            )
    }
}

impl DownloadWindow {
    /// A browser file waiting for Download now, Download later or Cancel, with its name to change.
    fn capture(&mut self, capture: &PendingCapture, cx: &mut Context<'_, Self>) -> AnyElement {
        let id = capture.id;
        let busy = self.busy;
        v_flex()
            .gap_4()
            .child(heading(&capture.file_name, &capture.url, capture.size, cx))
            .child(Field::new("Save as", Input::new(&self.name)))
            .child(
                div()
                    .text_role(Role::Meta, cx)
                    .child(format!("In {}", text::folder(&capture.folder))),
            )
            .child(actions(vec![
                button("capture-cancel", "Cancel", ButtonVariant::Ghost, busy).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.answer(
                            move |m| async move { api::reject_capture(&m, id).map(|()| None) },
                            cx,
                        );
                    }),
                ),
                button(
                    "capture-later",
                    "Download later",
                    ButtonVariant::Secondary,
                    busy,
                )
                .on_click(cx.listener(move |view, _, _, cx| view.confirm(id, true, cx))),
                button("capture-now", "Download", ButtonVariant::Primary, busy)
                    .on_click(cx.listener(move |view, _, _, cx| view.confirm(id, false, cx))),
            ]))
            .into_any_element()
    }

    fn confirm(&mut self, id: QuestionId, later: bool, cx: &mut Context<'_, Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        let confirmation = CaptureConfirmation {
            folder: None,
            file_name: (!name.is_empty()).then_some(name),
            queue: None,
            later,
        };
        self.answer(
            move |m| async move { api::confirm_capture(&m, id, confirmation).await.map(Some) },
            cx,
        );
    }

    /// A video waiting for its quality: the qualities it is offered in, the proposed one picked.
    fn choice(&mut self, choice: &MediaChoice, cx: &mut Context<'_, Self>) -> AnyElement {
        let id = choice.id;
        let busy = self.busy;
        let picked = self.picked.clone();
        let theme = cx.theme();
        let options: Vec<AnyElement> = choice
            .options
            .iter()
            .map(|option| {
                let chosen = picked.as_deref() == Some(option.id.as_str());
                let option_id = option.id.clone();
                let size = option
                    .size
                    .map(wirecatch_shell::activity::format_bytes)
                    .unwrap_or_default();
                h_flex()
                    .id(ElementId::Name(format!("quality-{}", option.id).into()))
                    .px_3()
                    .py_2()
                    .gap_3()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .border_1()
                    .border_color(if chosen { theme.primary } else { theme.border })
                    .when(chosen, |row| row.bg(theme.list_active))
                    .child(div().flex_1().child(option.quality.clone()))
                    .child(div().text_role(Role::Meta, cx).child(size))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.picked = Some(option_id.clone());
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let (start, later_start) = (choice.start.clone(), choice.start.clone());
        let page = choice.page_url.clone().unwrap_or_default();
        v_flex()
            .gap_3()
            .child(heading(&choice.title, &page, None, cx))
            .when_some(choice.missing_quality.clone(), |body, missing| {
                body.child(div().text_role(Role::Meta, cx).child(format!(
                    "This video has no {missing}; the closest is picked."
                )))
            })
            .child(v_flex().gap_1p5().children(options))
            .child(actions(vec![
                button("choice-cancel", "Cancel", ButtonVariant::Ghost, busy).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.answer(
                        move |m| async move { api::dismiss_media_choice(&m, id).map(|()| None) },
                        cx,
                    );
                    }),
                ),
                button(
                    "choice-later",
                    "Download later",
                    ButtonVariant::Secondary,
                    busy,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.choose(id, later_start.clone(), true, cx);
                })),
                button("choice-now", "Download", ButtonVariant::Primary, busy).on_click(
                    cx.listener(move |view, _, _, cx| view.choose(id, start.clone(), false, cx)),
                ),
            ]))
            .into_any_element()
    }

    /// Downloads the picked quality, saved the way the window proposed.
    fn choose(
        &mut self,
        id: QuestionId,
        mut selection: MediaSelection,
        later: bool,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(picked) = self.picked.clone() {
            selection.option_id = picked;
        }
        selection.later = later;
        self.answer(
            move |m| async move { api::choose_media(&m, id, selection).await.map(Some) },
            cx,
        );
    }

    /// A file already in the folder: Replace, Keep both or Cancel, and whether to do the same
    /// every time (D96).
    fn file_exists(
        &mut self,
        question: &FileExistsQuestion,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let id = question.id;
        let busy = self.busy;
        v_flex()
            .gap_4()
            .child(heading(&question.file_name, &question.url, None, cx))
            .child(div().child(format!(
                "A file with this name is already in {}.",
                text::folder(&question.folder)
            )))
            .child(
                Checkbox::new("file-exists-remember")
                    .label("Do the same every time")
                    .checked(self.remember)
                    .on_click(cx.listener(|view, checked: &bool, _, cx| {
                        view.remember = *checked;
                        cx.notify();
                    })),
            )
            .child(actions(vec![
                button("exists-cancel", "Cancel", ButtonVariant::Ghost, busy).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.settle(id, FileExistsChoice::Cancel, cx);
                    }),
                ),
                button("exists-both", "Keep both", ButtonVariant::Secondary, busy).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.settle(id, FileExistsChoice::KeepBoth, cx);
                    }),
                ),
                button("exists-replace", "Replace", ButtonVariant::Primary, busy).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.settle(id, FileExistsChoice::Replace, cx);
                    }),
                ),
            ]))
            .into_any_element()
    }

    fn settle(&mut self, id: QuestionId, choice: FileExistsChoice, cx: &mut Context<'_, Self>) {
        let answer = FileExistsAnswer {
            choice,
            remember: self.remember,
        };
        self.answer(
            move |m| async move { api::answer_file_exists(&m, id, answer).map(Some) },
            cx,
        );
    }

    /// A download's progress, with what can be done with it now.
    fn progress(&mut self, id: DownloadId, cx: &mut Context<'_, Self>) -> AnyElement {
        let Some(row) = self.download(id, cx) else {
            return closed_note(cx);
        };
        let per_mille = text::per_mille(&row);
        let line = if row.status == DownloadStatus::Downloading && row.speed > 0 {
            [
                Some(text::speed(row.speed)),
                text::percent(&row),
                row.eta_secs.map(|left| format!("{} left", text::eta(left))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
        } else {
            text::status_short(&row)
        };
        v_flex()
            .gap_3()
            .child(heading(&row.file_name, &row.url, row.total, cx))
            .when(row.status != DownloadStatus::Completed, |body| {
                body.child(
                    Progress::new("progress-bar")
                        .loading(per_mille.is_none())
                        .value(per_mille.map_or(0., |done| {
                            f32::from(done) * 100. / f32::from(PER_MILLE_DONE)
                        })),
                )
            })
            .child(div().text_role(Role::Meta, cx).child(line))
            .child(actions(progress_buttons(id, row.status)))
            .into_any_element()
    }
}

/// What a download's progress window offers in its state.
fn progress_buttons(id: DownloadId, status: DownloadStatus) -> Vec<Button> {
    let command =
        |button_id,
         label,
         variant,
         command: fn(&wirecatch_core::Manager, DownloadId) -> Result<(), ProtocolError>| {
            button(button_id, label, variant, false).on_click(move |_, window, cx| {
                act(window.window_handle(), cx, move |m| command(m, id));
            })
        };
    match status {
        DownloadStatus::Completed => vec![
            button(
                "progress-folder",
                "Show in folder",
                ButtonVariant::Secondary,
                false,
            )
            .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            button("progress-open", "Open", ButtonVariant::Primary, false)
                .on_click(move |_, window, cx| files::open(id, window, cx)),
        ],
        DownloadStatus::Paused => vec![
            command(
                "progress-cancel",
                "Cancel",
                ButtonVariant::Ghost,
                api::cancel,
            ),
            command(
                "progress-resume",
                "Resume",
                ButtonVariant::Primary,
                api::resume,
            ),
        ],
        DownloadStatus::Failed | DownloadStatus::Cancelled => {
            vec![command(
                "progress-retry",
                "Try again",
                ButtonVariant::Primary,
                api::retry,
            )]
        }
        _ => vec![
            command(
                "progress-cancel",
                "Cancel",
                ButtonVariant::Ghost,
                api::cancel,
            ),
            command(
                "progress-pause",
                "Pause",
                ButtonVariant::Secondary,
                api::pause,
            ),
        ],
    }
}

/// A video page being read for its qualities.
fn reading_view(id: QuestionId, url: &str, cx: &mut Context<'_, DownloadWindow>) -> AnyElement {
    v_flex()
        .gap_4()
        .child(heading("Reading the video", url, None, cx))
        .child(Progress::new("reading").loading(true))
        .child(actions(vec![
            button("reading-cancel", "Cancel", ButtonVariant::Ghost, false).on_click(cx.listener(
                move |view, _, _, cx| {
                    view.answer(
                        move |m| async move { api::dismiss_media_choice(&m, id).map(|()| None) },
                        cx,
                    );
                },
            )),
        ]))
        .into_any_element()
}

/// A video page waiting for the media add-on.
fn addon_view(item: &AddonRequestItem, cx: &mut Context<'_, DownloadWindow>) -> AnyElement {
    let (id, addon) = (item.id, item.addon.id);
    let title = format!("This video needs the {}", item.addon.name.to_lowercase());
    v_flex()
        .gap_4()
        .child(heading(&title, &item.url, None, cx))
        .child(div().child(
            "Install it to download videos from sites like this one. The download continues once it is installed.",
        ))
        .child(actions(vec![
            button("addon-cancel", "Cancel", ButtonVariant::Ghost, false).on_click(cx.listener(
                move |view, _, _, cx| {
                    view.answer(
                        move |m| async move { api::dismiss_media_choice(&m, id).map(|()| None) },
                        cx,
                    );
                },
            )),
            button("addon-install", "Install", ButtonVariant::Primary, false).on_click(
                move |_, window, cx| {
                    command(window.window_handle(), cx, move |m| async move {
                        api::install_addon(&m, addon).await
                    });
                },
            ),
        ]))
        .into_any_element()
}

/// The question was answered elsewhere, or the download is gone.
fn closed_note(cx: &mut Context<'_, DownloadWindow>) -> AnyElement {
    v_flex()
        .gap_4()
        .child(div().child("This was already answered, or is no longer waiting."))
        .child(actions(vec![
            button("closed-close", "Close", ButtonVariant::Secondary, false)
                .on_click(cx.listener(|view, _, _, cx| view.close(cx))),
        ]))
        .into_any_element()
}

/// The name, where it comes from, and its size when known.
fn heading(name: &str, link: &str, size: Option<u64>, cx: &App) -> impl IntoElement {
    let host = text::host(link).to_owned();
    let meta = match size {
        Some(size) => format!("{host} · {}", wirecatch_shell::activity::format_bytes(size)),
        None => host,
    };
    v_flex()
        .gap_0p5()
        .child(div().text_role(Role::Title, cx).child(name.to_owned()))
        .child(div().text_role(Role::Meta, cx).child(meta))
}

/// The window's buttons, at its foot, the main one last.
fn actions(buttons: Vec<Button>) -> impl IntoElement {
    h_flex().gap_2().justify_end().children(buttons)
}

fn button(id: &'static str, label: &'static str, variant: ButtonVariant, busy: bool) -> Button {
    Button::new(id)
        .label(label)
        .with_variant(variant)
        .disabled(busy)
}
