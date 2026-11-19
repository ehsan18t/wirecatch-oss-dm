//! The extension as a shell (D280): the options its popup shows and the values its own checks
//! use, built from the settings, and a change to one option turned into a settings patch.

use serde_json::{Map, Value, json};
use wirecatch_product::PRODUCT_NAME;
use wirecatch_protocol::app::{
    AppSettings, AppSettingsPatch, MAX_CONCURRENT_DOWNLOADS, MediaDefaultsPatch, OutputDefaults,
    OutputDefaultsPatch, OutputFormat, QualityFallback, SubtitleChoice, UnembeddedSubtitles,
    Update, VideoQuality,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;
use wirecatch_protocol::languages::LANGUAGES;
use wirecatch_protocol::shell::{
    MediaTables, OptionChange, OptionChoice, OptionGroup, OptionSpec, Revision, ShellConfig, kinds,
};
use wirecatch_protocol::wire::QueueSummary;

use crate::error::{CoreError, Result};

const MIB: u64 = 1024 * 1024;
/// The limit a person starts from when turning the speed limit on: a suggestion, not a default.
const FIRST_SPEED_LIMIT: u64 = 5 * MIB;
/// The resolutions offered, tallest first, between Best available and Audio only.
const QUALITY_HEIGHTS: [u32; 6] = [2160, 1440, 1080, 720, 480, 360];

/// The option ids, stable across lists and app versions: the extension only sends them back.
mod ids {
    pub const CAPTURE: &str = "capture_enabled";
    pub const ASK_BEFORE: &str = "ask_before_download";
    pub const ASK_EACH_VIDEO: &str = "ask_each_video";
    pub const PRIVATE_WINDOWS: &str = "private_windows";
    pub const PAGE_SITE: &str = "page_site";
    pub const QUEUE: &str = "default_queue";
    pub const SPEED_LIMIT: &str = "global_speed_limit";
    pub const AT_ONCE: &str = "max_concurrent_downloads";
    pub const MEDIA_BUTTON: &str = "show_media_button";
    pub const SHORTCUT: &str = "shortcut";
    pub const QUALITY: &str = "media_quality";
    pub const FALLBACK: &str = "media_fallback";
    pub const VIDEO_FORMAT: &str = "video_format";
    pub const SUBTITLES: &str = "subtitles";
    pub const LANGUAGES: &str = "subtitle_languages";
    pub const AUTO_SUBTITLES: &str = "auto_subtitles";
    pub const UNEMBEDDED: &str = "unembedded_subtitles";
    pub const CHAPTERS: &str = "chapters";
    pub const AUDIO_FORMAT: &str = "audio_format";
    pub const AUDIO_KBPS: &str = "audio_kbps";
}

/// The values the extension's own checks use.
pub(super) fn shell_config(settings: &AppSettings, revision: Revision) -> ShellConfig {
    let extension = &settings.extension;
    ShellConfig {
        revision,
        capture_enabled: extension.capture_enabled,
        private_windows: extension.private_windows,
        show_media_button: extension.show_media_button,
        ignored_sites: extension.ignored_sites.clone(),
        capture_extensions: extension.capture_extensions.clone(),
        min_capture_size: extension.min_capture_size,
        video_sites: extension.video_sites.clone(),
        media: MediaTables::default(),
    }
}

/// The groups of options the popup shows, in order.
pub(super) fn option_groups(settings: &AppSettings, queues: &[QueueSummary]) -> Vec<OptionGroup> {
    vec![
        capture_group(settings),
        downloads_group(settings, queues),
        media_group(settings),
    ]
}

fn option(
    id: &str,
    kind: &'static str,
    label: &str,
    help: Option<&str>,
    value: Value,
) -> OptionSpec {
    OptionSpec {
        id: id.to_owned(),
        kind,
        label: label.to_owned(),
        help: help.map(str::to_owned),
        disabled: None,
        value,
        extra: Map::new(),
    }
}

fn toggle(id: &str, label: &str, help: &str, on: bool) -> OptionSpec {
    option(id, kinds::TOGGLE, label, Some(help), Value::Bool(on))
}

fn with(mut option: OptionSpec, field: &str, value: Value) -> OptionSpec {
    option.extra.insert(field.to_owned(), value);
    option
}

fn choice(
    id: &str,
    label: &str,
    help: &str,
    value: Value,
    choices: Vec<OptionChoice>,
) -> OptionSpec {
    with(
        option(id, kinds::CHOICE, label, Some(help), value),
        "choices",
        serde_json::to_value(choices).unwrap_or(Value::Null),
    )
}

fn choices_of<T: serde::Serialize>(list: &[(T, &str)]) -> Vec<OptionChoice> {
    list.iter()
        .map(|(value, label)| OptionChoice {
            value: json!(value),
            label: (*label).to_owned(),
            keywords: None,
        })
        .collect()
}

fn browser_row(id: &str, row: &str) -> OptionSpec {
    with(
        option(id, kinds::BROWSER, "", None, Value::Null),
        "browser",
        json!(row),
    )
}

fn to_value(spec: OptionSpec) -> Value {
    serde_json::to_value(spec).unwrap_or(Value::Null)
}

fn group(
    id: &str,
    title: Option<&str>,
    summary: Option<String>,
    options: Vec<OptionSpec>,
) -> OptionGroup {
    OptionGroup {
        id: id.to_owned(),
        title: title.map(str::to_owned),
        summary,
        folded: title.is_some(),
        options: options.into_iter().map(to_value).collect(),
    }
}

fn capture_group(settings: &AppSettings) -> OptionGroup {
    let extension = &settings.extension;
    group(
        "capture",
        None,
        None,
        vec![
            toggle(
                ids::CAPTURE,
                "Capture downloads",
                &format!("Send downloads from this browser to {PRODUCT_NAME}."),
                extension.capture_enabled,
            ),
            toggle(
                ids::ASK_BEFORE,
                "Ask before each download",
                "Choose where each file goes before it starts.",
                extension.ask_before_download,
            ),
            toggle(
                ids::ASK_EACH_VIDEO,
                "Ask about each video",
                "Choose a quality for each video. When off, videos use your Video and audio settings.",
                settings.media.ask_each_time,
            ),
            toggle(
                ids::PRIVATE_WINDOWS,
                "Work in private windows",
                &format!("The browser must also allow {PRODUCT_NAME} there."),
                extension.private_windows,
            ),
            browser_row(ids::PAGE_SITE, "page_site"),
        ],
    )
}

/// A speed as the popup's summary shows it: `5 MB/s`, `1.5 MB/s`, `512 KB/s`.
fn rate_text(bytes_per_second: u64) -> String {
    if bytes_per_second >= MIB {
        // In u128: the settings take a limit up to `i64::MAX`, past what `* 10` fits in a u64.
        let mib = u128::from(MIB);
        let tenths = (u128::from(bytes_per_second) * 10 + mib / 2) / mib;
        let (whole, tenth) = (tenths / 10, tenths % 10);
        if tenth == 0 {
            format!("{whole} MB/s")
        } else {
            format!("{whole}.{tenth} MB/s")
        }
    } else {
        format!("{} KB/s", bytes_per_second.div_ceil(1024))
    }
}

fn downloads_group(settings: &AppSettings, queues: &[QueueSummary]) -> OptionGroup {
    let queue_name = queues
        .iter()
        .find(|q| q.id == settings.extension.default_queue)
        .map(|q| q.name.clone());
    let speed = settings
        .global_speed_limit
        .map_or_else(|| "No speed limit".to_owned(), rate_text);
    // The queue last: a long name is the part that gets cut.
    let summary = [
        Some(speed),
        Some(format!("{} at once", settings.max_concurrent_downloads)),
        queue_name,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let queue_choices = queues
        .iter()
        .map(|q| OptionChoice {
            value: json!(q.id),
            label: q.name.clone(),
            keywords: None,
        })
        .collect();
    let speed_limit = option(
        ids::SPEED_LIMIT,
        kinds::LIMIT,
        "Global speed limit",
        Some("Keeps browsing and calls smooth while downloads run."),
        json!(settings.global_speed_limit),
    );
    let at_once = option(
        ids::AT_ONCE,
        kinds::NUMBER,
        "Downloads at the same time",
        Some("Across all queues."),
        json!(settings.max_concurrent_downloads),
    );
    group(
        "downloads",
        Some("Downloads"),
        Some(summary),
        vec![
            choice(
                ids::QUEUE,
                "Queue for browser downloads",
                "Downloads captured from the browser are added to this queue.",
                json!(settings.extension.default_queue),
                queue_choices,
            ),
            number_fields(speed_limit, "MB/s", MIB, 0.1, None, 0.5, 1)
                .with_field("on_value", json!(FIRST_SPEED_LIMIT)),
            number_fields(
                at_once,
                "",
                1,
                1.0,
                Some(f64::from(MAX_CONCURRENT_DOWNLOADS)),
                1.0,
                0,
            ),
        ],
    )
}

trait WithField {
    fn with_field(self, field: &str, value: Value) -> Self;
}

impl WithField for OptionSpec {
    fn with_field(self, field: &str, value: Value) -> Self {
        with(self, field, value)
    }
}

fn number_fields(
    option: OptionSpec,
    unit: &str,
    scale: u64,
    min: f64,
    max: Option<f64>,
    step: f64,
    decimals: u8,
) -> OptionSpec {
    option
        .with_field("unit", json!(unit))
        .with_field("scale", json!(scale))
        .with_field("min", json!(min))
        .with_field("max", json!(max))
        .with_field("step", json!(step))
        .with_field("decimals", json!(decimals))
}

fn format_label(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Original => "Original",
        OutputFormat::Mp4 => "MP4",
        OutputFormat::Mkv => "MKV",
        OutputFormat::Webm => "WebM",
        OutputFormat::Mp3 => "MP3",
        OutputFormat::M4a => "M4A",
        OutputFormat::Opus => "Opus",
        OutputFormat::Flac => "FLAC",
        OutputFormat::Wav => "WAV",
    }
}

const ALL_FORMATS: [OutputFormat; 9] = [
    OutputFormat::Original,
    OutputFormat::Mp4,
    OutputFormat::Mkv,
    OutputFormat::Webm,
    OutputFormat::Mp3,
    OutputFormat::M4a,
    OutputFormat::Opus,
    OutputFormat::Flac,
    OutputFormat::Wav,
];

fn format_choices(keep: fn(OutputFormat) -> bool) -> Vec<OptionChoice> {
    ALL_FORMATS
        .into_iter()
        .filter(|&f| keep(f))
        .map(|f| OptionChoice {
            value: json!(f),
            label: format_label(f).to_owned(),
            keywords: None,
        })
        .collect()
}

fn quality_value(quality: VideoQuality) -> Value {
    match quality {
        VideoQuality::Best => json!("best"),
        VideoQuality::AudioOnly => json!("audio_only"),
        VideoQuality::Height(height) => json!(height),
    }
}

/// Best available, the usual heights and the saved one if it is another (a pick from the
/// quality window), Audio only.
fn quality_choices(saved: VideoQuality) -> Vec<OptionChoice> {
    let mut heights = QUALITY_HEIGHTS.to_vec();
    if let VideoQuality::Height(height) = saved
        && !heights.contains(&height)
    {
        heights.push(height);
        heights.sort_unstable_by(|a, b| b.cmp(a));
    }
    let mut choices = vec![OptionChoice {
        value: json!("best"),
        label: "Best available".into(),
        keywords: None,
    }];
    choices.extend(heights.into_iter().map(|h| OptionChoice {
        value: json!(h),
        label: format!("{h}p"),
        keywords: None,
    }));
    choices.push(OptionChoice {
        value: json!("audio_only"),
        label: "Audio only".into(),
        keywords: None,
    });
    choices
}

/// Every subtitle language by name with its code, so either finds it; a code saved earlier that
/// the list lacks is kept as it is.
fn language_choices(saved: &[String]) -> Vec<OptionChoice> {
    let mut choices: Vec<OptionChoice> = LANGUAGES
        .iter()
        .map(|language| OptionChoice {
            value: json!(language.code),
            label: format!("{} ({})", language.name, language.code),
            keywords: language.also_called.map(str::to_owned),
        })
        .collect();
    for code in saved {
        if !LANGUAGES.iter().any(|language| language.code == code) {
            choices.push(OptionChoice {
                value: json!(code),
                label: format!("{code} ({code})"),
                keywords: None,
            });
        }
    }
    choices.sort_by(|a, b| a.label.cmp(&b.label));
    choices
}

fn kbps_choices(format: OutputFormat) -> Vec<OptionChoice> {
    let list = format.kbps_choices();
    if list.is_empty() {
        return Vec::new();
    }
    let mut choices = vec![OptionChoice {
        value: json!("usual"),
        label: "Usual quality".into(),
        keywords: None,
    }];
    choices.extend(list.iter().map(|kbps| OptionChoice {
        value: json!(kbps),
        label: format!("{kbps} kbps"),
        keywords: None,
    }));
    choices
}

fn media_group(settings: &AppSettings) -> OptionGroup {
    let media = &settings.media;
    let quality_label = quality_choices(media.quality)
        .into_iter()
        .find(|c| c.value == quality_value(media.quality))
        .map(|c| c.label);
    let summary = [
        quality_label,
        Some(format_label(media.video.format).to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let mut options = video_rows(settings);
    options.extend(subtitle_rows(&media.video));
    options.extend(audio_rows(&media.audio));
    group("media", Some("Video and audio"), Some(summary), options)
}

/// The button over videos, the shortcut, the resolution and the video format.
fn video_rows(settings: &AppSettings) -> Vec<OptionSpec> {
    let media = &settings.media;
    let mut options = vec![
        toggle(
            ids::MEDIA_BUTTON,
            "Show a download button on videos",
            "Adds a small button over video and audio on web pages.",
            settings.extension.show_media_button,
        ),
        browser_row(ids::SHORTCUT, "shortcut"),
        choice(
            ids::QUALITY,
            "Resolution",
            "Used for videos that download without asking, and preselected in the quality window.",
            quality_value(media.quality),
            quality_choices(media.quality),
        ),
    ];
    if matches!(media.quality, VideoQuality::Height(_)) {
        options.push(choice(
            ids::FALLBACK,
            "If a video doesn’t offer that resolution",
            "Take the next lower or next higher resolution, or ask with the quality window.",
            json!(media.fallback),
            choices_of(&[
                (QualityFallback::Lower, "Next lower"),
                (QualityFallback::Higher, "Next higher"),
                (QualityFallback::Ask, "Ask me"),
            ]),
        ));
    }
    options.push(choice(
        ids::VIDEO_FORMAT,
        "Save videos as",
        "Original keeps the site’s format. Other formats convert only when needed.",
        json!(media.video.format),
        format_choices(OutputFormat::saves_video),
    ));
    options
}

/// How a video keeps its subtitles, which languages, and its chapters.
fn subtitle_rows(video: &OutputDefaults) -> Vec<OptionSpec> {
    let mut options = vec![choice(
        ids::SUBTITLES,
        "Subtitles",
        "In the video file, where players show them, or as .srt files next to it.",
        json!(video.subtitles),
        choices_of(&[
            (SubtitleChoice::Off, "Off"),
            (SubtitleChoice::Inside, "In the video"),
            (SubtitleChoice::Files, "Separate files"),
        ]),
    )];
    if video.subtitles != SubtitleChoice::Off {
        options.push(
            option(
                ids::LANGUAGES,
                kinds::CHOICES,
                "Subtitle languages",
                Some("Languages a video doesn’t have are skipped."),
                json!(video.subtitle_languages),
            )
            .with_field(
                "choices",
                json!(language_choices(&video.subtitle_languages)),
            )
            .with_field("placeholder", json!("The video’s language"))
            .with_field("search_placeholder", json!("Search languages")),
        );
        options.push(toggle(
            ids::AUTO_SUBTITLES,
            "Use auto-generated subtitles",
            "Used when a language has no subtitles written by people.",
            video.auto_subtitles,
        ));
    }
    if video.subtitles == SubtitleChoice::Inside {
        options.push(choice(
            ids::UNEMBEDDED,
            "If the format can’t hold subtitles",
            "MP4, MKV and WebM hold subtitles. Original keeps the site’s file as it is, which may not, and audio files hold none.",
            json!(video.unembedded_subtitles),
            choices_of(&[
                (UnembeddedSubtitles::Files, "Save them as separate files"),
                (UnembeddedSubtitles::Skip, "Leave them out"),
            ]),
        ));
    }
    options.push(toggle(
        ids::CHAPTERS,
        "Add chapters",
        "Adds a video’s chapters to MP4, MKV and WebM files, so players can jump between them.",
        video.chapters,
    ));
    options
}

/// The format and quality of audio-only downloads.
fn audio_rows(audio: &OutputDefaults) -> Vec<OptionSpec> {
    let mut options = vec![choice(
        ids::AUDIO_FORMAT,
        "Save audio as",
        "Downloaded audio is converted to this format. MP3, M4A and FLAC files include the title, artist and cover art.",
        json!(audio.format),
        format_choices(OutputFormat::saves_audio),
    )];
    let kbps = kbps_choices(audio.format);
    if !kbps.is_empty() {
        options.push(choice(
            ids::AUDIO_KBPS,
            "Audio quality",
            "Higher sounds better and takes more space.",
            audio
                .audio_kbps
                .map_or_else(|| json!("usual"), |k| json!(k)),
            kbps,
        ));
    }
    options
}

fn wrong_value(id: &str) -> CoreError {
    CoreError::Invalid(format!(
        "That value does not fit the setting \"{id}\". Update the extension or {PRODUCT_NAME} so both use the same version."
    ))
}

fn read<T: serde::de::DeserializeOwned>(id: &str, value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| wrong_value(id))
}

fn set_value(id: &str, change: OptionChange) -> Result<Value> {
    match change {
        OptionChange::Set(value) => Ok(value),
        OptionChange::Edit { .. } => Err(wrong_value(id)),
    }
}

fn video(patch: OutputDefaultsPatch) -> AppSettingsPatch {
    media(MediaDefaultsPatch {
        video: Some(patch),
        ..MediaDefaultsPatch::default()
    })
}

fn media(patch: MediaDefaultsPatch) -> AppSettingsPatch {
    AppSettingsPatch {
        media: Some(patch),
        ..AppSettingsPatch::default()
    }
}

fn extension(patch: ExtensionSettingsPatch) -> AppSettingsPatch {
    AppSettingsPatch {
        extension: Some(patch),
        ..AppSettingsPatch::default()
    }
}

fn quality_of(id: &str, value: Value) -> Result<VideoQuality> {
    match value {
        Value::String(word) if word == "best" => Ok(VideoQuality::Best),
        Value::String(word) if word == "audio_only" => Ok(VideoQuality::AudioOnly),
        Value::Number(height) => height
            .as_u64()
            .and_then(|h| u32::try_from(h).ok())
            .map(VideoQuality::Height)
            .ok_or_else(|| wrong_value(id)),
        _ => Err(wrong_value(id)),
    }
}

/// The settings patch one option's change stands for; checked afterwards like any change.
pub(super) fn option_patch(
    settings: &AppSettings,
    id: &str,
    change: OptionChange,
) -> Result<AppSettingsPatch> {
    if id == ids::LANGUAGES {
        return languages_patch(settings, change);
    }
    let value = set_value(id, change)?;
    if let Some(patch) = extension_patch(id, value.clone())? {
        return Ok(extension(patch));
    }
    if let Some(patch) = download_patch(id, value.clone())? {
        return Ok(patch);
    }
    if let Some(patch) = media_patch(settings, id, value)? {
        return Ok(media(patch));
    }
    Err(CoreError::Invalid(format!(
        "This version of {PRODUCT_NAME} has no setting \"{id}\". Update the extension or {PRODUCT_NAME} so both use the same version."
    )))
}

fn languages_patch(settings: &AppSettings, change: OptionChange) -> Result<AppSettingsPatch> {
    let id = ids::LANGUAGES;
    let mut languages = settings.media.video.subtitle_languages.clone();
    match change {
        OptionChange::Set(value) => languages = read(id, value)?,
        OptionChange::Edit { add, remove } => {
            let add: Vec<String> = read(id, Value::Array(add))?;
            let remove: Vec<String> = read(id, Value::Array(remove))?;
            for code in add {
                if !languages.contains(&code) {
                    languages.push(code);
                }
            }
            languages.retain(|code| !remove.contains(code));
        }
    }
    Ok(video(OutputDefaultsPatch {
        subtitle_languages: Some(languages),
        ..OutputDefaultsPatch::default()
    }))
}

/// The change for an option among the extension's own settings; `None` for another option.
fn extension_patch(id: &str, value: Value) -> Result<Option<ExtensionSettingsPatch>> {
    let none = ExtensionSettingsPatch::default;
    Ok(Some(match id {
        ids::CAPTURE => ExtensionSettingsPatch {
            capture_enabled: Some(read(id, value)?),
            ..none()
        },
        ids::ASK_BEFORE => ExtensionSettingsPatch {
            ask_before_download: Some(read(id, value)?),
            ..none()
        },
        ids::PRIVATE_WINDOWS => ExtensionSettingsPatch {
            private_windows: Some(read(id, value)?),
            ..none()
        },
        ids::MEDIA_BUTTON => ExtensionSettingsPatch {
            show_media_button: Some(read(id, value)?),
            ..none()
        },
        ids::QUEUE => ExtensionSettingsPatch {
            default_queue: Some(read(id, value)?),
            ..none()
        },
        _ => return Ok(None),
    }))
}

/// The change for the speed limit or the downloads at once; `None` for another option.
fn download_patch(id: &str, value: Value) -> Result<Option<AppSettingsPatch>> {
    Ok(Some(match id {
        ids::SPEED_LIMIT => {
            let limit: Option<u64> = read(id, value)?;
            if limit == Some(0) {
                return Err(wrong_value(id));
            }
            AppSettingsPatch {
                global_speed_limit: Update::Set(limit),
                ..AppSettingsPatch::default()
            }
        }
        ids::AT_ONCE => AppSettingsPatch {
            max_concurrent_downloads: Some(read(id, value)?),
            ..AppSettingsPatch::default()
        },
        _ => return Ok(None),
    }))
}

/// The change for an option about video and audio; `None` for another option.
fn media_patch(
    settings: &AppSettings,
    id: &str,
    value: Value,
) -> Result<Option<MediaDefaultsPatch>> {
    let none = MediaDefaultsPatch::default;
    let video = |patch| MediaDefaultsPatch {
        video: Some(patch),
        ..none()
    };
    let output = OutputDefaultsPatch::default;
    Ok(Some(match id {
        ids::ASK_EACH_VIDEO => MediaDefaultsPatch {
            ask_each_time: Some(read(id, value)?),
            ..none()
        },
        ids::QUALITY => MediaDefaultsPatch {
            quality: Some(quality_of(id, value)?),
            ..none()
        },
        ids::FALLBACK => MediaDefaultsPatch {
            fallback: Some(read(id, value)?),
            ..none()
        },
        ids::VIDEO_FORMAT => {
            let format: OutputFormat = read(id, value)?;
            if !format.saves_video() {
                return Err(wrong_value(id));
            }
            video(OutputDefaultsPatch {
                format: Some(format),
                ..output()
            })
        }
        ids::SUBTITLES => video(OutputDefaultsPatch {
            subtitles: Some(read(id, value)?),
            ..output()
        }),
        ids::AUTO_SUBTITLES => video(OutputDefaultsPatch {
            auto_subtitles: Some(read(id, value)?),
            ..output()
        }),
        ids::UNEMBEDDED => video(OutputDefaultsPatch {
            unembedded_subtitles: Some(read(id, value)?),
            ..output()
        }),
        ids::CHAPTERS => video(OutputDefaultsPatch {
            chapters: Some(read(id, value)?),
            ..output()
        }),
        ids::AUDIO_FORMAT | ids::AUDIO_KBPS => MediaDefaultsPatch {
            audio: Some(audio_patch(settings, id, value)?),
            ..none()
        },
        _ => return Ok(None),
    }))
}

/// A new audio format keeps the offered quality nearest the one chosen.
fn audio_patch(settings: &AppSettings, id: &str, value: Value) -> Result<OutputDefaultsPatch> {
    let audio = &settings.media.audio;
    if id == ids::AUDIO_FORMAT {
        let format: OutputFormat = read(id, value)?;
        if !format.saves_audio() {
            return Err(wrong_value(id));
        }
        return Ok(OutputDefaultsPatch {
            format: Some(format),
            audio_kbps: Update::Set(audio.audio_kbps.and_then(|kbps| format.nearest_kbps(kbps))),
            ..OutputDefaultsPatch::default()
        });
    }
    let kbps = match value {
        Value::String(word) if word == "usual" => None,
        value => Some(read::<u32>(id, value)?),
    };
    if kbps.is_some_and(|k| !audio.format.kbps_choices().contains(&k)) {
        return Err(wrong_value(id));
    }
    Ok(OutputDefaultsPatch {
        audio_kbps: Update::Set(kbps),
        ..OutputDefaultsPatch::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::DEFAULT_QUEUE_ID;

    fn queues() -> Vec<QueueSummary> {
        vec![QueueSummary {
            id: DEFAULT_QUEUE_ID,
            name: "Main".into(),
            is_default: true,
        }]
    }

    fn find<'a>(groups: &'a [OptionGroup], id: &str) -> Option<&'a Value> {
        groups
            .iter()
            .flat_map(|g| g.options.iter())
            .find(|o| o["id"] == id)
    }

    #[test]
    fn every_option_has_an_id_a_kind_and_a_label() {
        let groups = option_groups(&wirecatch_settings::default_settings(), &queues());
        assert_eq!(
            groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["capture", "downloads", "media"]
        );
        for option in groups.iter().flat_map(|g| g.options.iter()) {
            assert!(option["id"].is_string(), "{option}");
            assert!(option["kind"].is_string(), "{option}");
            assert!(option["label"].is_string(), "{option}");
        }
    }

    #[test]
    fn the_speed_limit_is_a_limit_in_megabytes_with_a_value_to_start_from() {
        let mut settings = wirecatch_settings::default_settings();
        settings.global_speed_limit = Some(3 * MIB / 2);
        let groups = option_groups(&settings, &queues());
        let speed = find(&groups, ids::SPEED_LIMIT).unwrap();
        assert_eq!(speed["kind"], "limit");
        assert_eq!(speed["value"], json!(3 * MIB / 2));
        assert_eq!(speed["scale"], json!(MIB));
        assert_eq!(speed["on_value"], json!(FIRST_SPEED_LIMIT));
        assert_eq!(
            groups[1].summary.as_deref(),
            Some("1.5 MB/s · 5 at once · Main")
        );

        let patch =
            option_patch(&settings, ids::SPEED_LIMIT, OptionChange::Set(Value::Null)).unwrap();
        assert_eq!(patch.global_speed_limit, Update::Set(None));
        let patch = option_patch(
            &settings,
            ids::SPEED_LIMIT,
            OptionChange::Set(json!(2_621_440)),
        )
        .unwrap();
        assert_eq!(patch.global_speed_limit, Update::Set(Some(2_621_440)));
        assert!(option_patch(&settings, ids::SPEED_LIMIT, OptionChange::Set(json!(-5))).is_err());
    }

    #[test]
    fn a_new_audio_format_keeps_the_nearest_quality_it_offers() {
        let mut settings = wirecatch_settings::default_settings();
        settings.media.audio.format = OutputFormat::Mp3;
        settings.media.audio.audio_kbps = Some(320);
        let patch = option_patch(
            &settings,
            ids::AUDIO_FORMAT,
            OptionChange::Set(json!("opus")),
        )
        .unwrap();
        let audio = patch.media.unwrap().audio.unwrap();
        assert_eq!(audio.format, Some(OutputFormat::Opus));
        let expected = OutputFormat::Opus.nearest_kbps(320);
        assert_eq!(audio.audio_kbps, Update::Set(expected));
        assert!(expected.is_some_and(|k| OutputFormat::Opus.kbps_choices().contains(&k)));
        assert!(
            option_patch(
                &settings,
                ids::AUDIO_FORMAT,
                OptionChange::Set(json!("mp4"))
            )
            .is_err()
        );
    }

    #[test]
    fn languages_change_by_what_is_added_and_removed() {
        let mut settings = wirecatch_settings::default_settings();
        settings.media.video.subtitles = SubtitleChoice::Files;
        settings.media.video.subtitle_languages = vec!["en".into(), "fr".into()];
        let patch = option_patch(
            &settings,
            ids::LANGUAGES,
            OptionChange::Edit {
                add: vec![json!("de")],
                remove: vec![json!("en")],
            },
        )
        .unwrap();
        assert_eq!(
            patch.media.unwrap().video.unwrap().subtitle_languages,
            Some(vec!["fr".into(), "de".into()])
        );
    }

    #[test]
    fn rows_that_depend_on_another_value_show_only_with_it() {
        let mut settings = wirecatch_settings::default_settings();
        settings.media.quality = VideoQuality::Best;
        settings.media.video.subtitles = SubtitleChoice::Off;
        let groups = option_groups(&settings, &queues());
        assert!(find(&groups, ids::FALLBACK).is_none());
        assert!(find(&groups, ids::LANGUAGES).is_none());
        assert!(find(&groups, ids::UNEMBEDDED).is_none());

        settings.media.quality = VideoQuality::Height(1080);
        settings.media.video.subtitles = SubtitleChoice::Inside;
        let groups = option_groups(&settings, &queues());
        assert!(find(&groups, ids::FALLBACK).is_some());
        assert!(find(&groups, ids::LANGUAGES).is_some());
        assert!(find(&groups, ids::UNEMBEDDED).is_some());
    }

    #[test]
    fn a_saved_resolution_outside_the_usual_list_is_offered() {
        let choices = quality_choices(VideoQuality::Height(900));
        assert!(choices.iter().any(|c| c.value == json!(900)));
        assert_eq!(choices.first().unwrap().value, json!("best"));
        assert_eq!(choices.last().unwrap().value, json!("audio_only"));
    }

    #[test]
    fn a_value_of_the_wrong_type_or_an_unknown_id_is_refused() {
        let settings = wirecatch_settings::default_settings();
        assert!(option_patch(&settings, ids::CAPTURE, OptionChange::Set(json!("yes"))).is_err());
        assert!(option_patch(&settings, "teleport", OptionChange::Set(json!(true))).is_err());
        assert!(
            option_patch(
                &settings,
                ids::CAPTURE,
                OptionChange::Edit {
                    add: vec![],
                    remove: vec![]
                }
            )
            .is_err()
        );
    }

    /// The option lists the extension's tests and preview draw, written by this builder so they
    /// never drift from it: `WIRECATCH_WRITE_FIXTURES=1 cargo test -p wirecatch-core options`
    /// writes them again.
    #[test]
    fn the_extension_samples_match_what_the_app_sends() {
        let revision = || Revision {
            instance: "sample".into(),
            counter: 1,
        };
        let mut long = wirecatch_settings::default_settings();
        long.global_speed_limit = Some(3 * MIB / 2);
        long.media.quality = VideoQuality::Height(1080);
        long.media.video.subtitles = SubtitleChoice::Inside;
        long.media.video.subtitle_languages = vec!["en".into(), "pt-br".into()];
        let long_queues = vec![
            queues()[0].clone(),
            QueueSummary {
                id: wirecatch_protocol::QueueId(2),
                name: "Videos for the long weekend trip, sorted by who sent them".into(),
                is_default: false,
            },
        ];
        long.extension.default_queue = wirecatch_protocol::QueueId(2);
        let lists = json!({
            "shell_config": shell_config(&wirecatch_settings::default_settings(), revision()),
            "default": wirecatch_protocol::shell::OptionList {
                revision: revision(),
                groups: option_groups(&wirecatch_settings::default_settings(), &queues()),
            },
            "long": wirecatch_protocol::shell::OptionList {
                revision: revision(),
                groups: option_groups(&long, &long_queues),
            },
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/protocol/src/options.sample.json");
        if std::env::var_os("WIRECATCH_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&lists).unwrap()).unwrap();
        }
        // Compared as JSON: Prettier formats the file with the rest of the package.
        let saved: Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        assert!(
            saved == lists,
            "{} is out of date: run WIRECATCH_WRITE_FIXTURES=1 cargo test -p wirecatch-core options, then pnpm format",
            path.display()
        );
    }

    #[test]
    fn rates_read_as_the_popup_shows_them() {
        assert_eq!(rate_text(5 * MIB), "5 MB/s");
        assert_eq!(rate_text(3 * MIB / 2), "1.5 MB/s");
        assert_eq!(rate_text(512 * 1024), "512 KB/s");
        assert_eq!(rate_text(u64::MAX), "17592186044416 MB/s");
    }
}
