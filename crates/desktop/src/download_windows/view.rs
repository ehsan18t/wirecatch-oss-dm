//! What a download window shows: the question it asks, read from the core, or a download's
//! progress. A window shows text websites chose, so it acts only on its own question or download
//! (D43). It takes the height its content needs.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, TITLE_BAR_HEIGHT, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, canvas, div, px, relative, size,
};
use wirecatch_protocol::app::{
    AddonRequestItem, AppSettingsPatch, CaptureConfirmation, DownloadStatus, DownloadSummary,
    FileExistsAnswer, FileExistsChoice, FileExistsQuestion, MediaChoice, MediaDefaultsPatch,
    PER_MILLE_DONE, PendingCapture, Question,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::rules::QueueReason;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::api;
use wirecatch_shell::download_windows::{Shows, fitted_height};

use super::media_form::{MediaAnswer, MediaForm};
use super::run_record::{Look, RunRecord};
use crate::backend::{Backend, act, command};
use crate::library::Library;
use crate::ui::small_title_bar::small_title_bar;
use crate::ui::speed_graph::run_graph;
use crate::ui::{Callout, Field, Role, Typed as _, tokens};
use crate::{files, text};

type ChooseQueue = Rc<dyn Fn(Option<QueueId>, &mut App)>;

const GRAPH_HEIGHT: f32 = 96.;
const STALL_CHECK: std::time::Duration = std::time::Duration::from_secs(1);
/// A file's question and its progress share this height: the window stays still between them and
/// Pause lands where Download was.
const FILE_CONTENT_HEIGHT: f32 = 288.;
/// How strongly a queue's name is tinted with the accent colour.
const BADGE_TINT: f32 = 0.14;
/// The queue picker's widest, leaving the row's buttons their full labels.
const PICKER_WIDTH: f32 = 136.;

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
    /// The queue picked for a file; `None` is Automatic.
    queue: Option<QueueId>,
    /// The folder picked for a file; `None` is where its queue saves.
    folder: Option<String>,
    /// What this window saw of the download's run.
    record: RunRecord,
    /// The form of a video's quality question.
    media: Option<(Entity<MediaForm>, Subscription)>,
    /// Do the same every time, for a file already in the folder.
    remember: bool,
    busy: bool,
    problem: Option<String>,
    /// The content's height when last drawn, for fitting the window to it.
    content: Rc<Cell<Option<Pixels>>>,
    _tick: gpui_kit::Task<()>,
    _follow: Subscription,
    _follow_system: Subscription,
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
        // Nothing else redraws a download that gets no bytes, so its stall would never show.
        let tick = cx.spawn(async move |view: gpui_kit::WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(STALL_CHECK).await;
                let alive = view.update(cx, |view, cx| {
                    if matches!(view.shows, Shows::Download(_)) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        let mut view = Self {
            library,
            label,
            shows,
            question: None,
            over: false,
            name: cx.new(|cx| InputState::new(window, cx)),
            queue: None,
            folder: None,
            record: RunRecord::default(),
            media: None,
            remember: false,
            busy: false,
            problem: None,
            content: Rc::default(),
            _tick: tick,
            _follow: follow,
            _follow_system: crate::theme::follow_system(window, cx),
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
                // This window's own answer closed it: the form stays until the answer says what follows.
                if matches!(view.shows, Shows::Download(_)) || (view.busy && question.is_none()) {
                    return;
                }
                view.over = question.is_none();
                if let Some(Question::Capture(capture)) = &question {
                    view.name_from(&capture.file_name, cx);
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
        self.answer_then(false, work, cx);
    }

    /// Sends an answer; with `later`, Download later, it leaves no window behind.
    fn answer_then<F, Fut>(&mut self, later: bool, work: F, cx: &mut Context<'_, Self>)
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
                    Ok(Some(_)) if later => view.close(cx),
                    Ok(Some(download)) => view.became(download, cx),
                    Ok(None) => view.close(cx),
                    Err(message) => {
                        view.problem = Some(message);
                        view.reload(cx);
                    }
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
        let label = self.label.clone();
        cx.defer(move |cx| super::became_download(&label, id, cx));
    }

    fn close(&mut self, cx: &mut Context<'_, Self>) {
        let label = self.label.clone();
        cx.defer(move |cx| {
            let handle = cx
                .global::<super::Windows>()
                .open
                .get(&label)
                .map(|(handle, _)| *handle);
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
            // The download's row arrives just after the answer: the form stays until then.
            (Shows::Download(id), Some(Question::Capture(capture)))
                if self.download(id, cx).is_none() =>
            {
                let capture = capture.clone();
                self.capture(&capture, cx)
            }
            (Shows::Download(id), Some(Question::Choice(choice)))
                if self.download(id, cx).is_none() =>
            {
                let choice = choice.clone();
                self.media_form(&choice, window, cx)
            }
            (Shows::Download(id), _) => self.progress(id, cx),
            (Shows::Question(_), _) if self.over => closed_note(cx),
            (Shows::Question(_), None) => div().into_any_element(),
            (Shows::Question(_), Some(question)) => match question.clone() {
                Question::Capture(capture) => self.capture(&capture, cx),
                Question::Reading(reading) => reading_view(reading.id, &reading.url, cx),
                Question::Choice(choice) => self.media_form(&choice, window, cx),
                Question::Addon(item) => addon_view(&item, cx),
                Question::FileExists(question) => self.file_exists(&question, cx),
            },
        };
        let strip = self.strip(cx);
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
                            .child(
                                v_flex()
                                    .p_4()
                                    .gap_4()
                                    .when_some(self.problem.clone(), |body, problem| {
                                        body.child(Callout::danger(problem))
                                    })
                                    .child(content),
                            )
                            .children(strip)
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        // `fit` reads this on the next draw: without one, a window
                                        // whose content grew stays short.
                                        if measured.replace(Some(bounds.size.height))
                                            != Some(bounds.size.height)
                                        {
                                            window.request_animation_frame();
                                        }
                                    },
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
    /// The switch along the window's foot, for what it shows: the same setting the extension's
    /// popup and Settings have, which applies from the next download on.
    fn strip(&self, cx: &App) -> Option<AnyElement> {
        // Read only once there is something to show: a question's window first draws while the
        // library is still updating, and reading it then panics.
        let settings = || self.library.read(cx).settings.as_ref();
        let (label, checked, patch): (_, _, fn(bool) -> AppSettingsPatch) =
            match (self.shows, &self.question) {
                (Shows::Question(_), Some(Question::Capture(_))) => (
                    "Ask before each download",
                    settings()?.extension.ask_before_download,
                    |on| AppSettingsPatch {
                        extension: Some(ExtensionSettingsPatch {
                            ask_before_download: Some(on),
                            ..ExtensionSettingsPatch::default()
                        }),
                        ..AppSettingsPatch::default()
                    },
                ),
                (Shows::Question(_), Some(Question::Choice(_))) => (
                    "Ask before each video",
                    settings()?.media.ask_each_time,
                    |on| AppSettingsPatch {
                        media: Some(MediaDefaultsPatch {
                            ask_each_time: Some(on),
                            ..MediaDefaultsPatch::default()
                        }),
                        ..AppSettingsPatch::default()
                    },
                ),
                (Shows::Download(_), _) => (
                    "Show progress while downloading",
                    settings()?.show_progress_windows,
                    |on| AppSettingsPatch {
                        show_progress_windows: Some(on),
                        ..AppSettingsPatch::default()
                    },
                ),
                _ => return None,
            };
        let theme = cx.theme();
        Some(
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .child(
                    Switch::new("window-switch")
                        .small()
                        .checked(checked)
                        .accessibility_label(label)
                        .on_click(move |on: &bool, window, cx| {
                            let patch = patch(*on);
                            act(window.window_handle(), cx, move |manager| {
                                api::change_settings(manager, patch)
                            });
                        }),
                )
                .child(div().text_role(Role::Meta, cx).child(label))
                .into_any_element(),
        )
    }

    /// A browser file waiting for Download now, Download later or Cancel, with its name to change.
    fn capture(&mut self, capture: &PendingCapture, cx: &mut Context<'_, Self>) -> AnyElement {
        let id = capture.id;
        let busy = self.busy || matches!(self.shows, Shows::Download(_));
        v_flex()
            .min_h(px(FILE_CONTENT_HEIGHT))
            .justify_between()
            .gap_4()
            .child(heading(&capture.file_name, &capture.url, capture.size, cx))
            .child(Field::new("Save as", Input::new(&self.name)))
            .child(Field::new("Folder", self.folder_field(capture, cx)))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .child(self.queue_picker(capture, cx)),
                    )
                    .children(vec![
                        button("capture-cancel", "Cancel", ButtonVariant::Ghost, busy).on_click(
                            cx.listener(move |view, _, _, cx| {
                                view.answer(
                                    move |m| async move {
                                        api::reject_capture(&m, id).map(|()| None)
                                    },
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
                        button("capture-now", "Download", ButtonVariant::Primary, busy).on_click(
                            cx.listener(move |view, _, _, cx| view.confirm(id, false, cx)),
                        ),
                    ]),
            )
            .into_any_element()
    }

    /// Where the file goes, the whole width; a click picks another folder.
    fn folder_field(&self, capture: &PendingCapture, cx: &mut Context<'_, Self>) -> AnyElement {
        let library = self.library.read(cx);
        let queue_folder = self
            .queue
            .and_then(|queue| library.queue(queue))
            .map(|queue| {
                queue
                    .settings
                    .folder
                    .own_ref()
                    .map_or_else(|| capture.type_folder.clone(), |folder| folder.0.clone())
            });
        let shown = self
            .folder
            .clone()
            .or(queue_folder)
            .unwrap_or_else(|| capture.folder.clone());
        let pick = cx.listener(|_, _, _, cx| {
            let picked = files::choose_folder(cx);
            cx.spawn(async move |view, cx| {
                let Some(folder) = picked.await else {
                    return;
                };
                _ = view.update(cx, |view, cx| {
                    view.folder = Some(folder);
                    cx.notify();
                });
            })
            .detach();
        });
        folder_box("capture-folder", &shown, pick, cx)
    }

    /// The queue the file joins, styled like the queue's badge: a bolt while Automatic picks it.
    /// Its menu and tooltip say why Automatic picked that one.
    fn queue_picker(&self, capture: &PendingCapture, cx: &mut Context<'_, Self>) -> AnyElement {
        let view = cx.entity().downgrade();
        let choose = Rc::new(move |queue: Option<QueueId>, cx: &mut App| {
            _ = view.update(cx, |view, cx| {
                view.queue = queue;
                cx.notify();
            });
        });
        queue_badge_picker(
            "capture-queue",
            capture.queue,
            &capture.queue_reason,
            self.queue,
            choose,
            cx,
        )
    }

    fn confirm(&mut self, id: QuestionId, later: bool, cx: &mut Context<'_, Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        let confirmation = CaptureConfirmation {
            folder: self.folder.clone(),
            file_name: (!name.is_empty()).then_some(name),
            queue: self.queue,
            later,
        };
        self.answer_then(
            later,
            move |m| async move { api::confirm_capture(&m, id, confirmation).await.map(Some) },
            cx,
        );
    }

    /// A video waiting for its quality: its form, kept for as long as the question lasts.
    fn media_form(
        &mut self,
        choice: &MediaChoice,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let id = choice.id;
        let same = self
            .media
            .as_ref()
            .is_some_and(|(form, _)| form.read(cx).question() == id);
        if !same {
            let library = self.library.clone();
            let question = choice.clone();
            let form = cx.new(|cx| MediaForm::new(question, library, window, cx));
            let answers = cx.subscribe(
                &form,
                move |view, _, answer: &MediaAnswer, cx| match answer {
                    MediaAnswer::Cancel => view.answer(
                        move |m| async move { api::dismiss_media_choice(&m, id).map(|()| None) },
                        cx,
                    ),
                    MediaAnswer::Choose(selection) => {
                        let selection = (**selection).clone();
                        view.answer_then(
                            selection.later,
                            move |m| async move {
                                api::choose_media(&m, id, selection).await.map(Some)
                            },
                            cx,
                        );
                    }
                },
            );
            self.media = Some((form, answers));
        }
        let busy = self.busy || matches!(self.shows, Shows::Download(_));
        let Some((form, _)) = &self.media else {
            return div().into_any_element();
        };
        form.update(cx, |form, _| form.busy = busy);
        form.clone().into_any_element()
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

    /// A download's progress, or how it ended, with what can be done with it now. Every state
    /// has the same layout, so the window keeps its size and the buttons stay under the pointer.
    fn progress(&mut self, id: DownloadId, cx: &mut Context<'_, Self>) -> AnyElement {
        let Some(row) = self.download(id, cx) else {
            return closed_note(cx);
        };
        self.question = None;
        self.media = None;
        let per_mille = text::per_mille(&row);
        self.record.see(
            Look {
                per_mille,
                speed: row.speed,
                done: row.done,
                running: row.status == DownloadStatus::Downloading,
            },
            Instant::now(),
        );
        let queue = self
            .library
            .read(cx)
            .queue(row.queue)
            .map(|queue| queue.settings.name.clone());
        let folder = Path::new(&row.path)
            .parent()
            .map(|folder| text::folder(&folder.to_string_lossy()));
        let buttons = progress_buttons(id, row.status, cx);
        let theme = cx.theme();
        v_flex()
            .min_h(px(FILE_CONTENT_HEIGHT))
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .child(div().flex_1().min_w_0().child(heading(
                        &row.file_name,
                        &row.url,
                        row.total,
                        cx,
                    )))
                    .children(state_mark(&row, self.record.stalled(Instant::now()), cx)),
            )
            .children(self.run_panel(&row, per_mille, cx))
            .child(self.facts(&row, cx))
            .when_some(folder, |body, folder| {
                body.child(
                    h_flex()
                        .gap_1p5()
                        .text_role(Role::Meta, cx)
                        .child(
                            Icon::new(Lucide::Folder)
                                .with_size(tokens::ICON)
                                .text_color(theme.muted_foreground),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(folder)),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .children(queue.map(|name| queue_badge(name, cx))),
                    )
                    .children(buttons),
            )
            .into_any_element()
    }

    /// The progress bar and speed graph in one: across is how much is done, height is how fast,
    /// with the speed and percent over it. A download too short to draw shows a plain full bar.
    fn run_panel(
        &self,
        row: &DownloadSummary,
        per_mille: Option<u16>,
        cx: &App,
    ) -> Option<AnyElement> {
        let over = matches!(
            row.status,
            DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled
        );
        if over && !self.record.drawable() {
            return (row.status == DownloadStatus::Completed)
                .then(|| Progress::new("progress-bar").value(100.).into_any_element());
        }
        let theme = cx.theme();
        let color = match row.status {
            DownloadStatus::Downloading if self.record.stalled(Instant::now()) => {
                theme.muted_foreground
            }
            DownloadStatus::Completed | DownloadStatus::Downloading => theme.primary,
            DownloadStatus::Failed => theme.danger,
            _ => theme.muted_foreground,
        };
        let label = match row.status {
            DownloadStatus::Downloading if self.record.stalled(Instant::now()) => {
                Some("Waiting for the server".to_owned())
            }
            DownloadStatus::Downloading if row.speed > 0 => Some(text::speed(row.speed)),
            DownloadStatus::Completed => self
                .record
                .peak()
                .map(|peak| format!("Peak {}", text::speed(peak))),
            DownloadStatus::Failed | DownloadStatus::Cancelled => None,
            _ => Some(text::status_short(row)),
        };
        let front = per_mille
            .filter(|_| {
                row.total.is_some_and(|total| total > 0)
                    && !matches!(
                        row.status,
                        DownloadStatus::Completed
                            | DownloadStatus::Processing
                            | DownloadStatus::Cancelled
                    )
            })
            .map(|done| f32::from(done) / f32::from(PER_MILLE_DONE));
        // Cancelling zeroes the row's progress: the curve stays where the run got to.
        let end = match row.status {
            DownloadStatus::Cancelled => self.record.reached(),
            _ => front,
        };
        let graph = self.record.drawable().then(|| {
            run_graph(self.record.points(end.unwrap_or(1.)), front, color)
                .absolute()
                .size_full()
        });
        Some(
            v_flex()
                .w_full()
                .h(px(GRAPH_HEIGHT))
                .rounded(theme.radius)
                .bg(theme.secondary)
                .overflow_hidden()
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .px_2p5()
                        .py_1p5()
                        .justify_between()
                        .child(div().text_role(Role::Strong, cx).children(label))
                        .child(div().text_role(Role::Meta, cx).children(
                            text::percent(row).filter(|_| {
                                !matches!(
                                    row.status,
                                    DownloadStatus::Completed | DownloadStatus::Processing
                                )
                            }),
                        )),
                )
                .child(div().relative().flex_1().min_h_0().children(graph))
                .into_any_element(),
        )
    }

    /// The figures under the graph, by state; why it failed takes their place.
    fn facts(&self, row: &DownloadSummary, cx: &App) -> AnyElement {
        if let (DownloadStatus::Failed, Some(error)) = (row.status, &row.error) {
            let kept = (row.done > 0).then(|| match row.total {
                Some(total) => format!(
                    "Downloaded {} of {}",
                    format_bytes(row.done),
                    format_bytes(total)
                ),
                None => format!("Downloaded {}", format_bytes(row.done)),
            });
            return v_flex()
                .gap_1()
                .child(
                    div()
                        .text_color(cx.theme().danger)
                        .child(error.message.clone()),
                )
                .children(kept.map(|kept| div().text_role(Role::Meta, cx).child(kept)))
                .into_any_element();
        }
        let average = self
            .record
            .average()
            .map(|bytes| ("Average", text::speed(bytes)));
        let ran = self.record.ran().map(|ran| ran.as_secs());
        let facts: Vec<(&str, String)> = match row.status {
            DownloadStatus::Completed => vec![
                Some(("Size", format_bytes(row.total.unwrap_or(row.done)))),
                ran.map(|secs| ("Took", text::eta(secs))),
                average,
                row.completed_at.map(|at| ("Finished at", text::clock(at))),
            ],
            DownloadStatus::Paused => vec![
                Some(("Downloaded", format_bytes(row.done))),
                row.total
                    .map(|total| ("Remaining", format_bytes(total.saturating_sub(row.done)))),
                average,
                ran.map(|secs| ("Time so far", text::eta(secs))),
            ],
            DownloadStatus::Cancelled => vec![Some(("Downloaded", format_bytes(row.done)))],
            _ => vec![
                Some(("Downloaded", format_bytes(row.done))),
                Some((
                    "Time left",
                    row.eta_secs
                        .filter(|_| !self.record.stalled(Instant::now()))
                        .map_or_else(|| "Unknown".to_owned(), text::eta),
                )),
                average,
                (row.connections > 0).then(|| ("Connections", row.connections.to_string())),
            ],
        }
        .into_iter()
        .flatten()
        .collect();
        h_flex()
            .w_full()
            .children(facts.into_iter().map(|(label, value)| {
                v_flex()
                    .w(relative(0.25))
                    .min_w_0()
                    .child(div().text_role(Role::Meta, cx).child(label))
                    .child(div().truncate().child(value))
            }))
            .into_any_element()
    }
}

/// The queue a file joins, styled like the queue's badge: a bolt while Automatic picks it. Its menu
/// and tooltip say why Automatic picked that one.
pub(super) fn queue_badge_picker(
    id: &'static str,
    proposed: QueueId,
    reason: &QueueReason,
    picked: Option<QueueId>,
    choose: ChooseQueue,
    cx: &App,
) -> AnyElement {
    let library = cx.global::<super::Windows>().library.read(cx);
    let name_of = |id: QueueId| {
        library
            .queue(id)
            .map(|queue| queue.settings.name.clone())
            .unwrap_or_default()
    };
    let proposed = name_of(proposed);
    let (lead, value) = automatic_reason(&proposed, reason);
    let tip = value
        .as_ref()
        .map_or_else(|| lead.clone(), |value| format!("{lead} {value}"));
    let shown = picked.map_or_else(|| proposed.clone(), name_of);
    let queues: Vec<(QueueId, String)> = library
        .queues
        .iter()
        .map(|queue| (queue.id, queue.settings.name.clone()))
        .collect();
    let accent = cx.theme().primary;
    Button::new(id)
        .ghost()
        .icon(Icon::new(if picked.is_none() {
            Lucide::Zap
        } else {
            Lucide::ListOrdered
        }))
        .label(shown)
        .dropdown_caret(true)
        .max_w(px(PICKER_WIDTH))
        .bg(accent.opacity(BADGE_TINT))
        .text_color(accent)
        .tooltip(tip)
        .dropdown_menu(move |mut menu, _, _| {
            let on_pick = |queue: Option<QueueId>| {
                let choose = choose.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| choose(queue, cx)
            };
            let (lead, value) = (lead.clone(), value.clone());
            menu = menu
                .scrollable(true)
                .max_h(px(220.))
                .item(
                    PopupMenuItem::element(move |_, cx| {
                        v_flex().child("Automatic").child(
                            h_flex()
                                .gap_1()
                                .text_role(Role::Meta, cx)
                                .child(lead.clone())
                                .when_some(value.clone(), |line, value| {
                                    line.child(div().text_color(cx.theme().primary).child(value))
                                }),
                        )
                    })
                    .checked(picked.is_none())
                    .on_click(on_pick(None)),
                )
                .separator();
            for (id, name) in &queues {
                menu = menu.item(
                    PopupMenuItem::new(SharedString::from(name.clone()))
                        .checked(picked == Some(*id))
                        .on_click(on_pick(Some(*id))),
                );
            }
            menu
        })
        .into_any_element()
}

/// A folder, the whole width; a click picks another.
pub(super) fn folder_box(
    id: &'static str,
    shown: &str,
    pick: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .w_full()
        .h(px(32.))
        .px_3()
        .gap_2()
        .border_1()
        .border_color(theme.input)
        .rounded(theme.radius)
        .cursor_pointer()
        .hover(|field| field.bg(theme.list_hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(text::folder(shown)),
        )
        .child(
            Icon::new(Lucide::Folder)
                .with_size(tokens::ICON)
                .text_color(theme.muted_foreground),
        )
        .on_click(pick)
        .into_any_element()
}

/// Why Automatic picks `queue`: the words, and the value to colour after them.
pub(super) fn automatic_reason(queue: &str, reason: &QueueReason) -> (String, Option<String>) {
    match reason {
        QueueReason::Matched(rule) => (
            format!("Goes to {queue}, for"),
            Some(text::rule_phrase(rule)),
        ),
        QueueReason::BrowserQueue => (format!("Goes to {queue}, for browser downloads"), None),
        QueueReason::Picked => (format!("Goes to {queue}, as you picked"), None),
        QueueReason::DefaultQueue => (format!("Goes to {queue}, the default queue"), None),
    }
}

/// A queue's name, tinted so it reads as the name of something.
fn queue_badge(name: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .min_w_0()
        .h(px(24.))
        .px_2()
        .gap_1()
        .rounded(theme.radius)
        .bg(theme.primary.opacity(BADGE_TINT))
        .text_role(Role::Meta, cx)
        .text_color(theme.primary)
        .child(Icon::new(Lucide::ListOrdered).with_size(tokens::ICON))
        .child(div().min_w_0().truncate().child(name))
}

/// How a download ended or that it waits, beside its name.
fn state_mark(row: &DownloadSummary, stalled: bool, cx: &App) -> Option<AnyElement> {
    let theme = cx.theme();
    let (icon, label, color) = match row.status {
        DownloadStatus::Completed => (Icon::new(IconName::CircleCheck), "Finished", theme.primary),
        DownloadStatus::Failed => (Icon::new(IconName::CircleAlert), "Failed", theme.danger),
        DownloadStatus::Cancelled => (
            Icon::new(IconName::CircleX),
            "Cancelled",
            theme.muted_foreground,
        ),
        DownloadStatus::Downloading if stalled => {
            (Icon::new(Lucide::Clock), "Waiting", theme.warning)
        }
        _ => return None,
    };
    Some(
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .text_role(Role::Meta, cx)
            .text_color(color)
            .child(icon.with_size(tokens::ICON))
            .child(label)
            .into_any_element(),
    )
}

/// What a download's window offers in its state, the main one last. Hide leaves a download
/// running; Close is for one with nothing left to run.
fn progress_buttons(
    id: DownloadId,
    status: DownloadStatus,
    cx: &mut Context<'_, DownloadWindow>,
) -> Vec<Button> {
    let command =
        |button_id,
         label,
         variant,
         command: fn(&wirecatch_core::Manager, DownloadId) -> Result<(), ProtocolError>| {
            button(button_id, label, variant, false).on_click(move |_, window, cx| {
                act(window.window_handle(), cx, move |m| command(m, id));
            })
        };
    let leave = |label, variant| {
        button("progress-leave", label, variant, false)
            .on_click(cx.listener(|view, _, _, cx| view.close(cx)))
    };
    match status {
        DownloadStatus::Completed => vec![
            leave("Close", ButtonVariant::Ghost),
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
            leave("Hide", ButtonVariant::Secondary),
            command(
                "progress-resume",
                "Resume",
                ButtonVariant::Primary,
                api::resume,
            ),
        ],
        DownloadStatus::Failed | DownloadStatus::Cancelled => vec![
            leave("Close", ButtonVariant::Ghost),
            command(
                "progress-retry",
                "Try again",
                ButtonVariant::Primary,
                api::retry,
            ),
        ],
        _ => vec![
            command(
                "progress-cancel",
                "Cancel",
                ButtonVariant::Ghost,
                api::cancel,
            ),
            leave("Hide", ButtonVariant::Secondary),
            command(
                "progress-pause",
                "Pause",
                ButtonVariant::Primary,
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
pub(super) fn heading(name: &str, link: &str, size: Option<u64>, cx: &App) -> impl IntoElement {
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

#[cfg(test)]
mod tests {
    use wirecatch_protocol::rules::Rule;

    use super::*;

    #[test]
    fn automatic_says_why_it_picked_the_queue() {
        assert_eq!(
            automatic_reason(
                "Auto Git",
                &QueueReason::Matched(Rule::Site("github.com".to_owned()))
            ),
            (
                "Goes to Auto Git, for".to_owned(),
                Some("github.com".to_owned())
            )
        );
        assert_eq!(
            automatic_reason("Main", &QueueReason::DefaultQueue).0,
            "Goes to Main, the default queue"
        );
        assert_eq!(
            automatic_reason("Main", &QueueReason::BrowserQueue).0,
            "Goes to Main, for browser downloads"
        );
        assert_eq!(
            automatic_reason("Main", &QueueReason::Picked).0,
            "Goes to Main, as you picked"
        );
    }
}
