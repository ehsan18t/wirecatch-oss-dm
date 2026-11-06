//! Media: whether a video asks for a quality or downloads at once with the saved choices (D50),
//! whether a pick in the quality window is kept (D200), the resolution and what to do when a
//! video lacks it, and what videos and audio alone are saved as, each with its own choices.

use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{Disableable as _, IndexPath};
use gpui_kit::{App, Axis, Entity, IntoElement as _, SharedString, Styled as _, px};
use wirecatch_protocol::app::{
    AddonId, AppSettings, AppSettingsPatch, MediaDefaultsPatch, OutputDefaultsPatch, OutputFormat,
    QualityFallback, SubtitleChoice, UnembeddedSubtitles, Update, VideoQuality,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::languages::LANGUAGES;
use wirecatch_shell::host;

use super::{Typing, change, choice, current, list_field, switch};
use crate::download_windows::media_output::{nearest_kbps, subtitle_toggle};
use crate::library::Library;
use crate::text::{ASK_EACH_VIDEO, AUDIO_FORMATS, REMEMBER_CHOICES, VIDEO_FORMATS};

const QUALITIES: &[(VideoQuality, &str)] = &[
    (VideoQuality::Best, "Best available"),
    (VideoQuality::Height(2160), "2160p"),
    (VideoQuality::Height(1440), "1440p"),
    (VideoQuality::Height(1080), "1080p"),
    (VideoQuality::Height(720), "720p"),
    (VideoQuality::Height(480), "480p"),
    (VideoQuality::Height(360), "360p"),
    (VideoQuality::AudioOnly, "Audio only"),
];
const FALLBACKS: &[(QualityFallback, &str)] = &[
    (QualityFallback::Lower, "Next lower"),
    (QualityFallback::Higher, "Next higher"),
    (QualityFallback::Ask, "Ask me"),
];
const UNEMBEDDED: &[(UnembeddedSubtitles, &str)] = &[
    (UnembeddedSubtitles::Files, "Save them as separate files"),
    (UnembeddedSubtitles::Skip, "Leave them out"),
];
const USUAL: &str = "usual";
const NEEDS_ADDON: &str = "Install the media add-on above to use these settings.";
const SUBTITLE_LANGUAGES_HELP: &str = "The creator's subtitles, or auto-generated ones if allowed. Languages a video doesn't have are skipped. With none chosen, the video's own language is used.";

pub(super) fn page(library: &Entity<Library>, cx: &App) -> SettingPage {
    let ready = addon_ready(library, cx);
    let video = needing_addon("Video", video(library, cx), ready);
    SettingPage::new(super::Page::Media.title())
        .resettable(false)
        .groups([
            SettingGroup::new()
                .title("Add-on")
                .item(super::addon::item(library, AddonId::Media)),
            if ready {
                video
            } else {
                video.description(NEEDS_ADDON)
            },
            needing_addon("Video subtitles", video_subtitles(library, cx), ready),
            needing_addon("Audio", audio(library, cx), ready),
        ])
}

/// Whether the media add-on works; true until the add-ons are known, so the page does not open
/// dimmed for a moment.
fn addon_ready(library: &Entity<Library>, cx: &App) -> bool {
    library
        .read(cx)
        .addons
        .iter()
        .find(|addon| addon.id == AddonId::Media)
        .is_none_or(|addon| addon.usable)
}

/// `items` under `title`, dimmed and locked until the add-on works.
fn needing_addon(title: &'static str, items: Vec<SettingItem>, ready: bool) -> SettingGroup {
    SettingGroup::new()
        .title(title)
        .items(items.into_iter().map(|item| item.disabled(!ready)))
}

fn media(patch: MediaDefaultsPatch) -> AppSettingsPatch {
    AppSettingsPatch {
        media: Some(patch),
        ..AppSettingsPatch::default()
    }
}

fn video_patch(patch: OutputDefaultsPatch) -> AppSettingsPatch {
    media(MediaDefaultsPatch {
        video: Some(patch),
        ..MediaDefaultsPatch::default()
    })
}

fn audio_patch(patch: OutputDefaultsPatch) -> AppSettingsPatch {
    media(MediaDefaultsPatch {
        audio: Some(patch),
        ..MediaDefaultsPatch::default()
    })
}

/// Every quality to offer, with the saved one when the list does not hold it: the quality window
/// saves any height a video has.
fn quality_options(saved: VideoQuality) -> Vec<(VideoQuality, SharedString)> {
    let mut options: Vec<(VideoQuality, SharedString)> = QUALITIES
        .iter()
        .map(|&(quality, name)| (quality, name.into()))
        .collect();
    if let VideoQuality::Height(height) = saved
        && !options.iter().any(|(quality, _)| *quality == saved)
    {
        let taller = |quality: &VideoQuality| {
            matches!(quality, VideoQuality::Best)
                || matches!(quality, VideoQuality::Height(other) if *other > height)
        };
        let place = options
            .iter()
            .take_while(|(quality, _)| taller(quality))
            .count();
        options.insert(place, (saved, format!("{height}p").into()));
    }
    options
}

fn quality_field(library: &Entity<Library>, cx: &App) -> SettingField<SharedString> {
    let saved = current(library, cx, |s| s.media.quality, VideoQuality::Best);
    let options = quality_options(saved);
    let names = options
        .iter()
        .map(|(_, name)| (name.clone(), name.clone()))
        .collect();
    let library = library.clone();
    let shown = options.clone();
    SettingField::dropdown(
        names,
        move |cx| {
            let saved = current(&library, cx, |s| s.media.quality, VideoQuality::Best);
            shown
                .iter()
                .find(|(quality, _)| *quality == saved)
                .map_or_else(SharedString::default, |(_, name)| name.clone())
        },
        move |name, cx| {
            if let Some((quality, _)) = options.iter().find(|(_, shown)| *shown == name) {
                change(
                    media(MediaDefaultsPatch {
                        quality: Some(*quality),
                        ..MediaDefaultsPatch::default()
                    }),
                    cx,
                );
            }
        },
    )
}

/// The video settings, asking first: whether the quality window opens decides what the rest mean.
fn video(library: &Entity<Library>, cx: &App) -> Vec<SettingItem> {
    let by_height = current(
        library,
        cx,
        |s| matches!(s.media.quality, VideoQuality::Height(_)),
        false,
    );
    let mut items = vec![
        SettingItem::new(
            ASK_EACH_VIDEO,
            switch(library, |s| s.media.ask_each_time, |on| {
                media(MediaDefaultsPatch {
                    ask_each_time: Some(on),
                    ..MediaDefaultsPatch::default()
                })
            }),
        )
        .description("Opens the quality window for each video. Off, videos start at once with the settings below.")
        .keywords(["ask", "quality window", "prompt"]),
        SettingItem::new(
            REMEMBER_CHOICES,
            switch(library, |s| s.media.remember_choices, |on| {
                media(MediaDefaultsPatch {
                    remember_choices: Some(on),
                    ..MediaDefaultsPatch::default()
                })
            }),
        )
        .description("Saves your quality window picks as the settings below. Off, they apply to that video only.")
        .keywords(["remember", "save", "last", "quality window"]),
        SettingItem::new("Resolution", quality_field(library, cx))
            .description(format!("Used when {} doesn't ask, and chosen first in the quality window. Audio-only downloads use the audio settings.", host::CHANNEL.name()))
            .keywords(["quality", "1080p", "4k", "720p", "best"]),
    ];
    if by_height {
        items.push(fallback(library));
    }
    items.extend([
        SettingItem::new(
            "Save videos as",
            choice(
                library,
                VIDEO_FORMATS,
                |s| s.media.video.format,
                |format| {
                    video_patch(OutputDefaultsPatch {
                        format: Some(format),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        )
        .description("Original keeps the site's format. Other formats convert only when needed.")
        .keywords(["format", "mp4", "mkv", "webm", "convert"]),
        SettingItem::new(
            "Add chapters",
            switch(
                library,
                |s| s.media.video.chapters,
                |on| {
                    video_patch(OutputDefaultsPatch {
                        chapters: Some(on),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        )
        .description(
            "Adds a video's chapters to MP4, MKV and WebM files, so players can jump between them.",
        )
        .keywords(["chapters"]),
        video_sites(library),
    ]);
    items
}

/// What is picked when a video lacks the saved resolution.
fn fallback(library: &Entity<Library>) -> SettingItem {
    SettingItem::new(
        "If a video doesn't offer that resolution",
        choice(
            library,
            FALLBACKS,
            |s| s.media.fallback,
            |fallback| {
                media(MediaDefaultsPatch {
                    fallback: Some(fallback),
                    ..MediaDefaultsPatch::default()
                })
            },
        ),
    )
    .description("Take the next lower or next higher resolution, or ask with the quality window.")
    .keywords(["fallback", "lower", "higher"])
}

/// Sites whose pages are read for video, besides those that work already.
fn video_sites(library: &Entity<Library>) -> SettingItem {
    let sites = {
        let library = library.clone();
        SettingField::render(move |options, window, cx| {
            list_field(
                "video-sites".into(),
                &current(
                    &library,
                    cx,
                    |s| s.extension.video_sites.clone(),
                    Vec::new(),
                ),
                Typing::Lines,
                "example.com",
                |sites, cx| {
                    change(
                        AppSettingsPatch {
                            extension: Some(ExtensionSettingsPatch {
                                video_sites: Some(sites),
                                ..ExtensionSettingsPatch::default()
                            }),
                            ..AppSettingsPatch::default()
                        },
                        cx,
                    );
                },
                window,
                cx,
            )
            .disabled(options.is_disabled())
        })
    };
    SettingItem::new("Additional video sites", sites)
                .layout(Axis::Vertical)
                .description(format!("{} also looks for video on pages from these sites. YouTube, Vimeo, TikTok and many others work already. One per line.", host::CHANNEL.name()))
                .keywords(["sites", "video", "pages"])
}

/// The subtitle control the quality window has, showing what `read` comes to (D195).
fn subtitle_field(
    library: &Entity<Library>,
    id: &'static str,
    holds: bool,
    read: fn(&AppSettings) -> SubtitleChoice,
    patch: fn(SubtitleChoice) -> AppSettingsPatch,
) -> SettingField<SharedString> {
    let library = library.clone();
    SettingField::render(move |options, _, cx| {
        let shown = current(&library, cx, read, SubtitleChoice::Off);
        subtitle_toggle(
            id,
            shown,
            holds,
            move |pick, _, cx| change(patch(*pick), cx),
            cx,
        )
        .disabled(options.is_disabled())
        .into_any_element()
    })
}

/// Which subtitle languages to take.
fn languages_item(
    library: &Entity<Library>,
    key: &'static str,
    read: fn(&AppSettings) -> Vec<String>,
    patch: fn(Vec<String>) -> AppSettingsPatch,
) -> SettingItem {
    SettingItem::new("Subtitle languages", languages(library, key, read, patch))
        .description(SUBTITLE_LANGUAGES_HELP)
        .keywords(["subtitles", "captions", "languages"])
}

fn auto_subtitles_item(
    library: &Entity<Library>,
    read: fn(&AppSettings) -> bool,
    patch: fn(bool) -> AppSettingsPatch,
) -> SettingItem {
    SettingItem::new("Use auto-generated subtitles", switch(library, read, patch))
        .description("Used when a language has no subtitles written by people.")
        .keywords(["subtitles", "captions", "auto"])
}

/// How a video keeps its subtitles, then what only applies when it keeps them.
fn video_subtitles(library: &Entity<Library>, cx: &App) -> Vec<SettingItem> {
    let choice_now = current(
        library,
        cx,
        |s| s.media.video.subtitles,
        SubtitleChoice::Off,
    );
    let mut items = vec![
        SettingItem::new(
            "Subtitles",
            subtitle_field(
                library,
                "video-subtitles",
                true,
                |s| s.media.video.subtitles,
                |subtitles| {
                    video_patch(OutputDefaultsPatch {
                        subtitles: Some(subtitles),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        )
        .description("In the video file, where players show them, or as .srt files next to it.")
        .keywords(["subtitles", "captions", "srt", "embed", "off"]),
    ];
    if choice_now == SubtitleChoice::Off {
        return items;
    }
    items.extend([
        languages_item(
            library,
            "video-languages",
            |s| s.media.video.subtitle_languages.clone(),
            |languages| {
                video_patch(OutputDefaultsPatch {
                    subtitle_languages: Some(languages),
                    ..OutputDefaultsPatch::default()
                })
            },
        ),
        auto_subtitles_item(
            library,
            |s| s.media.video.auto_subtitles,
            |on| {
                video_patch(OutputDefaultsPatch {
                    auto_subtitles: Some(on),
                    ..OutputDefaultsPatch::default()
                })
            },
        ),
    ]);
    // Asked only while subtitles go inside the video (D93).
    if choice_now == SubtitleChoice::Inside {
        items.push(
            SettingItem::new(
                "If the format can't hold subtitles",
                choice(
                    library,
                    UNEMBEDDED,
                    |s| s.media.video.unembedded_subtitles,
                    |when| {
                        video_patch(OutputDefaultsPatch {
                            unembedded_subtitles: Some(when),
                            ..OutputDefaultsPatch::default()
                        })
                    },
                ),
            )
            .description("MP4, MKV and WebM hold subtitles. Original keeps the site's file as it is, which may not, and audio files hold none.")
            .keywords(["subtitles", "srt"]),
        );
    }
    items
}

fn audio(library: &Entity<Library>, cx: &App) -> Vec<SettingItem> {
    let format = current(
        library,
        cx,
        |s| s.media.audio.format,
        OutputFormat::Original,
    );
    let mut items = vec![
        SettingItem::new("Save audio as", audio_format(library))
            .description("Downloaded audio is converted to this format. MP3, M4A and FLAC files include the title, artist and cover art.")
            .keywords(["audio", "music", "mp3", "m4a", "opus", "flac", "wav"]),
    ];
    if !format.kbps_choices().is_empty() {
        items.push(
            SettingItem::new("Audio quality", audio_quality(library, format))
                .description("Higher bit rates sound better and make larger files.")
                .keywords(["bitrate", "kbps", "quality"]),
        );
    }
    audio_subtitles(&mut items, library, cx);
    items.push(
        SettingItem::new(
            "Add chapters",
            switch(
                library,
                |s| s.media.audio.chapters,
                |on| {
                    audio_patch(OutputDefaultsPatch {
                        chapters: Some(on),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        )
        .description("Adds the video's chapters to M4A files.")
        .keywords(["chapters"]),
    );
    items
}

/// How audio keeps its subtitles, then what only applies when it keeps them.
fn audio_subtitles(items: &mut Vec<SettingItem>, library: &Entity<Library>, cx: &App) {
    let on = current(
        library,
        cx,
        |s| audio_subtitles_shown(s) != SubtitleChoice::Off,
        false,
    );
    items.push(
        SettingItem::new(
            "Subtitles",
            subtitle_field(
                library,
                "audio-subtitles",
                false,
                audio_subtitles_shown,
                |subtitles| {
                    audio_patch(OutputDefaultsPatch {
                        subtitles: Some(subtitles),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        )
        .description(
            "Audio files cannot hold subtitles, so they are saved as .srt files next to the file.",
        )
        .keywords(["subtitles", "captions", "srt", "off"]),
    );
    if on {
        items.extend([
            languages_item(
                library,
                "audio-languages",
                |s| s.media.audio.subtitle_languages.clone(),
                |languages| {
                    audio_patch(OutputDefaultsPatch {
                        subtitle_languages: Some(languages),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
            auto_subtitles_item(
                library,
                |s| s.media.audio.auto_subtitles,
                |on| {
                    audio_patch(OutputDefaultsPatch {
                        auto_subtitles: Some(on),
                        ..OutputDefaultsPatch::default()
                    })
                },
            ),
        ]);
    }
}

/// What the saved audio choice comes to: no audio format holds subtitles.
fn audio_subtitles_shown(settings: &AppSettings) -> SubtitleChoice {
    let audio = &settings.media.audio;
    audio.subtitles.in_format(false, audio.unembedded_subtitles)
}

/// The audio format; a new one keeps the bit rate nearest the one chosen, when it has a choice.
fn audio_format(library: &Entity<Library>) -> SettingField<SharedString> {
    let options = AUDIO_FORMATS
        .iter()
        .map(|&(_, name)| (SharedString::from(name), SharedString::from(name)))
        .collect();
    let (reading, picking) = (library.clone(), library.clone());
    SettingField::dropdown(
        options,
        move |cx| {
            let format = current(
                &reading,
                cx,
                |s| s.media.audio.format,
                OutputFormat::Original,
            );
            AUDIO_FORMATS
                .iter()
                .find(|(known, _)| *known == format)
                .map_or_else(SharedString::default, |&(_, name)| name.into())
        },
        move |name, cx| {
            let Some(&(format, _)) = AUDIO_FORMATS.iter().find(|(_, n)| *n == name.as_ref()) else {
                return;
            };
            let kept = current(&picking, cx, |s| s.media.audio.audio_kbps, None);
            change(
                audio_patch(OutputDefaultsPatch {
                    format: Some(format),
                    audio_kbps: Update::Set(kbps_for(format, kept)),
                    ..OutputDefaultsPatch::default()
                }),
                cx,
            );
        },
    )
}

/// The bit rates `format` offers, after its usual one.
fn audio_quality(library: &Entity<Library>, format: OutputFormat) -> SettingField<SharedString> {
    let mut options = vec![(
        SharedString::from(USUAL),
        SharedString::from("Usual quality"),
    )];
    options.extend(format.kbps_choices().iter().map(|kbps| {
        (
            SharedString::from(kbps.to_string()),
            SharedString::from(format!("{kbps} kbps")),
        )
    }));
    let reading = library.clone();
    SettingField::dropdown(
        options,
        move |cx| {
            current(&reading, cx, |s| s.media.audio.audio_kbps, None)
                .map_or_else(|| USUAL.into(), |kbps| kbps.to_string().into())
        },
        |key, cx| {
            let kbps = key.as_ref().parse::<u32>().ok();
            change(
                audio_patch(OutputDefaultsPatch {
                    audio_kbps: Update::Set(kbps),
                    ..OutputDefaultsPatch::default()
                }),
                cx,
            );
        },
    )
}

/// The bit rate to keep when the audio format becomes `format`: the one nearest `kbps` when the
/// format has a choice, else the format's usual one.
fn kbps_for(format: OutputFormat, kbps: Option<u32>) -> Option<u32> {
    nearest_kbps(format, kbps?)
}

/// A language to pick subtitles in, found by its name, its code or another name.
#[derive(Clone)]
struct LanguageChoice {
    title: SharedString,
    code: String,
    also_called: Option<&'static str>,
}

impl SearchableListItem for LanguageChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.code
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.title.to_lowercase().contains(&query)
            || self
                .also_called
                .is_some_and(|names| names.to_lowercase().contains(&query))
    }
}

/// Every language, by name with its code, and a code saved earlier that the list lacks.
fn language_choices(saved: &[String]) -> Vec<LanguageChoice> {
    let mut choices: Vec<LanguageChoice> = LANGUAGES
        .iter()
        .map(|language| LanguageChoice {
            title: format!("{} ({})", language.name, language.code).into(),
            code: language.code.to_owned(),
            also_called: language.also_called,
        })
        .collect();
    for code in saved {
        if !LANGUAGES.iter().any(|language| language.code == code) {
            choices.push(LanguageChoice {
                title: format!("{code} ({code})").into(),
                code: code.clone(),
                also_called: None,
            });
        }
    }
    choices.sort_by(|a, b| a.title.cmp(&b.title));
    choices
}

/// Subtitle languages, picked from a searchable list; each change is saved as it is made.
fn languages(
    library: &Entity<Library>,
    key: &'static str,
    read: fn(&AppSettings) -> Vec<String>,
    patch: fn(Vec<String>) -> AppSettingsPatch,
) -> SettingField<SharedString> {
    let library = library.clone();
    SettingField::render(move |options, window, cx| {
        let saved = current(&library, cx, read, Vec::new());
        let state = window.use_keyed_state(SharedString::from(key), cx, |window, cx| {
            let choices = language_choices(&saved);
            let picked = saved
                .iter()
                .filter_map(|code| choices.iter().position(|choice| &choice.code == code))
                .map(|row| IndexPath::default().row(row))
                .collect();
            let state = ComboboxState::new(SearchableVec::new(choices), picked, window, cx)
                .multiple(true)
                .searchable(true);
            cx.subscribe_self(
                move |_, event: &ComboboxEvent<SearchableVec<LanguageChoice>>, cx| {
                    if let ComboboxEvent::Change(codes) = event {
                        change(patch(codes.clone()), cx);
                    }
                },
            )
            .detach();
            state
        });
        // The list keeps its own selection: follow the saved one when it changes elsewhere, such
        // as an import. Comparing with the last saved value leaves a pick not yet saved alone.
        let last = window.use_keyed_state(SharedString::from(format!("{key}-saved")), cx, {
            let saved = saved.clone();
            move |_, _| saved
        });
        if *last.read(cx) != saved {
            last.update(cx, |last, _| last.clone_from(&saved));
            if state.read(cx).selected_values() != saved {
                state.update(cx, |state, cx| {
                    state.set_selected_values(&saved, window, cx);
                });
            }
        }
        Combobox::new(&state)
            .disabled(options.is_disabled())
            .w(px(280.))
            .placeholder("The video's language")
            .search_placeholder("Search languages")
            .into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_height_the_list_lacks_shows_as_itself_in_height_order() {
        let saved = VideoQuality::Height(1200);
        let options = quality_options(saved);
        let place = options.iter().position(|(quality, _)| *quality == saved);
        assert_eq!(place, Some(3));
        assert_eq!(options[3].1.as_ref(), "1200p");
        assert_eq!(options.len(), QUALITIES.len() + 1);
    }

    #[test]
    fn a_listed_quality_adds_nothing() {
        for &(quality, _) in QUALITIES {
            assert_eq!(quality_options(quality).len(), QUALITIES.len());
        }
    }

    #[test]
    fn the_menus_offer_only_formats_that_save_their_kind() {
        assert!(VIDEO_FORMATS.iter().all(|(format, _)| format.saves_video()));
        assert!(AUDIO_FORMATS.iter().all(|(format, _)| format.saves_audio()));
    }

    #[test]
    fn a_new_audio_format_keeps_the_nearest_bit_rate() {
        assert_eq!(kbps_for(OutputFormat::Opus, Some(320)), Some(192));
        assert_eq!(kbps_for(OutputFormat::M4a, Some(160)), Some(128));
        assert_eq!(kbps_for(OutputFormat::Flac, Some(320)), None);
        assert_eq!(kbps_for(OutputFormat::Mp3, None), None);
    }
}
