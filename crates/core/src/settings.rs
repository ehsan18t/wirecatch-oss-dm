//! Default settings, validation, and the category rules that pick a folder.

use std::collections::HashSet;
use std::path::PathBuf;

use wirecatch_protocol::app::{
    AppSettings, Category, CategoryRule, DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    DEFAULT_MAX_CONCURRENT_DOWNLOADS, DEFAULT_QUEUE_MAX_CONCURRENT, MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD, MediaDefaults, OutputDefaults, OutputFormat, PortablePath, Queue,
    Theme,
};
use wirecatch_protocol::extension::ExtensionSettings;

use crate::error::{CoreError, Result};
use crate::folders::KnownFolders;
use crate::schedule;

pub use wirecatch_protocol::DEFAULT_QUEUE_ID;

const MAX_QUEUE_NAME_CHARS: usize = 60;
/// Speed limits are stored as signed 64-bit integers.
const MAX_SPEED_LIMIT: u64 = i64::MAX as u64;

// Limits on list sizes and lengths keep validation (done under the manager's lock)
// and the stored settings small, whatever a client or a profile file sends.
/// Most queues there can be.
pub const MAX_QUEUES: usize = 100;
/// Most sites in the ignored-sites list.
pub const MAX_IGNORED_SITES: usize = 1000;
/// Most file types in one list (capture filter or a category rule).
pub const MAX_FILE_TYPES: usize = 256;
/// Most category rules.
pub const MAX_CATEGORY_RULES: usize = 64;
/// Longest host name (the DNS limit).
const MAX_SITE_CHARS: usize = 253;
const MAX_FILE_TYPE_CHARS: usize = 32;
const MAX_FOLDER_CHARS: usize = 1024;

/// Settings for a fresh install: everything works without setup (D3).
#[must_use]
pub fn default_settings() -> AppSettings {
    let rule = |category, folder: &str, extensions: &[&str]| CategoryRule {
        category,
        extensions: extensions
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        folder: PortablePath(format!("{{Downloads}}/{folder}")),
    };
    AppSettings {
        default_folder: PortablePath("{Downloads}".into()),
        categories: vec![
            rule(
                Category::Video,
                "Video",
                &[
                    "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "m4v", "mpg", "mpeg", "3gp",
                ],
            ),
            rule(
                Category::Audio,
                "Music",
                &["mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "wma"],
            ),
            rule(
                Category::Archive,
                "Compressed",
                &[
                    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso",
                ],
            ),
            rule(
                Category::Document,
                "Documents",
                &[
                    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf",
                    "txt", "csv", "epub",
                ],
            ),
            rule(
                Category::Program,
                "Programs",
                &[
                    "exe", "msi", "msix", "appimage", "deb", "rpm", "flatpak", "apk", "dmg",
                ],
            ),
            rule(
                Category::Image,
                "Images",
                &[
                    "jpg", "jpeg", "png", "gif", "webp", "avif", "heic", "bmp", "svg", "tiff",
                ],
            ),
        ],
        connections_per_download: DEFAULT_CONNECTIONS_PER_DOWNLOAD,
        max_concurrent_downloads: DEFAULT_MAX_CONCURRENT_DOWNLOADS,
        global_speed_limit: None,
        // The extension judges downloads by the same defaults until it first hears from the app.
        extension: ExtensionSettings::default(),
        theme: Theme::System,
        start_with_system: false,
        close_to_tray: true,
        notify_on_complete: true,
        auto_update_addons: true,
        show_progress_windows: true,
        // Original: nobody's downloads change until they choose (D24).
        media: MediaDefaults::default(),
    }
}

/// The default queue as it is created on first start.
pub(crate) fn default_queue() -> Queue {
    Queue {
        id: DEFAULT_QUEUE_ID,
        name: "Main".into(),
        max_concurrent: DEFAULT_QUEUE_MAX_CONCURRENT,
        speed_limit: None,
        schedule: None,
        is_default: true,
        paused: false,
        running_outside_schedule: false,
    }
}

/// Brings the numbers downloads depend on into the range [`validate`] keeps them in. Stored settings and queues are read back without the checks a change goes through, and an older version or an edit by hand may have stored a value out of range: a stored 0 connections used to run as one connection, and the engine now refuses it.
pub(crate) fn within_limits(mut settings: AppSettings) -> AppSettings {
    settings.connections_per_download = settings
        .connections_per_download
        .clamp(1, MAX_CONNECTIONS_PER_DOWNLOAD);
    settings.max_concurrent_downloads = settings
        .max_concurrent_downloads
        .clamp(1, MAX_CONCURRENT_DOWNLOADS);
    settings.global_speed_limit = speed_limit_within_limits(settings.global_speed_limit);
    settings
}

/// [`within_limits`] for a queue.
pub(crate) fn queue_within_limits(mut queue: Queue) -> Queue {
    queue.max_concurrent = queue.max_concurrent.clamp(1, MAX_CONCURRENT_DOWNLOADS);
    queue.speed_limit = speed_limit_within_limits(queue.speed_limit);
    queue
}

/// A limit of zero would stop every download for good, so it reads as no limit, which is what it was meant to say.
fn speed_limit_within_limits(limit: Option<u64>) -> Option<u64> {
    limit
        .filter(|&limit| limit > 0)
        .map(|limit| limit.min(MAX_SPEED_LIMIT))
}

/// Checks `settings` against the current queues and folders and returns them with
/// lists cleaned up (lower case, no dots, no duplicates).
pub(crate) fn validate(
    mut settings: AppSettings,
    queues: &[Queue],
    folders: &KnownFolders,
) -> Result<AppSettings> {
    check_folder(&settings.default_folder, folders)?;
    if settings.categories.len() > MAX_CATEGORY_RULES {
        return Err(CoreError::Invalid(format!(
            "There can be at most {MAX_CATEGORY_RULES} category rules. Remove some first."
        )));
    }
    for rule in &mut settings.categories {
        check_folder(&rule.folder, folders)?;
        rule.extensions = clean_extensions(&rule.extensions)?;
    }
    if !(1..=MAX_CONNECTIONS_PER_DOWNLOAD).contains(&settings.connections_per_download) {
        return Err(CoreError::Invalid(format!(
            "Connections per download must be between 1 and {MAX_CONNECTIONS_PER_DOWNLOAD}."
        )));
    }
    check_concurrency(settings.max_concurrent_downloads, "Downloads at once")?;
    check_speed_limit(settings.global_speed_limit)?;

    let ext = &mut settings.extension;
    if !queues.iter().any(|q| q.id == ext.default_queue) {
        return Err(CoreError::Invalid(
            "The default queue for the browser no longer exists. Pick another queue.".into(),
        ));
    }
    ext.capture_extensions = clean_extensions(&ext.capture_extensions)?;
    ext.ignored_sites = clean_list(&ext.ignored_sites, normalize_site, &IGNORED_SITES)?;
    check_media_defaults(&mut settings.media.video, false)?;
    check_media_defaults(&mut settings.media.audio, true)?;
    Ok(settings)
}

/// Most subtitle languages remembered for media downloads.
pub const MAX_SUBTITLE_LANGUAGES: usize = 20;
const MAX_LANGUAGE_CHARS: usize = 35;

/// Checks the starting choices for media downloads (D24): a format of the
/// right kind, an audio quality in a sensible range and plain language codes
/// (lower case, without duplicates).
fn check_media_defaults(defaults: &mut OutputDefaults, audio: bool) -> Result<()> {
    use OutputFormat as F;
    let fits = match defaults.format {
        F::Original => true,
        F::Mp4 | F::Mkv | F::Webm => !audio,
        F::Mp3 | F::M4a | F::Opus | F::Flac | F::Wav => audio,
    };
    if !fits {
        return Err(CoreError::Invalid(if audio {
            "Audio downloads can be saved as Original, MP3, M4A, Opus, FLAC or WAV.".into()
        } else {
            "Videos can be saved as Original, MP4, MKV or WebM.".into()
        }));
    }
    if defaults
        .audio_kbps
        .is_some_and(|k| !(32..=512).contains(&k))
    {
        return Err(CoreError::Invalid(
            "The audio quality must be between 32 and 512 kbps.".into(),
        ));
    }
    let mut languages: Vec<String> = Vec::new();
    for language in &defaults.subtitle_languages {
        let language = language.trim().to_ascii_lowercase();
        if language.is_empty() {
            continue;
        }
        let plain = language.len() <= MAX_LANGUAGE_CHARS
            && language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !plain {
            return Err(CoreError::Invalid(format!(
                "\"{language}\" is not a language code. Use codes such as en, fr or pt-BR."
            )));
        }
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    if languages.len() > MAX_SUBTITLE_LANGUAGES {
        return Err(CoreError::Invalid(format!(
            "At most {MAX_SUBTITLE_LANGUAGES} subtitle languages can be remembered. Remove some."
        )));
    }
    defaults.subtitle_languages = languages;
    Ok(())
}

/// Checks that `count` queues are allowed.
pub(crate) fn check_queue_count(count: usize) -> Result<()> {
    if count > MAX_QUEUES {
        return Err(CoreError::LimitReached(format!(
            "There can be at most {MAX_QUEUES} queues. Delete one first."
        )));
    }
    Ok(())
}

/// Checks a queue's name, limits and schedule, trimming the name.
pub(crate) fn validate_queue(mut queue: Queue) -> Result<Queue> {
    queue.name = queue.name.trim().to_string();
    if queue.name.is_empty() {
        return Err(CoreError::Invalid("Give the queue a name.".into()));
    }
    if queue.name.chars().count() > MAX_QUEUE_NAME_CHARS {
        return Err(CoreError::Invalid(format!(
            "Queue names can be at most {MAX_QUEUE_NAME_CHARS} characters."
        )));
    }
    check_concurrency(queue.max_concurrent, "Downloads at once in a queue")?;
    check_speed_limit(queue.speed_limit)?;
    if let Some(s) = &queue.schedule {
        let times_valid =
            schedule::is_valid_time(s.start) && s.stop.is_none_or(schedule::is_valid_time);
        if !times_valid {
            return Err(CoreError::Invalid(
                "Schedule times must be between 00:00 and 23:59.".into(),
            ));
        }
    }
    Ok(queue)
}

/// A category from a `Content-Type`, for a file whose name says nothing (no
/// extension, or one no rule lists).
pub(crate) fn category_from_mime(content_type: &str) -> Category {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let (kind, subtype) = mime.split_once('/').unwrap_or((mime.as_str(), ""));
    match (kind, subtype) {
        ("video", _) => Category::Video,
        ("audio", _) => Category::Audio,
        ("image", _) => Category::Image,
        ("text", "plain" | "csv") | ("application", "pdf" | "msword" | "rtf" | "epub+zip") => {
            Category::Document
        }
        ("application", sub)
            if sub.starts_with("vnd.openxmlformats") || sub.starts_with("vnd.oasis") =>
        {
            Category::Document
        }
        (
            "application",
            "zip" | "x-zip-compressed" | "x-7z-compressed" | "x-rar-compressed" | "vnd.rar"
            | "gzip" | "x-gzip" | "x-tar" | "x-bzip2" | "x-xz" | "zstd" | "x-iso9660-image",
        ) => Category::Archive,
        (
            "application",
            "x-msdownload"
            | "vnd.microsoft.portable-executable"
            | "x-msi"
            | "x-ms-installer"
            | "vnd.debian.binary-package"
            | "x-rpm"
            | "vnd.android.package-archive"
            | "vnd.appimage"
            | "x-apple-diskimage",
        ) => Category::Program,
        _ => Category::Other,
    }
}

/// The category whose rule lists the extension of `file_name`.
pub(crate) fn category_of(settings: &AppSettings, file_name: &str) -> Category {
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return Category::Other;
    };
    let ext = ext.to_lowercase();
    settings
        .categories
        .iter()
        .find(|rule| rule.extensions.contains(&ext))
        .map_or(Category::Other, |rule| rule.category)
}

/// The folder new downloads of `category` go to.
pub(crate) fn folder_for(
    settings: &AppSettings,
    category: Category,
    folders: &KnownFolders,
) -> Result<PathBuf> {
    let folder = settings
        .categories
        .iter()
        .find(|rule| rule.category == category)
        .map_or(&settings.default_folder, |rule| &rule.folder);
    Ok(folders.resolve(folder)?)
}

fn check_concurrency(value: u32, what: &str) -> Result<()> {
    if (1..=MAX_CONCURRENT_DOWNLOADS).contains(&value) {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "{what} must be between 1 and {MAX_CONCURRENT_DOWNLOADS}."
        )))
    }
}

fn check_speed_limit(limit: Option<u64>) -> Result<()> {
    match limit {
        Some(0) => Err(CoreError::Invalid(
            "A speed limit must be above zero. Turn the limit off instead to download at full speed."
                .into(),
        )),
        Some(l) if l > MAX_SPEED_LIMIT => Err(CoreError::Invalid(
            "That speed limit is too large. Turn the limit off instead.".into(),
        )),
        _ => Ok(()),
    }
}

fn check_folder(folder: &PortablePath, folders: &KnownFolders) -> Result<()> {
    if folder.0.chars().count() > MAX_FOLDER_CHARS {
        return Err(CoreError::Invalid(format!(
            "Folder paths can be at most {MAX_FOLDER_CHARS} characters long. Choose a shorter folder."
        )));
    }
    folders.resolve(folder)?;
    Ok(())
}

/// The limits of one list of settings, and how to name it in an error.
struct ListRules {
    /// Plural noun phrase, for example "sites to ignore".
    what: &'static str,
    max_items: usize,
    max_chars: usize,
}

const IGNORED_SITES: ListRules = ListRules {
    what: "sites to ignore",
    max_items: MAX_IGNORED_SITES,
    max_chars: MAX_SITE_CHARS,
};

const FILE_TYPES: ListRules = ListRules {
    what: "file types",
    max_items: MAX_FILE_TYPES,
    max_chars: MAX_FILE_TYPE_CHARS,
};

fn clean_extensions(list: &[String]) -> Result<Vec<String>> {
    clean_list(
        list,
        |e| e.trim().trim_start_matches('.').to_lowercase(),
        &FILE_TYPES,
    )
}

/// A host name the way the ignored-sites list stores it.
pub(crate) fn normalize_site(site: &str) -> String {
    site.trim().trim_end_matches('/').to_lowercase()
}

/// Applies `normalize`, drops empty entries and duplicates, keeps the order, and
/// enforces the list's limits. Linear in the length of the list.
fn clean_list(
    list: &[String],
    normalize: impl Fn(&str) -> String,
    rules: &ListRules,
) -> Result<Vec<String>> {
    let ListRules {
        what,
        max_items,
        max_chars,
    } = rules;
    if list.len() > *max_items {
        return Err(CoreError::Invalid(format!(
            "The list of {what} can hold at most {max_items} entries. Remove some first."
        )));
    }
    let mut seen = HashSet::with_capacity(list.len());
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter().map(|s| normalize(s)) {
        if item.chars().count() > *max_chars {
            return Err(CoreError::Invalid(format!(
                "An entry in the list of {what} is longer than {max_chars} characters. Shorten or remove it."
            )));
        }
        if !item.is_empty() && seen.insert(item.clone()) {
            out.push(item);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{Schedule, TimeOfDay};

    fn folders() -> KnownFolders {
        KnownFolders::under_home(std::env::temp_dir().join("wc-home"))
    }

    /// The extension judges browser downloads by the protocol's defaults until it first hears
    /// from the app (`DEFAULT_EXTENSION_SETTINGS` in `@wirecatch/protocol`), so they must be the
    /// app's own.
    #[test]
    fn a_fresh_install_has_the_extension_defaults_the_extension_assumes() {
        assert_eq!(default_settings().extension, ExtensionSettings::default());
        assert_eq!(default_queue().id, DEFAULT_QUEUE_ID);
    }

    #[test]
    fn defaults_are_valid() {
        let settings = default_settings();
        let checked = validate(settings.clone(), &[default_queue()], &folders()).unwrap();
        assert_eq!(checked, settings);
    }

    /// Media defaults: a format of the right kind, a sane audio quality and
    /// plain language codes, cleaned up.
    #[test]
    fn media_defaults_are_checked_and_cleaned() {
        let check = |video: OutputFormat, audio: OutputFormat| {
            let mut s = default_settings();
            s.media.video.format = video;
            s.media.audio.format = audio;
            validate(s, &[default_queue()], &folders())
        };
        assert!(check(OutputFormat::Mp4, OutputFormat::Mp3).is_ok());
        assert!(check(OutputFormat::Original, OutputFormat::Original).is_ok());
        let wrong = check(OutputFormat::Mp3, OutputFormat::Original).unwrap_err();
        assert!(wrong.to_string().contains("MP4, MKV or WebM"), "{wrong}");
        assert!(check(OutputFormat::Original, OutputFormat::Webm).is_err());

        let mut s = default_settings();
        s.media.video.subtitle_languages =
            vec!["EN".into(), " en ".into(), "pt-BR".into(), String::new()];
        s.media.audio.audio_kbps = Some(320);
        let s = validate(s, &[default_queue()], &folders()).unwrap();
        assert_eq!(s.media.video.subtitle_languages, ["en", "pt-br"]);
        for bad in [Some(8), Some(10_000)] {
            let mut s = default_settings();
            s.media.audio.audio_kbps = bad;
            assert!(validate(s, &[default_queue()], &folders()).is_err());
        }
        let mut s = default_settings();
        s.media.video.subtitle_languages = vec!["en; rm -rf".into()];
        assert!(validate(s, &[default_queue()], &folders()).is_err());
    }

    /// Settings stored before media defaults existed read as Original.
    #[test]
    fn older_settings_start_from_the_original_format() {
        let mut json = serde_json::to_value(default_settings()).unwrap();
        json.as_object_mut().unwrap().remove("media");
        let s: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(s.media, MediaDefaults::default());
        assert_eq!(s.media.video.format, OutputFormat::Original);
    }

    #[test]
    fn validation_cleans_lists() {
        let mut s = default_settings();
        s.extension.capture_extensions =
            vec![".ZIP".into(), "zip".into(), " ".into(), "Mp4".into()];
        s.extension.ignored_sites = vec!["Example.com/".into(), "example.com".into()];
        s.categories[0].extensions = vec![".MKV".into()];
        let s = validate(s, &[default_queue()], &folders()).unwrap();
        assert_eq!(s.extension.capture_extensions, vec!["zip", "mp4"]);
        assert_eq!(s.extension.ignored_sites, vec!["example.com"]);
        assert_eq!(s.categories[0].extensions, vec!["mkv"]);
    }

    #[test]
    fn validation_rejects_bad_values_with_a_reason() {
        let queues = [default_queue()];
        let check = |change: fn(&mut AppSettings)| {
            let mut s = default_settings();
            change(&mut s);
            validate(s, &queues, &folders()).unwrap_err().to_string()
        };
        assert!(check(|s| s.connections_per_download = 0).contains("between 1 and 32"));
        assert!(check(|s| s.max_concurrent_downloads = 0).contains("between 1 and 100"));
        assert!(check(|s| s.global_speed_limit = Some(0)).contains("above zero"));
        assert!(check(|s| s.extension.default_queue = QueueId(9)).contains("no longer exists"));
        assert!(
            check(|s| s.default_folder = PortablePath("rel".into())).contains("full folder path")
        );
    }

    #[test]
    fn list_sizes_and_lengths_are_capped_with_a_reason() {
        let queues = [default_queue()];
        let check = |change: &dyn Fn(&mut AppSettings)| {
            let mut s = default_settings();
            change(&mut s);
            validate(s, &queues, &folders())
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        let many = |n: usize, f: &dyn Fn(usize) -> String| (0..n).map(f).collect::<Vec<_>>();

        let full = check(&|s| {
            s.extension.ignored_sites = many(MAX_IGNORED_SITES, &|i| format!("s{i}.example"));
        });
        assert_eq!(full, Ok(()));
        let over = check(&|s| {
            s.extension.ignored_sites = many(MAX_IGNORED_SITES + 1, &|i| format!("s{i}.example"));
        });
        assert!(over.unwrap_err().contains("at most 1000"));
        let types = check(&|s| {
            s.extension.capture_extensions = many(MAX_FILE_TYPES + 1, &|i| format!("x{i}"));
        });
        assert!(types.unwrap_err().contains("at most 256"));
        let long_site = check(&|s| s.extension.ignored_sites = vec!["a".repeat(300)]);
        assert!(long_site.unwrap_err().contains("longer than 253"));
        let long_type = check(&|s| s.categories[0].extensions = vec!["x".repeat(40)]);
        assert!(long_type.unwrap_err().contains("longer than 32"));
        let rules =
            check(&|s| s.categories = vec![s.categories[0].clone(); MAX_CATEGORY_RULES + 1]);
        assert!(rules.unwrap_err().contains("at most 64"));
        let deep = check(&|s| {
            s.default_folder = PortablePath(format!("{{Downloads}}/{}", "d/".repeat(600)));
        });
        assert!(deep.unwrap_err().contains("at most 1024"));
        assert!(check_queue_count(MAX_QUEUES).is_ok());
        assert!(
            check_queue_count(MAX_QUEUES + 1)
                .unwrap_err()
                .to_string()
                .contains("at most 100")
        );
    }

    #[test]
    fn duplicates_in_a_full_list_collapse() {
        let mut s = default_settings();
        s.extension.ignored_sites = (0..MAX_IGNORED_SITES)
            .map(|i| format!("S{}.example", i % 10))
            .collect();
        let s = validate(s, &[default_queue()], &folders()).unwrap();
        assert_eq!(s.extension.ignored_sites.len(), 10);
        assert_eq!(s.extension.ignored_sites[0], "s0.example");
    }

    #[test]
    fn queue_validation() {
        let mut q = default_queue();
        q.name = "  Night  ".into();
        assert_eq!(validate_queue(q.clone()).unwrap().name, "Night");
        q.name = " ".into();
        assert!(validate_queue(q.clone()).is_err());
        q.name = "Night".into();
        q.schedule = Some(Schedule {
            start: TimeOfDay {
                hour: 24,
                minute: 0,
            },
            stop: None,
            days: vec![],
        });
        assert!(validate_queue(q).unwrap_err().to_string().contains("00:00"));
    }

    #[test]
    fn category_and_folder_come_from_the_rules() {
        let s = default_settings();
        let f = folders();
        assert_eq!(category_of(&s, "Movie.MKV"), Category::Video);
        assert_eq!(category_of(&s, "backup.tar.gz"), Category::Archive);
        assert_eq!(category_of(&s, "README"), Category::Other);
        assert_eq!(
            folder_for(&s, Category::Video, &f).unwrap(),
            f.downloads.join("Video")
        );
        assert_eq!(folder_for(&s, Category::Other, &f).unwrap(), f.downloads);
    }
}
