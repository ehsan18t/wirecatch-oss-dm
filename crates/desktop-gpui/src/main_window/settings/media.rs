//! Media: whether a video asks for a quality or downloads at once with the saved choices (D50),
//! the resolution and what to do when a video lacks it, and what videos and audio alone are
//! saved as, each with its own choices.

use gpui_kit::component::IndexPath;
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::{App, Axis, Entity, IntoElement as _, SharedString, Styled as _, px};
use wirecatch_protocol::app::{
    AddonId, AppSettings, AppSettingsPatch, MediaDefaultsPatch, OutputDefaultsPatch, OutputFormat,
    QualityFallback, UnembeddedSubtitles, Update, VideoQuality,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::languages::LANGUAGES;

use super::{Typing, change, choice, current, list_field, switch};
use crate::library::Library;

const ASKING: &[(bool, &str)] = &[(true, "Ask each time"), (false, "Use my choices")];
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
/// In the order a menu lists them, the original first (`OutputFormat::saves_video`).
const VIDEO_FORMATS: &[(OutputFormat, &str)] = &[
    (OutputFormat::Original, "Original"),
    (OutputFormat::Mp4, "MP4"),
    (OutputFormat::Mkv, "MKV"),
    (OutputFormat::Webm, "WebM"),
];
/// In the order a menu lists them, the original first (`OutputFormat::saves_audio`).
const AUDIO_FORMATS: &[(OutputFormat, &str)] = &[
    (OutputFormat::Original, "Original"),
    (OutputFormat::Mp3, "MP3"),
    (OutputFormat::M4a, "M4A"),
    (OutputFormat::Opus, "Opus"),
    (OutputFormat::Flac, "FLAC"),
    (OutputFormat::Wav, "WAV"),
];
const PLACEMENTS: &[(bool, &str)] = &[(false, "Inside the video"), (true, "As separate files")];
const UNEMBEDDED: &[(UnembeddedSubtitles, &str)] = &[
    (UnembeddedSubtitles::Files, "Save them as separate files"),
    (UnembeddedSubtitles::Skip, "Leave them out"),
];
const USUAL: &str = "usual";

pub(super) fn page(library: &Entity<Library>, cx: &App) -> SettingPage {
    SettingPage::new("Media").resettable(false).groups([
        SettingGroup::new()
            .title("Add-on")
            .item(super::addon::item(library, AddonId::Media, cx)),
        video(library, cx),
        video_subtitles(library, cx),
        audio(library, cx),
    ])
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

fn video(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let by_height = current(
        library,
        cx,
        |s| matches!(s.media.quality, VideoQuality::Height(_)),
        false,
    );
    let mut group = SettingGroup::new()
        .title("Video")
        .item(
            SettingItem::new(
                "When you download a video",
                choice(library, ASKING, |s| s.media.ask_each_time, |ask| {
                    media(MediaDefaultsPatch {
                        ask_each_time: Some(ask),
                        ..MediaDefaultsPatch::default()
                    })
                }),
            )
            .description("Ask for a quality each time, or start at once with the settings below.")
            .keywords(["ask", "quality window", "prompt"]),
        )
        .item(
            SettingItem::new(
                "Resolution",
                choice(library, QUALITIES, |s| s.media.quality, |quality| {
                    media(MediaDefaultsPatch {
                        quality: Some(quality),
                        ..MediaDefaultsPatch::default()
                    })
                }),
            )
            .description("Used when Wirecatch doesn't ask, and chosen first in the quality window. Audio-only downloads use the audio settings.")
            .keywords(["quality", "1080p", "4k", "720p", "best"]),
        );
    if by_height {
        group = group.item(fallback(library));
    }
    group
        .item(
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
            .description(
                "Original keeps the site's format. Other formats convert only when needed.",
            )
            .keywords(["format", "mp4", "mkv", "webm", "convert"]),
        )
        .item(
            SettingItem::new(
                "Add chapters",
                switch(library, |s| s.media.video.chapters, |on| {
                    video_patch(OutputDefaultsPatch {
                        chapters: Some(on),
                        ..OutputDefaultsPatch::default()
                    })
                }),
            )
            .description("Adds a video's chapters to MP4, MKV and WebM files, so players can jump between them.")
            .keywords(["chapters"]),
        )
        .item(video_sites(library))
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
        SettingField::render(move |_, window, cx| {
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
        })
    };
    SettingItem::new("Additional video sites", sites)
                .layout(Axis::Vertical)
                .description("Wirecatch also looks for video on pages from these sites. YouTube, Vimeo, TikTok and many others work already. One per line.")
                .keywords(["sites", "video", "pages"])
}

/// Where a video's subtitles go, and auto-generated ones.
fn video_subtitles(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let in_files = current(library, cx, |s| s.media.video.subtitle_files, false);
    let group = SettingGroup::new()
        .title("Video subtitles")
        .item(
            SettingItem::new(
                "Subtitle languages",
                languages(
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
            )
            .description("The creator's subtitles, or auto-generated ones if allowed. Languages a video doesn't have are skipped.")
            .keywords(["subtitles", "captions", "languages"]),
        )
        .item(
            SettingItem::new(
                "Use auto-generated subtitles",
                switch(
                    library,
                    |s| s.media.video.auto_subtitles,
                    |on| {
                        video_patch(OutputDefaultsPatch {
                            auto_subtitles: Some(on),
                            ..OutputDefaultsPatch::default()
                        })
                    },
                ),
            )
            .description("Used when a language has no subtitles written by people.")
            .keywords(["subtitles", "captions", "auto"]),
        )
        .item(
            SettingItem::new(
                "Subtitles",
                choice(
                    library,
                    PLACEMENTS,
                    |s| s.media.video.subtitle_files,
                    |files| {
                        video_patch(OutputDefaultsPatch {
                            subtitle_files: Some(files),
                            ..OutputDefaultsPatch::default()
                        })
                    },
                ),
            )
            .description(
                "Inside the video file, where players show them, or as .srt files next to it.",
            )
            .keywords(["subtitles", "srt", "embed"]),
        );
    // Asked only while subtitles go inside the video (D93).
    if !in_files {
        return group.item(
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
    group
}

fn audio(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let format = current(
        library,
        cx,
        |s| s.media.audio.format,
        OutputFormat::Original,
    );
    let mut group = SettingGroup::new().title("Audio").item(
        SettingItem::new("Save audio as", audio_format(library))
            .description("Downloaded audio is converted to this format. MP3, M4A and FLAC files include the title, artist and cover art.")
            .keywords(["audio", "music", "mp3", "m4a", "opus", "flac", "wav"]),
    );
    if !format.kbps_choices().is_empty() {
        group = group.item(
            SettingItem::new("Audio quality", audio_quality(library, format))
                .description("Higher bit rates sound better and make larger files.")
                .keywords(["bitrate", "kbps", "quality"]),
        );
    }
    group
        .item(
            SettingItem::new(
                "Subtitle languages",
                languages(
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
            )
            .description("Saved as .srt files next to the audio file.")
            .keywords(["subtitles", "captions", "languages"]),
        )
        .item(
            SettingItem::new(
                "Use auto-generated subtitles",
                switch(
                    library,
                    |s| s.media.audio.auto_subtitles,
                    |on| {
                        audio_patch(OutputDefaultsPatch {
                            auto_subtitles: Some(on),
                            ..OutputDefaultsPatch::default()
                        })
                    },
                ),
            )
            .description("Used when a language has no subtitles written by people.")
            .keywords(["subtitles", "captions", "auto"]),
        )
        .item(
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
        )
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
    let wanted = kbps?;
    format
        .kbps_choices()
        .iter()
        .copied()
        .min_by_key(|choice| (choice.abs_diff(wanted), *choice))
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
    SettingField::render(move |_, window, cx| {
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
        Combobox::new(&state)
            .w(px(280.))
            .placeholder("None")
            .search_placeholder("Search languages")
            .into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
