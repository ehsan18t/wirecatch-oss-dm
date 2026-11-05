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
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, canvas, div, px, relative,
    size,
};
use wirecatch_protocol::app::{
    AddonId, AddonRequestItem, AddonState, AppSettingsPatch, CaptureConfirmation, DownloadStatus,
    DownloadSummary, FileExistsAnswer, FileExistsChoice, FileExistsQuestion, FileTypeChoice,
    FileTypeQuestion, FixAction, MediaChoice, MediaDefaultsPatch, PER_MILLE_DONE, PendingCapture,
    Question,
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
use crate::backend::{Backend, act, said};
use crate::library::Library;
use crate::main_window::fixes::run_from_download_window;
use crate::text::{ASK_EACH_VIDEO, REMEMBER_CHOICES};
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
/// How strongly a chip or panel is tinted with its colour.
const TINT: f32 = 0.14;
/// The queue picker's widest, leaving the row's buttons their full labels.
const PICKER_WIDTH: f32 = 136.;

/// One download window.
pub struct DownloadWindow {
    library: Entity<Library>,
    window: AnyWindowHandle,
    label: String,
    shows: Shows,
    /// The question, once read; `None` while reading and once it closed.
    question: Option<Question>,
    /// The download already in the list for the question's link.
    earlier: Option<DownloadId>,
    /// The question is no longer open in the core.
    over: bool,
    /// The file name the person may change.
    name: Entity<InputState>,
    /// The name the core last gave the field.
    core_name: Option<String>,
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
    /// Install was pressed for the add-on this page needs, and it has not been seen to start.
    addon_asked: Option<AddonId>,
    /// The add-on this window's page has waited for, kept once the question ends.
    addon_awaited: Option<AddonId>,
    /// The page this window last saw being read, kept once the question ends.
    reading: Option<String>,
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
            window: window.window_handle(),
            label,
            shows,
            question: None,
            earlier: None,
            over: false,
            addon_asked: None,
            addon_awaited: None,
            reading: None,
            name: cx.new(|cx| InputState::new(window, cx)),
            core_name: None,
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
        let read = Backend::run(cx, move |manager| async move {
            (
                api::question(&manager, id),
                api::earlier_download(&manager, id),
            )
        });
        cx.spawn(async move |view, cx| {
            let Ok(Ok((question, earlier))) = read.await else {
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
                if let Some(Question::Addon(item)) = &question {
                    view.addon_awaited = Some(item.addon.id);
                }
                match &question {
                    Some(Question::Reading(reading)) => view.reading = Some(reading.url.clone()),
                    Some(Question::Addon(item)) => view.reading = Some(item.url.clone()),
                    Some(_) => view.reading = None,
                    None => {}
                }
                if question.is_some() {
                    view.earlier = earlier.map(|row| row.id);
                }
                view.question = question;
                cx.notify();
            });
        })
        .detach();
    }

    /// The core started reading `url`. A read can end before this window first asks about it.
    pub fn reading_from(&mut self, url: &str, cx: &App) {
        self.reading = Some(url.to_owned());
        self.earlier = newest_attempt(&self.library.read(cx).downloads, url);
    }

    fn name_from(&mut self, name: &str, cx: &mut Context<'_, Self>) {
        let field = self.name.read(cx).value();
        if core_name_applies(&field, self.core_name.as_deref(), name) {
            self.core_name = Some(name.to_owned());
            put_name(self.name.clone(), name.to_owned(), self.window, cx);
        }
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
            let Some(answer) = said(done.await) else {
                return;
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
        self.library.read(cx).downloads.find(id).cloned()
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
            (Shows::Question(_), _) if self.over => self.over_note(cx),
            (Shows::Question(_), None) => div().into_any_element(),
            (Shows::Question(_), Some(question)) => match question.clone() {
                Question::Capture(capture) => self.capture(&capture, cx),
                Question::Reading(reading) => reading_view(reading.id, &reading.url, cx),
                Question::Choice(choice) => self.media_form(&choice, window, cx),
                Question::Addon(item) => self.addon_view(&item, cx),
                Question::FileExists(question) => self.file_exists(&question, cx),
                Question::FileType(question) => self.file_type(&question, cx),
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

/// A setting's switch along the window's foot.
struct FootSwitch {
    label: &'static str,
    checked: bool,
    patch: fn(bool) -> AppSettingsPatch,
}

impl FootSwitch {
    fn row(self, cx: &App) -> AnyElement {
        let Self {
            label,
            checked,
            patch,
        } = self;
        h_flex()
            .gap_2()
            .child(
                Switch::new(label)
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
            .into_any_element()
    }
}

fn media_patch(patch: MediaDefaultsPatch) -> AppSettingsPatch {
    AppSettingsPatch {
        media: Some(patch),
        ..AppSettingsPatch::default()
    }
}

impl DownloadWindow {
    /// The switches along the window's foot, for what it shows: the same settings the extension's
    /// popup and Settings have, which apply at once, Cancel or not (D200).
    fn strip(&self, cx: &App) -> Option<AnyElement> {
        // Read only once there is something to show: a question's window first draws while the
        // library is still updating, and reading it then panics.
        let settings = || self.library.read(cx).settings.as_ref();
        let switches = match (self.shows, &self.question) {
            (Shows::Question(_), Some(Question::Capture(_))) => vec![FootSwitch {
                label: "Ask before each download",
                checked: settings()?.extension.ask_before_download,
                patch: |on| AppSettingsPatch {
                    extension: Some(ExtensionSettingsPatch {
                        ask_before_download: Some(on),
                        ..ExtensionSettingsPatch::default()
                    }),
                    ..AppSettingsPatch::default()
                },
            }],
            (Shows::Question(_), Some(Question::Choice(_))) => {
                let media = &settings()?.media;
                vec![
                    FootSwitch {
                        label: REMEMBER_CHOICES,
                        checked: media.remember_choices,
                        patch: |on| {
                            media_patch(MediaDefaultsPatch {
                                remember_choices: Some(on),
                                ..MediaDefaultsPatch::default()
                            })
                        },
                    },
                    FootSwitch {
                        label: ASK_EACH_VIDEO,
                        checked: media.ask_each_time,
                        patch: |on| {
                            media_patch(MediaDefaultsPatch {
                                ask_each_time: Some(on),
                                ..MediaDefaultsPatch::default()
                            })
                        },
                    },
                ]
            }
            (Shows::Download(_), _) => vec![FootSwitch {
                label: "Show progress while downloading",
                checked: settings()?.show_progress_windows,
                patch: |on| AppSettingsPatch {
                    show_progress_windows: Some(on),
                    ..AppSettingsPatch::default()
                },
            }],
            _ => return None,
        };
        let rows: Vec<AnyElement> = switches.into_iter().map(|switch| switch.row(cx)).collect();
        let theme = cx.theme();
        Some(
            v_flex()
                .px_4()
                .py_2()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.sidebar)
                .children(rows)
                .into_any_element(),
        )
    }

    /// A browser file waiting for Download now, Download later or Cancel, with its name to change.
    fn capture(&mut self, capture: &PendingCapture, cx: &mut Context<'_, Self>) -> AnyElement {
        let id = capture.id;
        let busy = self.busy || matches!(self.shows, Shows::Download(_));
        let earlier = self.earlier.and_then(|earlier| self.download(earlier, cx));
        v_flex()
            .min_h(px(FILE_CONTENT_HEIGHT))
            .gap_4()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(heading(&capture.file_name, &capture.url, capture.size, cx))
                    .children(earlier.map(|row| earlier_chip(&row, cx))),
            )
            .child(Field::new("Save as", Input::new(&self.name)))
            .child(Field::new("Folder", self.folder_field(capture, cx)))
            .child(
                h_flex()
                    .w_full()
                    .mt_auto()
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
        let name = self.name.read(cx).value();
        let confirmation = CaptureConfirmation {
            folder: self.folder.clone(),
            file_name: chosen_name(&name, self.core_name.as_deref()),
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
        let earlier = self.earlier;
        form.update(cx, |form, _| {
            form.busy = busy;
            form.earlier = earlier;
        });
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
            .child(div().child(text::file_exists_said(question)))
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

    /// A server's name of another file type than the confirmed one (D166).
    fn file_type(&mut self, question: &FileTypeQuestion, cx: &mut Context<'_, Self>) -> AnyElement {
        let id = question.id;
        let busy = self.busy;
        let choose = |button_id, label, variant, choice| {
            button(button_id, label, variant, busy).on_click(cx.listener(move |view, _, _, cx| {
                view.answer(
                    move |m| async move { api::answer_file_type(&m, id, choice).map(Some) },
                    cx,
                );
            }))
        };
        let buttons = vec![
            choose(
                "type-cancel",
                "Cancel",
                ButtonVariant::Ghost,
                FileTypeChoice::Cancel,
            ),
            choose(
                "type-server",
                "Use server's name",
                ButtonVariant::Secondary,
                FileTypeChoice::UseServers,
            ),
            choose(
                "type-keep",
                "Keep my name",
                ButtonVariant::Primary,
                FileTypeChoice::KeepConfirmed,
            ),
        ];
        v_flex()
            .gap_4()
            .child(heading(&question.confirmed_name, &question.url, None, cx))
            .child(div().child(text::file_type_said(question)))
            .child(actions(buttons))
            .into_any_element()
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
        let buttons = progress_buttons(&row, cx);
        let theme = cx.theme();
        v_flex()
            .min_h(px(FILE_CONTENT_HEIGHT))
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
            .child(self.run_panel(&row, per_mille, cx))
            .children(self.facts(&row, cx))
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
    /// with the speed and percent over it. It is in every state and takes the spare height, so the
    /// rows keep even gaps: a failure's reason fills it, and a download too short to draw says it
    /// finished.
    fn run_panel(&self, row: &DownloadSummary, per_mille: Option<u16>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let panel = v_flex()
            .w_full()
            .flex_1()
            .min_h(px(GRAPH_HEIGHT))
            .rounded(theme.radius)
            .overflow_hidden();
        if let (DownloadStatus::Failed, Some(error)) = (row.status, &row.error) {
            return panel
                .px_2p5()
                .py_1p5()
                .bg(theme.danger.opacity(TINT))
                .text_color(theme.danger)
                .child(error.message.clone())
                .into_any_element();
        }
        if row.status == DownloadStatus::Completed && !self.record.drawable() {
            return panel
                .px_2p5()
                .pt_1p5()
                .bg(theme.primary.opacity(TINT))
                .child(div().text_role(Role::Strong, cx).child("Download finished"))
                .into_any_element();
        }
        let cancelled = row.status == DownloadStatus::Cancelled;
        let color = match row.status {
            DownloadStatus::Downloading if self.record.stalled(Instant::now()) => {
                theme.muted_foreground
            }
            DownloadStatus::Completed | DownloadStatus::Downloading => theme.primary,
            DownloadStatus::Failed => theme.danger,
            _ => theme.muted_foreground,
        };
        let label = self.panel_label(row);
        let moving = !matches!(
            row.status,
            DownloadStatus::Completed | DownloadStatus::Processing | DownloadStatus::Cancelled
        );
        let front = per_mille
            .filter(|_| moving && row.total.is_some_and(|total| total > 0))
            .map(|done| f32::from(done) / f32::from(PER_MILLE_DONE));
        // Cancelling zeroes the row's progress: the curve stays where the run got to.
        let end = if cancelled {
            self.record.reached()
        } else {
            front
        };
        let graph = self.record.drawable().then(|| {
            run_graph(self.record.points(end.unwrap_or(1.)), front, color)
                .absolute()
                .size_full()
        });
        panel
            .bg(theme.secondary)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px_2p5()
                    .pt_1p5()
                    .justify_between()
                    .child(div().text_role(Role::Strong, cx).children(label))
                    .child(
                        div()
                            .text_role(Role::Meta, cx)
                            .children(text::percent(row).filter(|_| moving)),
                    ),
            )
            .when(cancelled, |panel| {
                panel.child(
                    div()
                        .flex_shrink_0()
                        .px_2p5()
                        .text_role(Role::Meta, cx)
                        .child("Anything downloaded so far was removed. Try again starts over."),
                )
            })
            .child(div().relative().mt_1p5().flex_1().min_h_0().children(graph))
            .into_any_element()
    }

    fn panel_label(&self, row: &DownloadSummary) -> Option<String> {
        match row.status {
            DownloadStatus::Downloading if self.record.stalled(Instant::now()) => {
                Some("Waiting for the server".to_owned())
            }
            DownloadStatus::Downloading if row.speed > 0 => Some(text::speed(row.speed)),
            DownloadStatus::Completed => self
                .record
                .peak()
                .map(|peak| format!("Peak {}", text::speed(peak))),
            DownloadStatus::Failed => None,
            DownloadStatus::Cancelled => Some("Download cancelled".to_owned()),
            _ => Some(text::status_short(row)),
        }
    }

    /// The figures under the panel, by state: a failure keeps what it downloaded, and a cancelled
    /// download has nothing left to count.
    fn facts(&self, row: &DownloadSummary, cx: &App) -> Option<AnyElement> {
        if row.status == DownloadStatus::Cancelled {
            return None;
        }
        if row.status == DownloadStatus::Failed && row.error.is_some() {
            return (row.done > 0).then(|| {
                let kept = match row.total {
                    Some(total) => format!(
                        "Downloaded {} of {}",
                        format_bytes(row.done),
                        format_bytes(total)
                    ),
                    None => format!("Downloaded {}", format_bytes(row.done)),
                };
                div()
                    .text_role(Role::Meta, cx)
                    .child(kept)
                    .into_any_element()
            });
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
        Some(
            h_flex()
                .w_full()
                .children(facts.into_iter().map(|(label, value)| {
                    v_flex()
                        .w(relative(0.25))
                        .min_w_0()
                        .child(div().text_role(Role::Meta, cx).child(label))
                        .child(div().truncate().child(value))
                }))
                .into_any_element(),
        )
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
        .bg(accent.opacity(TINT))
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
    tinted_chip(Lucide::ListOrdered, name, cx.theme().primary, cx)
}

/// A warning that the question's link is already in the list, with the way to the file once it
/// is finished: downloading again stays allowed.
pub(super) fn earlier_chip(row: &DownloadSummary, cx: &App) -> AnyElement {
    let id = row.id;
    h_flex()
        .gap_2()
        .child(tinted_chip(
            Lucide::TriangleAlert,
            text::earlier_notice(row),
            cx.theme().warning,
            cx,
        ))
        .when(row.status == DownloadStatus::Completed, |line| {
            line.child(
                Button::new("earlier-show-in-folder")
                    .xsmall()
                    .link()
                    .label("Show in folder")
                    .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
            )
        })
        .into_any_element()
}

fn tinted_chip(icon: Lucide, label: String, tint: Hsla, cx: &App) -> impl IntoElement {
    h_flex()
        .min_w_0()
        .h(px(24.))
        .px_2()
        .gap_1()
        .rounded(cx.theme().radius)
        .bg(tint.opacity(TINT))
        .text_role(Role::Meta, cx)
        .text_color(tint)
        .child(Icon::new(icon).with_size(tokens::ICON))
        .child(div().min_w_0().truncate().child(label))
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
fn progress_buttons(row: &DownloadSummary, cx: &mut Context<'_, DownloadWindow>) -> Vec<Button> {
    let id = row.id;
    let cancel = || {
        button("progress-cancel", "Cancel", ButtonVariant::Ghost, false)
            .on_click(move |_, window, cx| super::ask_to_cancel(id, window, cx))
    };
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
    match row.status {
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
            cancel(),
            leave("Hide", ButtonVariant::Secondary),
            command(
                "progress-resume",
                "Resume",
                ButtonVariant::Primary,
                api::resume,
            ),
        ],
        DownloadStatus::Failed | DownloadStatus::Cancelled => {
            std::iter::once(leave("Close", ButtonVariant::Ghost))
                .chain(fix_buttons(row))
                .collect()
        }
        _ => vec![
            cancel(),
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

/// The window's fixes for a download that ended without finishing, the main one last.
fn fix_buttons(row: &DownloadSummary) -> Vec<Button> {
    let id = row.id;
    let fixes = super::fixes_offered(row);
    let main = fixes.last().copied();
    fixes
        .into_iter()
        .map(|fix| {
            let button_id = if fix == FixAction::Retry {
                "progress-retry"
            } else {
                "progress-fix"
            };
            let variant = if Some(fix) == main {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            };
            button(button_id, text::fix_label(fix), variant, false)
                .on_click(move |_, window, cx| run_from_download_window(id, fix, window, cx))
        })
        .collect()
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

/// Where a page waiting for an add-on stands.
#[derive(Debug, PartialEq, Eq)]
enum AddonStep {
    Offer,
    /// With how far along it is in percent, once its size is known.
    Installing(Option<u64>),
    Failed(String),
}

/// `asked`: Install was pressed and the install has not been seen to start yet, so a click cannot
/// start a second one, and an earlier failure does not stay on show.
fn addon_step(state: Option<&AddonState>, asked: bool) -> AddonStep {
    match state {
        Some(AddonState::Installing { done, total }) => AddonStep::Installing(
            total
                .filter(|total| *total > 0)
                .map(|total| (*done).min(total) * 100 / total),
        ),
        // Installed: the core is about to carry on with the download and close this question.
        Some(AddonState::Installed | AddonState::UpdateAvailable) => AddonStep::Installing(None),
        _ if asked => AddonStep::Installing(None),
        Some(AddonState::Failed { message }) => AddonStep::Failed(message.clone()),
        _ => AddonStep::Offer,
    }
}

impl DownloadWindow {
    /// A video page waiting for the media add-on.
    fn addon_view(&mut self, item: &AddonRequestItem, cx: &mut Context<'_, Self>) -> AnyElement {
        let (id, addon) = (item.id, item.addon.id);
        let state = self
            .library
            .read(cx)
            .addons
            .iter()
            .find(|known| known.id == addon)
            .map(|known| known.state.clone());
        if matches!(state, Some(AddonState::Installing { .. })) {
            self.addon_asked = None;
        }
        let step = addon_step(state.as_ref(), self.addon_asked.is_some());
        let name = item.addon.name.to_lowercase();
        let installing = matches!(step, AddonStep::Installing(_));
        let main = match step {
            AddonStep::Installing(_) => "Installing",
            AddonStep::Failed(_) => "Try again",
            AddonStep::Offer => "Install and download",
        };
        let said = match step {
            AddonStep::Offer => {
                div().child("Install it to download videos from sites like this one.")
            }
            AddonStep::Installing(done) => v_flex()
                .gap_2()
                .child(done.map_or_else(
                    || format!("Installing the {name}. The download starts when it is ready."),
                    |done| {
                        format!(
                            "Installing the {name}, {done}%. The download starts when it is ready."
                        )
                    },
                ))
                .child(
                    Progress::new("addon-progress")
                        .loading(done.is_none())
                        .value(
                            done.map_or(0., |done| f32::from(u8::try_from(done).unwrap_or(100))),
                        ),
                ),
            AddonStep::Failed(message) => div().text_color(cx.theme().danger).child(message),
        };
        v_flex()
            .gap_4()
            .child(heading(
                &format!("This video needs the {name}"),
                &item.url,
                None,
                cx,
            ))
            .child(said)
            .child(actions(vec![
                button("addon-cancel", "Cancel", ButtonVariant::Ghost, false).on_click(
                    cx.listener(move |view, _, _, cx| {
                        view.answer(
                            move |m| async move {
                                api::dismiss_media_choice(&m, id).map(|()| None)
                            },
                            cx,
                        );
                    }),
                ),
                button("addon-install", main, ButtonVariant::Primary, installing)
                    .loading(installing)
                    .on_click(cx.listener(move |view, _, _, cx| view.install_addon(addon, cx))),
            ]))
            .into_any_element()
    }

    /// Why the add-on this window's page waited for failed; the core drops the page when it does.
    fn addon_failure(&self, cx: &App) -> Option<String> {
        let awaited = self.addon_awaited?;
        let library = self.library.read(cx);
        let addon = library.addons.iter().find(|known| known.id == awaited)?;
        match &addon.state {
            AddonState::Failed { message } => Some(message.clone()),
            _ => None,
        }
    }

    fn over_note(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        if let Some(message) = self.addon_failure(cx) {
            failed_note("The media add-on could not be installed", &message, cx)
        } else if let Some(message) = self.reading_failure(cx) {
            failed_note("This video could not be downloaded", &message, cx)
        } else {
            closed_note(cx)
        }
    }

    /// Why the page this window was reading failed; its question ends when it does.
    fn reading_failure(&self, cx: &App) -> Option<String> {
        let link = self.reading.as_deref()?;
        let library = self.library.read(cx);
        let row = failed_video(&library.downloads, link, self.earlier)?;
        row.error.as_ref().map(|error| error.message.clone())
    }

    fn install_addon(&mut self, addon: AddonId, cx: &mut Context<'_, Self>) {
        self.addon_asked = Some(addon);
        self.problem = None;
        cx.notify();
        let started = Backend::run(
            cx,
            move |m| async move { api::install_addon(&m, addon).await },
        );
        cx.spawn(async move |view, cx| {
            let refused = match said(started.await) {
                Some(Ok(())) => return,
                Some(Err(message)) => message,
                None => "The install stopped unexpectedly.".to_owned(),
            };
            _ = view.update(cx, |view: &mut Self, cx| {
                view.addon_asked = None;
                view.problem = Some(refused);
                cx.notify();
            });
        })
        .detach();
    }
}

/// What ended the page's question; the list's row has Try again.
fn failed_note(
    title: &'static str,
    message: &str,
    cx: &mut Context<'_, DownloadWindow>,
) -> AnyElement {
    v_flex()
        .gap_4()
        .child(div().text_role(Role::Title, cx).child(title))
        .child(
            div()
                .text_color(cx.theme().danger)
                .child(message.to_owned()),
        )
        .child(actions(vec![
            button("failed-close", "Close", ButtonVariant::Secondary, false)
                .on_click(cx.listener(|view, _, _, cx| view.close(cx))),
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

/// The name the person chose in the field: a name left as proposed is no choice, so the server's
/// answer may still improve it.
fn chosen_name(field: &str, proposed: Option<&str>) -> Option<String> {
    let name = field.trim();
    (!name.is_empty() && Some(name) != proposed).then(|| name.to_owned())
}

/// Whether the core's name replaces what the field holds: never a name the person typed.
fn core_name_applies(field: &str, last: Option<&str>, core: &str) -> bool {
    last.is_none_or(|last| last != core && field == last)
}

/// `window` is the field's own: a question's window can open behind another, so the active one may be neither.
fn put_name(state: Entity<InputState>, name: String, window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        _ = window.update(cx, |_, window, cx| {
            state.update(cx, |state, cx| state.set_value(name, window, cx));
        });
    });
}

fn newest_attempt(downloads: &[DownloadSummary], link: &str) -> Option<DownloadId> {
    downloads
        .iter()
        .filter(|row| row.url == link)
        .map(|row| row.id)
        .max()
}

/// The newest failed attempt at `link` made after `earlier`, the newest from before this question.
fn failed_video<'a>(
    downloads: &'a [DownloadSummary],
    link: &str,
    earlier: Option<DownloadId>,
) -> Option<&'a DownloadSummary> {
    downloads
        .iter()
        .filter(|row| {
            row.url == link && row.status == DownloadStatus::Failed && Some(row.id) > earlier
        })
        .max_by_key(|row| row.id)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Render, TestAppContext};
    use wirecatch_protocol::app::{DownloadStatus, DownloadSummary};
    use wirecatch_protocol::rules::Rule;

    fn row(id: i64, url: &str, status: DownloadStatus) -> DownloadSummary {
        let mut row = wirecatch_test_support::downloads::download(id);
        row.url = url.to_owned();
        row.status = status;
        row
    }

    #[test]
    fn the_newest_attempt_at_a_link_is_found() {
        use super::newest_attempt;
        use wirecatch_protocol::DownloadId;

        let page = "https://example.com/watch/1";
        let downloads = [
            row(1, page, DownloadStatus::Failed),
            row(2, page, DownloadStatus::Completed),
            row(3, "https://example.com/other", DownloadStatus::Failed),
        ];
        assert_eq!(newest_attempt(&downloads, page), Some(DownloadId(2)));
        assert_eq!(newest_attempt(&downloads, "https://example.com/new"), None);
    }

    #[test]
    fn a_window_finds_the_video_it_was_reading_failed() {
        use super::failed_video;
        use wirecatch_protocol::DownloadId;

        let page = "https://example.com/watch/1";
        let downloads = [
            row(1, page, DownloadStatus::Failed),
            row(2, "https://example.com/other", DownloadStatus::Failed),
            row(3, page, DownloadStatus::Failed),
        ];
        let found = failed_video(&downloads, page, Some(DownloadId(1)));
        assert_eq!(found.map(|row| row.id), Some(DownloadId(3)));
        // Only the attempt from before this question failed: this one was answered elsewhere.
        assert!(failed_video(&downloads[..1], page, Some(DownloadId(1))).is_none());
        // `earlier` names only the newest attempt; the ones before it are older still.
        assert!(failed_video(&downloads, page, Some(DownloadId(3))).is_none());
        let going = [row(4, page, DownloadStatus::Downloading)];
        assert!(failed_video(&going, page, None).is_none());
    }

    struct NameField(gpui_kit::Entity<super::InputState>);

    impl Render for NameField {
        fn render(
            &mut self,
            _: &mut gpui_kit::Window,
            _: &mut gpui_kit::Context<'_, Self>,
        ) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }

    #[test]
    fn only_a_name_the_person_changed_counts_as_chosen() {
        use super::chosen_name;

        assert_eq!(chosen_name("get", Some("get")), None);
        assert_eq!(chosen_name(" get ", Some("get")), None);
        assert_eq!(chosen_name("", Some("get")), None);
        assert_eq!(
            chosen_name("report.pdf", Some("get")).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(chosen_name("get", None).as_deref(), Some("get"));
    }

    #[test]
    fn a_name_the_person_typed_outlasts_the_question_being_read_again() {
        use super::core_name_applies;

        assert!(core_name_applies("", None, "report.pdf"));
        assert!(!core_name_applies(
            "my name.pdf",
            Some("report.pdf"),
            "report.pdf"
        ));
        assert!(!core_name_applies(
            "report.pdf",
            Some("report.pdf"),
            "report.pdf"
        ));
        // The core found a better name, and the person has not changed the field.
        assert!(core_name_applies(
            "report.pdf",
            Some("report.pdf"),
            "final.pdf"
        ));
        assert!(!core_name_applies(
            "my name.pdf",
            Some("report.pdf"),
            "final.pdf"
        ));
    }

    #[gpui_kit::test]
    fn a_window_behind_the_others_still_gets_its_name(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        let (field, cx) = cx.add_window_view(|window, cx| {
            NameField(cx.new(|cx| super::InputState::new(window, cx)))
        });
        cx.deactivate_window();

        let state = field.read_with(cx, |field, _| field.0.clone());
        let put = state.clone();
        cx.update(|window, cx| {
            super::put_name(put, "report.pdf".to_owned(), window.window_handle(), cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.value().as_ref(), "report.pdf");
        });
    }

    #[test]
    fn an_addon_question_follows_the_install() {
        use super::{AddonState, AddonStep, addon_step};

        assert_eq!(
            addon_step(Some(&AddonState::NotInstalled), false),
            AddonStep::Offer
        );
        // Pressed: no second click while the install has not been seen to start.
        assert_eq!(
            addon_step(Some(&AddonState::NotInstalled), true),
            AddonStep::Installing(None)
        );
        assert_eq!(
            addon_step(
                Some(&AddonState::Installing {
                    done: 42,
                    total: Some(100),
                }),
                false,
            ),
            AddonStep::Installing(Some(42))
        );
        let failed = AddonState::Failed {
            message: "No connection.".to_owned(),
        };
        assert_eq!(
            addon_step(Some(&failed), false),
            AddonStep::Failed("No connection.".to_owned())
        );
        // Try again pressed: the old failure does not stay on show.
        assert_eq!(addon_step(Some(&failed), true), AddonStep::Installing(None));
    }

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
