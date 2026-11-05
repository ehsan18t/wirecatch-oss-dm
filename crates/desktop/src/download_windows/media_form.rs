//! The quality window's form: quality, format, subtitles, chapters and where the file goes.

use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px,
};
use wirecatch_protocol::app::{
    AddonId, AddonInfo, AddonState, Category, MediaChoice, MediaOption, MediaSelection,
    OutputDefaults, OutputFormat, StreamAction, SubtitleChoice,
};
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::{api, host};

use super::media_output::{
    OutputChoice, conversion_note, fit_for, format_label, holds_subtitles, kbps_for, quality_notes,
    shown_subtitles, size_for, subtitle_reason, subtitle_toggle, video_length,
};
use super::view::{earlier_chip, folder_box, queue_badge_picker};
use crate::backend::Backend;
use crate::library::Library;
use crate::ui::{Role, Typed as _};
use crate::{files, text};

const LABEL_WIDTH: f32 = 64.;
/// How far the notes under a control sit in: the label and the gap after it.
const NOTE_INSET: f32 = 72.;
const BEST_TINT: f32 = 0.14;

/// What the person decided.
pub enum MediaAnswer {
    Cancel,
    Choose(Box<MediaSelection>),
}

/// One entry of a plain select.
#[derive(Clone)]
struct Choice<T> {
    title: SharedString,
    value: T,
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

/// A quality, with its size and whether it is the best one on the right.
#[derive(Clone)]
struct Quality {
    id: String,
    title: SharedString,
    best: bool,
    size: Option<String>,
    accent: Hsla,
    muted: Hsla,
}

impl Quality {
    fn row(&self) -> AnyElement {
        h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.title.clone()),
            )
            .when(self.best, |row| {
                row.child(
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .text_xs()
                        .bg(self.accent.opacity(BEST_TINT))
                        .text_color(self.accent)
                        .child("Best"),
                )
            })
            .children(
                self.size
                    .clone()
                    .map(|size| div().text_xs().text_color(self.muted).child(size)),
            )
            .into_any_element()
    }
}

impl SearchableListItem for Quality {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.row())
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.row()
    }

    fn value(&self) -> &String {
        &self.id
    }
}

type Picker<T> = Entity<SelectState<SearchableVec<Choice<T>>>>;
type Subtitles = Entity<ComboboxState<SearchableVec<Choice<String>>>>;

pub struct MediaForm {
    library: Entity<Library>,
    choice: MediaChoice,
    option_id: String,
    output: OutputChoice,
    file_name: Entity<InputState>,
    folder: Option<String>,
    /// Where audio only goes when its type decides: the question names only the video's folder.
    audio_folder: Option<String>,
    /// The queue picked; `None` is Automatic.
    queue: Option<QueueId>,
    /// The selects list what the quality and format offer: they are rebuilt on the next draw.
    stale: bool,
    /// An answer is on its way.
    pub busy: bool,
    /// The download already in the list for this video.
    pub earlier: Option<DownloadId>,
    /// A quality that needs the media add-on: it is sent once the add-on is in.
    awaiting_addon: Option<MediaSelection>,
    /// The add-on's state when the install was asked for, until the install is seen to start: a change to the list before then says nothing about it.
    addon_before: Option<AddonState>,
    /// Why the last install asked for here did not work.
    install_problem: Option<String>,
    quality: Entity<SelectState<SearchableVec<Quality>>>,
    format: Picker<OutputFormat>,
    kbps: Picker<Option<u32>>,
    subtitles: Subtitles,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MediaAnswer> for MediaForm {}

impl MediaForm {
    pub fn new(
        choice: MediaChoice,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let mut output = OutputChoice::proposed(&choice.start, &choice.subtitles);
        let option_id = choice.start.option_id.clone();
        let fit = choice
            .options
            .iter()
            .find(|option| option.id == option_id)
            .and_then(|option| fit_for(option, output.format));
        output.kbps = kbps_for(fit, output.format, output.kbps);

        let quality = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let format =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let kbps = cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let subtitles = subtitle_picker(&choice, &output, window, cx);
        let audio_folder = Backend::now(cx).0.and_then(|manager| {
            let folder = manager.type_folder(Category::Audio).ok()?;
            Some(folder.to_string_lossy().into_owned())
        });

        let title = choice.title.clone();
        let file_name = cx.new(|cx| InputState::new(window, cx).placeholder(title));
        let mut subscriptions = follow_pickers(&quality, &format, &kbps, &subtitles, cx);
        subscriptions.push(cx.observe(&library, |form, _, cx| form.addon_moved(cx)));

        Self {
            library,
            choice,
            option_id,
            output,
            file_name,
            folder: None,
            audio_folder,
            queue: None,
            stale: true,
            busy: false,
            earlier: None,
            awaiting_addon: None,
            addon_before: None,
            install_problem: None,
            quality,
            format,
            kbps,
            subtitles,
            _subscriptions: subscriptions,
        }
    }

    pub fn question(&self) -> QuestionId {
        self.choice.id
    }

    fn picked(&self) -> Option<&MediaOption> {
        self.choice
            .options
            .iter()
            .find(|option| option.id == self.option_id)
    }

    /// Another quality: switching between video and audio only starts from that kind's choices.
    fn pick_quality(&mut self, id: String, cx: &mut Context<'_, Self>) {
        if id == self.option_id {
            return;
        }
        let was_audio = self.picked().map(|option| option.audio_only);
        self.option_id = id;
        let Some(now) = self.picked().cloned() else {
            return;
        };
        if was_audio == Some(now.audio_only) {
            if fit_for(&now, self.output.format).is_none() {
                self.output.format = OutputFormat::Original;
            }
            self.output.kbps = kbps_for(
                fit_for(&now, self.output.format),
                self.output.format,
                self.output.kbps,
            );
        } else {
            let library = self.library.read(cx);
            let media = library.settings.as_ref().map(|settings| &settings.media);
            let defaults = media.map(|media| {
                if now.audio_only {
                    &media.audio
                } else {
                    &media.video
                }
            });
            self.output = self.output.switched(&now, &self.choice, defaults);
        }
        self.stale = true;
        cx.notify();
    }

    fn pick_format(&mut self, format: OutputFormat, cx: &mut Context<'_, Self>) {
        if format == self.output.format {
            return;
        }
        let saved = self
            .library
            .read(cx)
            .settings
            .as_ref()
            .and_then(|settings| {
                let media = &settings.media;
                let audio_only = self.picked().is_some_and(|option| option.audio_only);
                (if audio_only {
                    &media.audio
                } else {
                    &media.video
                })
                .audio_kbps
            });
        let fit = self.picked().and_then(|option| fit_for(option, format));
        self.output.kbps = kbps_for(fit, format, self.output.kbps.or(saved));
        self.output.format = format;
        self.stale = true;
        cx.notify();
    }

    /// Lists what the picked quality and format offer, with what is picked selected.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.stale {
            return;
        }
        self.stale = false;
        let theme = cx.theme();
        let (accent, muted) = (theme.primary, theme.muted_foreground);
        let qualities: Vec<Quality> = self
            .choice
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let shown = if fit_for(option, self.output.format).is_some() {
                    self.output.format
                } else {
                    OutputFormat::Original
                };
                let kbps = self.output.kbps.filter(|_| option.id == self.option_id);
                Quality {
                    id: option.id.clone(),
                    title: if option.quality.is_empty() {
                        option.label.clone().into()
                    } else {
                        option.quality.clone().into()
                    },
                    best: index == 0,
                    size: size_for(option, shown, kbps, self.choice.duration_ms).map(format_bytes),
                    accent,
                    muted,
                }
            })
            .collect();
        let option_id = self.option_id.clone();
        self.quality.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(qualities), window, cx);
            state.set_selected_value(&option_id, window, cx);
        });

        let picked = self.picked().cloned();
        let formats: Vec<Choice<OutputFormat>> = picked
            .iter()
            .flat_map(|option| &option.outputs)
            .map(|fit| Choice {
                title: format_label(fit).into(),
                value: fit.format,
            })
            .collect();
        let format = self.output.format;
        self.format.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(formats), window, cx);
            state.set_selected_value(&format, window, cx);
        });

        let fit = picked.as_ref().and_then(|option| fit_for(option, format));
        let mut rates: Vec<Choice<Option<u32>>> = Vec::new();
        if let Some(fit) = fit.filter(|fit| {
            !fit.format.kbps_choices().is_empty() && fit.audio != StreamAction::Encode
        }) {
            let usual = fit
                .audio_kbps
                .map_or_else(String::new, |kbps| format!(" ({kbps} kbps)"));
            rates.push(Choice {
                title: format!("As downloaded{usual}").into(),
                value: None,
            });
        }
        rates.extend(format.kbps_choices().iter().map(|kbps| Choice {
            title: format!("{kbps} kbps").into(),
            value: Some(*kbps),
        }));
        let kbps = self.output.kbps;
        self.kbps.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(rates), window, cx);
            state.set_selected_value(&kbps, window, cx);
        });

        let languages = self.output.subtitles.clone();
        self.subtitles.update(cx, |state, cx| {
            if state.selected_values() != languages {
                state.set_selected_values(&languages, window, cx);
            }
        });
    }

    /// The saved choices for the kind of quality picked.
    fn saved_choices(&self, cx: &App) -> OutputDefaults {
        let audio_only = self.picked().is_some_and(|option| option.audio_only);
        self.library
            .read(cx)
            .settings
            .as_ref()
            .map(|settings| {
                if audio_only {
                    settings.media.audio.clone()
                } else {
                    settings.media.video.clone()
                }
            })
            .unwrap_or_default()
    }

    fn pick_subtitles(&mut self, pick: SubtitleChoice, cx: &mut Context<'_, Self>) {
        let saved = self.saved_choices(cx);
        self.output
            .choose_subtitles(pick, &self.choice.subtitles, &saved);
        self.stale = true;
        cx.notify();
    }

    /// Where the file goes unless the person picked a folder here: the one picked when the link
    /// was added, its queue's, or the one for its type.
    fn app_folder(&self, cx: &App) -> String {
        let Some(type_folder) = &self.choice.type_folder else {
            return self.choice.folder.clone();
        };
        let library = self.library.read(cx);
        let own = library
            .queue(self.queue.unwrap_or(self.choice.queue))
            .and_then(|queue| queue.settings.folder.own_ref())
            .map(|folder| folder.0.clone());
        if let Some(own) = own {
            return own;
        }
        self.audio_folder
            .clone()
            .filter(|_| self.picked().is_some_and(|option| option.audio_only))
            .unwrap_or_else(|| type_folder.clone())
    }

    fn subtitles_shown(&self) -> SubtitleChoice {
        let fit = self
            .picked()
            .and_then(|option| fit_for(option, self.output.format));
        self.output.subtitles_shown(fit)
    }

    fn needs_install(&self, cx: &App) -> bool {
        let needs_ffmpeg = self.picked().is_some_and(|option| option.needs_ffmpeg)
            || self.subtitles_shown() != SubtitleChoice::Off;
        needs_ffmpeg
            && !self
                .library
                .read(cx)
                .addons
                .iter()
                .any(|addon| addon.usable)
    }

    fn addon_note(&self, cx: &App) -> Option<String> {
        if self.awaiting_addon.is_some() {
            return Some(
                "Installing the media add-on. The download starts when it is ready.".to_owned(),
            );
        }
        let joins = self.picked()?.needs_ffmpeg;
        if !self.needs_install(cx) {
            return None;
        }
        if let Some(problem) = &self.install_problem {
            return Some(problem.clone());
        }
        let library = self.library.read(cx);
        let size = library
            .addons
            .first()
            .and_then(|addon| addon.size)
            .map_or_else(String::new, |size| format!(" ({})", format_bytes(size)));
        let app = host::CHANNEL.name();
        Some(if joins {
            format!(
                "Video and audio download separately: {app} first installs the media add-on{size} to merge them."
            )
        } else {
            format!("Subtitles need the media add-on: {app} first installs it{size}.")
        })
    }

    fn answer(&mut self, later: bool, cx: &mut Context<'_, Self>) {
        let Some(option) = self.picked() else {
            return;
        };
        let mut selection = self.choice.start.clone();
        selection.option_id.clone_from(&option.id);
        let name = self.file_name.read(cx).value().trim().to_owned();
        selection.file_name = (!name.is_empty()).then_some(name);
        selection.folder.clone_from(&self.folder);
        selection.queue = self.queue.filter(|queue| *queue != self.choice.queue);
        selection.later = later;
        self.output.apply(option, &self.choice, &mut selection);
        if self.needs_install(cx) {
            self.install_then(selection, cx);
        } else {
            cx.emit(MediaAnswer::Choose(Box::new(selection)));
        }
    }

    /// Installs the media add-on; `addon_moved` sends `selection` once it is in.
    fn install_then(&mut self, selection: MediaSelection, cx: &mut Context<'_, Self>) {
        self.awaiting_addon = Some(selection);
        self.install_problem = None;
        self.addon_before = self
            .library
            .read(cx)
            .addons
            .first()
            .map(|addon| addon.state.clone());
        cx.notify();
        let started = Backend::run(cx, move |manager| async move {
            api::install_addon(&manager, AddonId::Media).await
        });
        cx.spawn(async move |form, cx| {
            let refused = match started.await {
                Ok(Ok(Ok(()))) => return,
                Ok(Ok(Err(error))) => error.message,
                Ok(Err(unopened)) => unopened.reason.message,
                Err(e) => format!("The install stopped unexpectedly ({e})."),
            };
            _ = form.update(cx, |form: &mut Self, cx| {
                form.awaiting_addon = None;
                form.install_problem = Some(refused);
                cx.notify();
            });
        })
        .detach();
    }

    fn addon_moved(&mut self, cx: &mut Context<'_, Self>) {
        if self.awaiting_addon.is_none() {
            return;
        }
        let wait = install_wait(
            self.library.read(cx).addons.first(),
            self.addon_before.as_ref(),
        );
        match wait {
            InstallWait::Ready => {
                if let Some(selection) = self.awaiting_addon.take() {
                    cx.emit(MediaAnswer::Choose(Box::new(selection)));
                }
            }
            InstallWait::Started => self.addon_before = None,
            InstallWait::NotYet => {}
            InstallWait::Ended(problem) => {
                self.awaiting_addon = None;
                self.install_problem = problem;
            }
        }
        cx.notify();
    }

    fn heading(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let meta = video_meta(
            self.choice.page_url.as_deref(),
            self.choice.duration_ms.map(video_length),
        );
        let title: SharedString = self.choice.title.clone().into();
        let tip = title.clone();
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .id("media-title")
                    .truncate()
                    .text_role(Role::Title, cx)
                    .child(title)
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
            )
            .children(meta.map(|meta| div().text_role(Role::Meta, cx).child(meta)))
            .into_any_element()
    }

    fn earlier_note(&self, cx: &App) -> Option<AnyElement> {
        let earlier = self.earlier?;
        let library = self.library.read(cx);
        let row = library.downloads.find(earlier)?;
        Some(earlier_chip(row, cx))
    }

    fn format_row(&self, cx: &App) -> Option<AnyElement> {
        let option = self.picked()?;
        (option.outputs.len() > 1).then(|| {
            let format = self.output.format;
            let rates = fit_for(option, format).is_some() && !format.kbps_choices().is_empty();
            row(
                "Format",
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Select::new(&self.format).accessibility_label("Format")),
                    )
                    .when(rates, |line| {
                        line.child(
                            div().flex_1().min_w_0().child(
                                Select::new(&self.kbps).accessibility_label("Audio quality"),
                            ),
                        )
                    }),
                cx,
            )
        })
    }

    /// How the video keeps its subtitles; a video with none has no row. What the format cannot do
    /// is not offered (D195).
    fn subtitle_rows(&self, cx: &mut Context<'_, Self>) -> Vec<AnyElement> {
        if self.choice.subtitles.is_empty() {
            return Vec::new();
        }
        let fit = self
            .picked()
            .and_then(|option| fit_for(option, self.output.format));
        let shown = self.output.subtitles_shown(fit);
        let toggle = subtitle_toggle(
            "media-subtitles",
            shown,
            holds_subtitles(fit),
            cx.listener(|form, pick: &SubtitleChoice, _, cx| form.pick_subtitles(*pick, cx)),
            cx,
        );
        let mut rows = vec![row("Subtitles", toggle, cx)];
        rows.extend(
            fit.and_then(|fit| subtitle_reason(fit, &self.output))
                .map(|reason| note(reason, cx)),
        );
        if shown != SubtitleChoice::Off {
            rows.push(
                div()
                    .ml(px(NOTE_INSET))
                    .child(
                        Combobox::new(&self.subtitles)
                            .placeholder("None")
                            .search_placeholder("Search languages"),
                    )
                    .into_any_element(),
            );
        }
        rows
    }

    fn chapters(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let offered = self.choice.chapters
            && self
                .picked()
                .and_then(|option| fit_for(option, self.output.format))
                .is_some_and(|fit| fit.chapters);
        offered.then(|| {
            let label = "Add the video's chapters";
            row(
                "Chapters",
                h_flex()
                    .gap_2()
                    .child(
                        Switch::new("media-chapters")
                            .small()
                            .checked(self.output.chapters)
                            .accessibility_label(label)
                            .on_click(cx.listener(|form, on: &bool, _, cx| {
                                form.output.chapters = *on;
                                cx.notify();
                            })),
                    )
                    .child(div().text_role(Role::Meta, cx).child(label)),
                cx,
            )
        })
    }

    fn folder_row(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let shown = self.folder.clone().unwrap_or_else(|| self.app_folder(cx));
        let pick = cx.listener(|_, _, _, cx| {
            let picked = files::choose_folder(cx);
            cx.spawn(async move |form, cx| {
                let Some(folder) = picked.await else {
                    return;
                };
                _ = form.update(cx, |form: &mut Self, cx| {
                    form.folder = (folder != form.app_folder(cx)).then_some(folder);
                    cx.notify();
                });
            })
            .detach();
        });
        row("Folder", folder_box("media-folder", &shown, pick, cx), cx)
    }

    fn buttons(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let busy = self.busy || self.awaiting_addon.is_some();
        let form = cx.entity().downgrade();
        let choose = Rc::new(move |queue: Option<QueueId>, cx: &mut App| {
            _ = form.update(cx, |form, cx| {
                form.queue = queue;
                cx.notify();
            });
        });
        h_flex()
            .w_full()
            .pt_1()
            .gap_2()
            .child(h_flex().flex_1().min_w_0().child(queue_badge_picker(
                "media-queue",
                self.choice.queue,
                &self.choice.queue_reason,
                self.queue,
                choose,
                cx,
            )))
            .child(
                Button::new("media-cancel")
                    .label("Cancel")
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(self.busy)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(MediaAnswer::Cancel))),
            )
            .child(
                Button::new("media-later")
                    .label("Download later")
                    .with_variant(ButtonVariant::Secondary)
                    .disabled(busy)
                    .on_click(cx.listener(|form, _, _, cx| form.answer(true, cx))),
            )
            .child(
                Button::new("media-now")
                    .label(if self.needs_install(cx) {
                        "Install and download"
                    } else {
                        "Download"
                    })
                    .with_variant(ButtonVariant::Primary)
                    .disabled(busy)
                    .on_click(cx.listener(|form, _, _, cx| form.answer(false, cx))),
            )
    }
}

impl Render for MediaForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        self.refresh(window, cx);
        let fit = self
            .picked()
            .and_then(|option| fit_for(option, self.output.format))
            .cloned();
        v_flex()
            .gap_2p5()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(self.heading(cx))
                    .children(self.earlier_note(cx)),
            )
            .when_some(self.choice.missing_quality.clone(), |body, missing| {
                body.child(div().text_role(Role::Meta, cx).child(format!(
                    "This video doesn't offer {missing}, your saved resolution."
                )))
            })
            .child(row(
                "Quality",
                Select::new(&self.quality)
                    .accessibility_label("Quality")
                    .search_placeholder("Search qualities"),
                cx,
            ))
            .children(self.format_row(cx))
            .children(self.addon_note(cx).map(|text| note(text, cx)))
            .children(conversion_note(fit.as_ref(), self.output.kbps).map(|text| note(text, cx)))
            .children(
                quality_notes(fit.as_ref(), self.output.kbps)
                    .into_iter()
                    .map(|text| note(text, cx)),
            )
            .children(self.subtitle_rows(cx))
            .children(self.chapters(cx))
            .child(row("File name", Input::new(&self.file_name), cx))
            .child(self.folder_row(cx))
            .child(self.buttons(cx))
    }
}

fn subtitle_picker(
    choice: &MediaChoice,
    output: &OutputChoice,
    window: &mut Window,
    cx: &mut Context<'_, MediaForm>,
) -> Subtitles {
    let languages: Vec<Choice<String>> = shown_subtitles(&choice.subtitles)
        .into_iter()
        .map(|(id, label)| Choice {
            title: label.into(),
            value: id,
        })
        .collect();
    let ticked = output
        .subtitles
        .iter()
        .filter_map(|id| languages.iter().position(|each| &each.value == id))
        .map(|row| IndexPath::default().row(row))
        .collect();
    cx.new(|cx| {
        ComboboxState::new(SearchableVec::new(languages), ticked, window, cx)
            .multiple(true)
            .searchable(true)
    })
}

fn follow_pickers(
    quality: &Entity<SelectState<SearchableVec<Quality>>>,
    format: &Picker<OutputFormat>,
    kbps: &Picker<Option<u32>>,
    subtitles: &Subtitles,
    cx: &mut Context<'_, MediaForm>,
) -> Vec<Subscription> {
    vec![
        cx.subscribe(
            quality,
            |form, _, event: &SelectEvent<SearchableVec<Quality>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    form.pick_quality(id.clone(), cx);
                }
            },
        ),
        cx.subscribe(
            format,
            |form, _, event: &SelectEvent<SearchableVec<Choice<OutputFormat>>>, cx| {
                if let SelectEvent::Confirm(Some(format)) = event {
                    form.pick_format(*format, cx);
                }
            },
        ),
        cx.subscribe(
            kbps,
            |form, _, event: &SelectEvent<SearchableVec<Choice<Option<u32>>>>, cx| {
                if let SelectEvent::Confirm(Some(kbps)) = event {
                    form.output.kbps = *kbps;
                    form.stale = true;
                    cx.notify();
                }
            },
        ),
        cx.subscribe(
            subtitles,
            |form, _, event: &ComboboxEvent<SearchableVec<Choice<String>>>, cx| {
                if let ComboboxEvent::Change(ids) = event {
                    form.output.pick_languages(ids);
                    cx.notify();
                }
            },
        ),
    ]
}

fn row(label: &'static str, control: impl IntoElement, cx: &App) -> AnyElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
        .into_any_element()
}

fn note(text: String, cx: &App) -> AnyElement {
    div()
        .ml(px(NOTE_INSET))
        .text_role(Role::Meta, cx)
        .child(text)
        .into_any_element()
}

/// The line under a video's title: its site and length, whichever are known.
fn video_meta(page: Option<&str>, length: Option<String>) -> Option<String> {
    let site = page.map(text::host).filter(|host| !host.is_empty());
    match (site, length) {
        (Some(site), Some(length)) => Some(format!("{site} · {length}")),
        (Some(site), None) => Some(site.to_owned()),
        (None, length) => length,
    }
}

/// Where the form's wait for an install stands.
#[derive(Debug, PartialEq, Eq)]
enum InstallWait {
    Ready,
    Started,
    NotYet,
    /// It ended without the add-on, for this reason if it failed.
    Ended(Option<String>),
}

/// The wait, with the add-on now `addon`; `before` is its state when the install was asked for, until the install is seen to start.
fn install_wait(addon: Option<&AddonInfo>, before: Option<&AddonState>) -> InstallWait {
    let Some(addon) = addon else {
        return InstallWait::NotYet;
    };
    if addon.usable {
        return InstallWait::Ready;
    }
    match &addon.state {
        AddonState::Installing { .. } => InstallWait::Started,
        state if Some(state) == before => InstallWait::NotYet,
        AddonState::Failed { message } => InstallWait::Ended(Some(message.clone())),
        _ => InstallWait::Ended(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(state: AddonState, usable: bool) -> AddonInfo {
        AddonInfo {
            id: AddonId::Media,
            name: "Media add-on".to_owned(),
            description: String::new(),
            state,
            usable,
            installed_version: None,
            latest_version: None,
            size: None,
            note: None,
        }
    }

    #[test]
    fn a_list_change_before_the_install_starts_keeps_the_choice() {
        let not_installed = addon(AddonState::NotInstalled, false);
        assert_eq!(
            install_wait(Some(&not_installed), Some(&not_installed.state)),
            InstallWait::NotYet
        );
        let failed = addon(
            AddonState::Failed {
                message: "No connection.".to_owned(),
            },
            false,
        );
        assert_eq!(
            install_wait(Some(&failed), Some(&failed.state)),
            InstallWait::NotYet,
            "the failure of an earlier try"
        );
    }

    #[test]
    fn an_install_that_fails_says_why_and_one_that_stops_ends_the_wait() {
        let failed = addon(
            AddonState::Failed {
                message: "No connection.".to_owned(),
            },
            false,
        );
        let installing = AddonState::Installing {
            done: 0,
            total: None,
        };
        assert_eq!(
            install_wait(
                Some(&addon(installing, false)),
                Some(&AddonState::NotInstalled)
            ),
            InstallWait::Started
        );
        assert_eq!(
            install_wait(Some(&failed), None),
            InstallWait::Ended(Some("No connection.".to_owned()))
        );
        assert_eq!(
            install_wait(Some(&addon(AddonState::NotInstalled, false)), None),
            InstallWait::Ended(None)
        );
    }

    #[test]
    fn a_working_add_on_is_ready_whatever_is_being_done_to_it() {
        let failed_update = addon(
            AddonState::Failed {
                message: "No connection.".to_owned(),
            },
            true,
        );
        assert_eq!(
            install_wait(Some(&failed_update), Some(&failed_update.state)),
            InstallWait::Ready
        );
    }

    #[test]
    fn a_video_without_a_page_or_length_has_no_line_under_its_title() {
        let page = Some("https://example.com/watch");
        let length = || Some("12:04".to_owned());
        assert_eq!(
            video_meta(page, length()).as_deref(),
            Some("example.com · 12:04")
        );
        assert_eq!(video_meta(page, None).as_deref(), Some("example.com"));
        assert_eq!(video_meta(None, length()).as_deref(), Some("12:04"));
        assert_eq!(video_meta(None, None), None);
    }
}
