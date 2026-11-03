//! The quality window's form: quality, Save as, subtitles, chapters and where the file goes.

use std::path::Path;

use gpui_kit::component::IndexPath;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px, relative,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AddonState, Category, MediaChoice, MediaOption, MediaSelection, OutputFormat,
};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::host;

use super::media_output::{
    OutputChoice, conversion_note, fit_for, format_label, kbps_for, quality_notes, size_for,
    subtitle_labels,
};
use super::view::{automatic_reason, folder_box, heading};
use crate::backend::Backend;
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _};
use crate::{files, text};

const LABEL_WIDTH: f32 = 64.;

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
                        .bg(self.accent.opacity(0.14))
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

pub struct MediaForm {
    library: Entity<Library>,
    choice: MediaChoice,
    option_id: String,
    output: OutputChoice,
    file_name: Entity<InputState>,
    folder: Option<String>,
    /// Where audio only goes when its type decides: the question names only the video's folder.
    audio_folder: Option<String>,
    queue_id: QueueId,
    where_open: bool,
    every_video: Option<bool>,
    /// The selects list what the quality and format offer: they are rebuilt on the next draw.
    stale: bool,
    /// An answer is on its way.
    pub busy: bool,
    quality: Entity<SelectState<SearchableVec<Quality>>>,
    format: Picker<OutputFormat>,
    kbps: Picker<Option<u32>>,
    queue: Picker<QueueId>,
    subtitles: Entity<ComboboxState<SearchableVec<Choice<String>>>>,
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
        let output = OutputChoice::proposed(&choice.start);
        let option_id = choice.start.option_id.clone();
        let queue_id = choice.queue;

        let quality = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let format =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let kbps = cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));

        let queue = queue_picker(&library, queue_id, window, cx);
        let subtitles = subtitle_picker(&choice, &output, window, cx);
        let audio_folder = Backend::now(cx).0.and_then(|manager| {
            let folder = manager.type_folder(Category::Audio).ok()?;
            Some(folder.to_string_lossy().into_owned())
        });

        let title = choice.title.clone();
        let file_name = cx.new(|cx| InputState::new(window, cx).placeholder(title));

        let subscriptions = follow_pickers(&quality, &format, &kbps, &queue, &subtitles, cx);

        Self {
            library,
            choice,
            option_id,
            output,
            file_name,
            folder: None,
            audio_folder,
            queue_id,
            where_open: false,
            every_video: None,
            stale: true,
            busy: false,
            quality,
            format,
            kbps,
            queue,
            subtitles,
            _subscriptions: subscriptions,
        }
    }

    pub fn question(&self) -> wirecatch_protocol::QuestionId {
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
            !fit.format.kbps_choices().is_empty()
                && fit.audio != wirecatch_protocol::app::StreamAction::Encode
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
    }

    /// Where the file goes unless the person picked a folder: its queue's, or the one for its type.
    fn app_folder(&self, cx: &App) -> String {
        let library = self.library.read(cx);
        let own = library
            .queue(self.queue_id)
            .and_then(|queue| queue.settings.folder.own_ref())
            .map(|folder| folder.0.clone());
        if let Some(own) = own {
            return own;
        }
        let Some(type_folder) = &self.choice.type_folder else {
            return self.choice.folder.clone();
        };
        self.audio_folder
            .clone()
            .filter(|_| self.picked().is_some_and(|option| option.audio_only))
            .unwrap_or_else(|| type_folder.clone())
    }

    fn queue_name(&self, cx: &App) -> String {
        self.library
            .read(cx)
            .queue(self.queue_id)
            .map(|queue| queue.settings.name.clone())
            .unwrap_or_default()
    }

    /// Why the download goes to the queue shown.
    fn queue_line(&self, cx: &App) -> String {
        let name = self.queue_name(cx);
        if self.queue_id != self.choice.queue {
            return format!("Goes to {name}, as you picked");
        }
        let (lead, value) = automatic_reason(&name, &self.choice.queue_reason);
        value.map_or(lead.clone(), |value| format!("{lead} {value}"))
    }

    fn every_video_shown(&self, cx: &App) -> bool {
        self.every_video.unwrap_or_else(|| {
            let library = self.library.read(cx);
            library
                .settings
                .as_ref()
                .is_some_and(|settings| !settings.media.ask_each_time)
        })
    }

    fn needs_install(&self, cx: &App) -> bool {
        self.picked().is_some_and(|option| option.needs_ffmpeg)
            && !self.library.read(cx).addons.iter().any(|addon| {
                matches!(
                    addon.state,
                    AddonState::Installed | AddonState::UpdateAvailable
                )
            })
    }

    fn join_note(&self, cx: &App) -> Option<String> {
        if !self.picked()?.needs_ffmpeg {
            return None;
        }
        if self.needs_install(cx) {
            let library = self.library.read(cx);
            let size = library
                .addons
                .first()
                .and_then(|addon| addon.size)
                .map_or_else(String::new, |size| format!(" ({})", format_bytes(size)));
            return Some(format!(
                "Video and audio download separately: {} first installs the media add-on{size} to merge them.",
                host::CHANNEL.name(),
            ));
        }
        (self.output.format == OutputFormat::Original).then(|| {
            "Video and audio download separately and are merged into one file at the end."
                .to_owned()
        })
    }

    fn answer(&self, later: bool, cx: &mut Context<'_, Self>) {
        let Some(option) = self.picked() else {
            return;
        };
        let mut selection = self.choice.start.clone();
        selection.option_id.clone_from(&option.id);
        let name = self.file_name.read(cx).value().trim().to_owned();
        selection.file_name = (!name.is_empty()).then_some(name);
        selection.folder.clone_from(&self.folder);
        selection.queue = (self.queue_id != self.choice.queue).then_some(self.queue_id);
        selection.later = later;
        self.output.apply(option, &self.choice, &mut selection);
        selection.every_video = self
            .library
            .read(cx)
            .settings
            .is_some()
            .then(|| self.every_video_shown(cx));
        cx.emit(MediaAnswer::Choose(Box::new(selection)));
    }

    fn save_as(&self, cx: &App) -> Option<AnyElement> {
        let option = self.picked()?;
        (option.outputs.len() > 1).then(|| {
            let format = self.output.format;
            let rates = fit_for(option, format).is_some() && !format.kbps_choices().is_empty();
            row(
                "Save as",
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Select::new(&self.format).accessibility_label("Save as")),
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

    /// Whether the subtitles ticked go inside the file, or how they are saved when they cannot.
    fn subtitle_placement(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        if self.output.subtitles.is_empty() {
            return None;
        }
        let embeds = self
            .picked()
            .and_then(|option| fit_for(option, self.output.format))
            .is_some_and(|fit| fit.embeds_subtitles);
        Some(if embeds {
            Checkbox::new("media-subtitle-files")
                .label("Save subtitles as .srt files next to the video")
                .checked(self.output.subtitle_files)
                .on_click(cx.listener(|form, on: &bool, _, cx| {
                    form.output.subtitle_files = *on;
                    cx.notify();
                }))
                .into_any_element()
        } else {
            note(
                "Subtitles are saved as .srt files next to the file.".to_owned(),
                cx,
            )
        })
    }

    fn chapters(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let offered = self.choice.chapters
            && self
                .picked()
                .and_then(|option| fit_for(option, self.output.format))
                .is_some_and(|fit| fit.chapters);
        offered.then(|| {
            h_flex()
                .w_full()
                .justify_between()
                .child("Add the video's chapters")
                .child(
                    Switch::new("media-chapters")
                        .small()
                        .checked(self.output.chapters)
                        .accessibility_label("Add the video's chapters")
                        .on_click(cx.listener(|form, on: &bool, _, cx| {
                            form.output.chapters = *on;
                            cx.notify();
                        })),
                )
                .into_any_element()
        })
    }

    fn every_video_row(&self, cx: &mut Context<'_, Self>) -> Vec<AnyElement> {
        let on = self.every_video_shown(cx);
        let mut rows = vec![
            Checkbox::new("media-every-video")
                .label("Use these choices for every video")
                .checked(on)
                .on_click(cx.listener(|form, on: &bool, _, cx| {
                    form.every_video = Some(*on);
                    cx.notify();
                }))
                .into_any_element(),
        ];
        if on {
            let at = self.choice.missing_quality.as_ref().map_or_else(
                || "with these choices".to_owned(),
                |missing| format!("at {missing}, as saved"),
            );
            rows.push(note(
                format!("Videos will then download right away {at}. You can change this in Settings, Media."),
                cx,
            ));
        }
        rows
    }

    fn buttons(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let busy = self.busy;
        h_flex()
            .w_full()
            .pt_1()
            .gap_2()
            .justify_end()
            .child(
                Button::new("media-cancel")
                    .label("Cancel")
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy)
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

    fn where_section(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let shown = self.folder.clone().unwrap_or_else(|| self.app_folder(cx));
        let tail = Path::new(&shown).file_name().map_or_else(
            || text::folder(&shown),
            |name| name.to_string_lossy().into_owned(),
        );
        let name = self.file_name.read(cx).value().trim().to_owned();
        let summary = [tail, self.queue_name(cx), name]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let open = self.where_open;
        let pick = cx.listener(|_, _, _, cx| {
            let picked = files::choose_folder(cx);
            cx.spawn(async move |form, cx| {
                let Some(folder) = picked.await else {
                    return;
                };
                _ = form.update(cx, |form: &mut Self, cx| {
                    form.folder = Some(folder);
                    cx.notify();
                });
            })
            .detach();
        });
        let folder = folder_box("media-folder", &shown, pick, cx);
        let toggle = cx.listener(|form, _, _, cx| {
            form.where_open = !form.where_open;
            cx.notify();
        });
        let theme = cx.theme();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .id("media-where")
                    .w_full()
                    .py_1p5()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .child("Save to")
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_role(Role::Meta, cx)
                            .child(summary),
                    )
                    .child(
                        gpui_kit::component::Icon::new(if open {
                            gpui_kit::component::IconName::ChevronDown
                        } else {
                            gpui_kit::component::IconName::ChevronRight
                        })
                        .with_size(crate::ui::tokens::ICON)
                        .text_color(theme.muted_foreground),
                    )
                    .on_click(toggle),
            )
            .when(open, |section| {
                section
                    .child(
                        Field::new("File name", Input::new(&self.file_name))
                            .note("Leave empty to use the video's title."),
                    )
                    .child(Field::new("Folder", folder))
                    .child(Field::new(
                        "Queue",
                        Select::new(&self.queue).accessibility_label("Queue"),
                    ))
            })
            .into_any_element()
    }
}

impl Render for MediaForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        self.refresh(window, cx);
        let fit = self
            .picked()
            .and_then(|option| fit_for(option, self.output.format))
            .cloned();
        let page = self.choice.page_url.clone().unwrap_or_default();
        v_flex()
            .gap_2p5()
            .child(heading(&self.choice.title, &page, None, cx))
            .when_some(self.choice.missing_quality.clone(), |body, missing| {
                body.child(note(
                    format!("This video doesn't offer {missing}, your saved resolution."),
                    cx,
                ))
            })
            .child(row(
                "Quality",
                Select::new(&self.quality)
                    .accessibility_label("Quality")
                    .search_placeholder("Search qualities"),
                cx,
            ))
            .children(self.join_note(cx).map(|text| note(text, cx)))
            .children(self.save_as(cx))
            .children(conversion_note(fit.as_ref(), self.output.kbps).map(|text| note(text, cx)))
            .children(
                quality_notes(fit.as_ref(), self.output.kbps)
                    .into_iter()
                    .map(|text| note(text, cx)),
            )
            .when(!self.choice.subtitles.is_empty(), |body| {
                body.child(row(
                    "Subtitles",
                    Combobox::new(&self.subtitles)
                        .placeholder("None")
                        .search_placeholder("Search languages"),
                    cx,
                ))
            })
            .children(self.subtitle_placement(cx))
            .children(self.chapters(cx))
            .child(self.where_section(cx))
            .child(note(self.queue_line(cx), cx))
            .children(self.every_video_row(cx))
            .child(self.buttons(cx))
    }
}

fn queue_picker(
    library: &Entity<Library>,
    queue_id: QueueId,
    window: &mut Window,
    cx: &mut Context<'_, MediaForm>,
) -> Picker<QueueId> {
    let queues: Vec<Choice<QueueId>> = library
        .read(cx)
        .queues
        .iter()
        .map(|queue| Choice {
            title: queue.settings.name.clone().into(),
            value: queue.id,
        })
        .collect();
    let row = queues.iter().position(|each| each.value == queue_id);
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(queues),
            row.map(|row| IndexPath::default().row(row)),
            window,
            cx,
        )
    })
}

fn subtitle_picker(
    choice: &MediaChoice,
    output: &OutputChoice,
    window: &mut Window,
    cx: &mut Context<'_, MediaForm>,
) -> Entity<ComboboxState<SearchableVec<Choice<String>>>> {
    let languages: Vec<Choice<String>> = choice
        .subtitles
        .iter()
        .zip(subtitle_labels(&choice.subtitles))
        .map(|(subtitle, label)| Choice {
            title: label.into(),
            value: subtitle.id.clone(),
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
    queue: &Picker<QueueId>,
    subtitles: &Entity<ComboboxState<SearchableVec<Choice<String>>>>,
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
            queue,
            |form, _, event: &SelectEvent<SearchableVec<Choice<QueueId>>>, cx| {
                if let SelectEvent::Confirm(Some(queue)) = event {
                    form.queue_id = *queue;
                    cx.notify();
                }
            },
        ),
        cx.subscribe(
            subtitles,
            |form, _, event: &ComboboxEvent<SearchableVec<Choice<String>>>, cx| {
                if let ComboboxEvent::Change(ids) = event {
                    form.output.subtitles.clone_from(ids);
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
        .child(div().flex_1().min_w_0().w(relative(1.)).child(control))
        .into_any_element()
}

fn note(text: String, cx: &App) -> AnyElement {
    div()
        .text_role(Role::Meta, cx)
        .child(text)
        .into_any_element()
}
