//! Default settings, validation, and the category rules that pick a folder.

mod error;
pub mod folders;
pub mod proxy;
pub mod settings_file;

use std::collections::HashSet;
use std::path::PathBuf;

use wirecatch_protocol::app::{
    Accent, AppSettings, Category, CategoryRule, DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    DEFAULT_MAX_CONCURRENT_DOWNLOADS, FinishNoticeTime, MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD, MAX_QUEUE_DESCRIPTION_CHARS, MediaDefaults, NotifyOnFinish,
    NotifyOnStart, OutputDefaults, PortablePath, Queue, QueueSettings, QueueStatus, Setting,
    StartNoticeTime, Theme, VideoQuality, WhenFileExists,
};
use wirecatch_protocol::extension::{
    ExtensionSettings, ExtensionSettingsPatch, MAX_IGNORED_SITES, MAX_SITE_CHARS, MAX_VIDEO_SITES,
    NotASite, SitesRefused, normalize_file_type, read_site,
};
use wirecatch_protocol::proxy::{AppExceptions, ProxySettings};

use wirecatch_protocol::rules::AddDownloads;
use wirecatch_queues::{rules, schedule};
use wirecatch_sites::local_network::{self, NotALocalSite};
use wirecatch_sites::public_suffix::SuffixList;

use crate::folders::KnownFolders;

pub use error::{Result, SettingsError, SettingsFileError};

pub use wirecatch_protocol::DEFAULT_QUEUE_ID;

const MAX_QUEUE_NAME_CHARS: usize = 60;
/// Speed limits are stored as signed 64-bit integers.
const MAX_SPEED_LIMIT: u64 = i64::MAX as u64;

// Limits on list sizes and lengths keep validation (done under the manager's lock)
// and the stored settings small, whatever a client or a profile file sends.
/// Most queues there can be.
pub const MAX_QUEUES: usize = 100;
/// Most file types in one list (capture filter or a category rule).
pub const MAX_FILE_TYPES: usize = 256;
/// Most category rules.
pub const MAX_CATEGORY_RULES: usize = 64;
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
        accent: Accent::Cyan,
        start_with_system: false,
        close_to_tray: true,
        notify_on_finish: NotifyOnFinish::Notification,
        finish_notice_time: FinishNoticeTime::EightSeconds,
        notify_on_start: NotifyOnStart::Browser,
        start_notice_time: StartNoticeTime::ThreeSeconds,
        notify_during_do_not_disturb: true,
        auto_update_addons: true,
        show_progress_windows: true,
        when_file_exists: WhenFileExists::Ask,
        // Original: nobody's downloads change until they choose (D24).
        media: MediaDefaults::default(),
        local_sites: Vec::new(),
        update_suffix_list: true,
        prerelease_updates: false,
        proxy: ProxySettings::default(),
    }
}

/// The default queue as it is created on first start.
#[must_use]
pub fn default_queue() -> Queue {
    Queue {
        id: DEFAULT_QUEUE_ID,
        is_default: true,
        settings: QueueSettings::new("Main"),
        status: QueueStatus::default(),
    }
}

/// Brings the numbers downloads depend on into the range [`validate`] keeps them in, and keeps the ignored sites the way the list keeps them. Stored settings and queues are read back without the checks a change goes through, and an older version or an edit by hand may have stored a value out of range: a stored 0 connections used to run as one connection, and the engine now refuses it.
#[must_use]
pub fn within_limits(mut settings: AppSettings, suffixes: &SuffixList) -> AppSettings {
    settings.connections_per_download = settings
        .connections_per_download
        .clamp(1, MAX_CONNECTIONS_PER_DOWNLOAD);
    settings.max_concurrent_downloads = settings
        .max_concurrent_downloads
        .clamp(1, MAX_CONCURRENT_DOWNLOADS);
    settings.global_speed_limit = speed_limit_within_limits(settings.global_speed_limit);
    settings.extension.ignored_sites = stored_sites(settings.extension.ignored_sites);
    settings.local_sites = stored_local_sites(&settings.local_sites, suffixes);
    // Exceptions this build would refuse cannot be matched as they are; the defaults take
    // their place until the person sets them again.
    settings.proxy = crate::proxy::validate(settings.proxy.clone()).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "the stored proxy exceptions cannot be used; using the defaults instead");
        ProxySettings {
            exceptions: AppExceptions::default(),
            ..settings.proxy
        }
    });
    settings
}

/// The stored ignored sites the way the list keeps them, without the ones a change would refuse. None of those can match a host the browser reports, so nothing that was ignored stops being ignored; each one changed or dropped is logged by its place in the list, since an entry may hold a password (`user:password@host`).
pub(crate) fn stored_sites(stored: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::with_capacity(stored.len());
    let mut kept = Vec::with_capacity(stored.len());
    for (entry, site) in stored.into_iter().enumerate() {
        match read_site(&site) {
            Ok(Some(host)) if host.chars().count() <= MAX_SITE_CHARS => {
                if host != site {
                    tracing::info!(entry, kept_as = %host, "a stored site to ignore is kept as its host");
                }
                if seen.insert(host.clone()) {
                    kept.push(host);
                }
            }
            Ok(Some(_)) => tracing::info!(
                entry,
                "a stored site to ignore is longer than a host name can be, so it is dropped"
            ),
            Ok(None) | Err(_) => tracing::info!(
                entry,
                "a stored site to ignore names no host, so it is dropped"
            ),
        }
    }
    kept
}

/// The stored sites on the person's network that [`validate`] would keep with `suffixes`, without duplicates. One a change would refuse is dropped, since one that covers the sites of many owners must never count; each one dropped is logged by its place in the list. A start reads the stored list through it, and so does a newer suffix list taken while the app runs.
pub fn stored_local_sites(stored: &[String], suffixes: &SuffixList) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(stored.len());
    for (entry, site) in stored.iter().enumerate() {
        match local_network::local_site(site, suffixes) {
            Ok(Some(site)) if site.chars().count() <= MAX_SITE_CHARS => {
                if !kept.contains(&site) {
                    kept.push(site);
                }
            }
            Ok(None) => {}
            Ok(Some(_)) | Err(_) => tracing::info!(
                entry,
                "a stored site on the network could not be saved today, so it is dropped"
            ),
        }
    }
    kept.truncate(MAX_LOCAL_SITES);
    kept
}

/// Checks the sites a patch from the browser or a window names, before the manager's lock is taken (see [`ExtensionSettingsPatch::check_sites`]).
pub fn check_site_patch(patch: &ExtensionSettingsPatch) -> Result<()> {
    patch.check_sites().map_err(|refused| match refused {
        SitesRefused::TooMany => too_many(&IGNORED_SITES),
        SitesRefused::NotASite(not) => not_a_site(&not),
    })
}

fn not_a_site(NotASite(entry): &NotASite) -> SettingsError {
    SettingsError::Invalid(format!(
        "\"{}\" is not a site. Type a host name such as example.com, or a web address.",
        entry.trim()
    ))
}

fn too_many(rules: &ListRules) -> SettingsError {
    let ListRules {
        what, max_items, ..
    } = rules;
    SettingsError::Invalid(format!(
        "The list of {what} can hold at most {max_items} entries. Remove some first."
    ))
}

/// [`within_limits`] for a queue.
#[must_use]
pub fn queue_within_limits(mut settings: QueueSettings) -> QueueSettings {
    if let Setting::Own(max) = &mut settings.max_concurrent {
        *max = (*max).clamp(1, MAX_CONCURRENT_DOWNLOADS);
    }
    if let Setting::Own(connections) = &mut settings.connections {
        *connections = (*connections).clamp(1, MAX_CONNECTIONS_PER_DOWNLOAD);
    }
    if let Setting::Own(limit) = settings.speed_limit {
        settings.speed_limit =
            speed_limit_within_limits(Some(limit)).map_or(Setting::Global, Setting::Own);
    }
    settings
}

/// A limit of zero would stop every download for good, so it reads as no limit, which is what it was meant to say.
fn speed_limit_within_limits(limit: Option<u64>) -> Option<u64> {
    limit
        .filter(|&limit| limit > 0)
        .map(|limit| limit.min(MAX_SPEED_LIMIT))
}

/// Checks `settings` against the current queues and folders and returns them with
/// lists cleaned up (lower case, no dots, no duplicates).
pub fn validate(
    mut settings: AppSettings,
    queues: &[Queue],
    folders: &KnownFolders,
    suffixes: &SuffixList,
) -> Result<AppSettings> {
    check_folder(&settings.default_folder, folders)?;
    if settings.categories.len() > MAX_CATEGORY_RULES {
        return Err(SettingsError::Invalid(format!(
            "There can be at most {MAX_CATEGORY_RULES} category rules. Remove some first."
        )));
    }
    for rule in &mut settings.categories {
        check_folder(&rule.folder, folders)?;
        rule.extensions = clean_extensions(&rule.extensions)?;
    }
    check_connections(settings.connections_per_download)?;
    check_concurrency(
        settings.max_concurrent_downloads,
        "Downloads at the same time",
    )?;
    check_speed_limit(settings.global_speed_limit)?;

    let ext = &mut settings.extension;
    if !queues.iter().any(|q| q.id == ext.default_queue) {
        return Err(SettingsError::Invalid(
            "The default queue for the browser no longer exists. Pick another queue.".into(),
        ));
    }
    ext.capture_extensions = clean_extensions(&ext.capture_extensions)?;
    ext.ignored_sites = clean_list(
        &ext.ignored_sites,
        |site| {
            read_site(site)
                .map(Option::unwrap_or_default)
                .map_err(|not| not_a_site(&not))
        },
        &IGNORED_SITES,
    )?;
    ext.video_sites = clean_list(
        &ext.video_sites,
        |site| {
            read_site(site)
                .map(Option::unwrap_or_default)
                .map_err(|not| not_a_site(&not))
        },
        &VIDEO_SITES,
    )?;
    check_media_defaults(&mut settings.media.video, Saves::Video)?;
    check_media_defaults(&mut settings.media.audio, Saves::Audio)?;
    if settings.media.quality == VideoQuality::Height(0) {
        return Err(SettingsError::Invalid(
            "The resolution must be a height in pixels, such as 1080.".into(),
        ));
    }
    settings.local_sites = clean_local_sites(&settings.local_sites, suffixes)?;
    settings.proxy = crate::proxy::validate(settings.proxy)?;
    Ok(settings)
}

/// Most sites on the person's network (D39).
pub const MAX_LOCAL_SITES: usize = 256;

const LOCAL_SITES: ListRules = ListRules {
    what: "sites on your network",
    max_items: MAX_LOCAL_SITES,
    max_chars: MAX_SITE_CHARS,
};

/// The sites on the person's network as [`local_network::local_site`] keeps
/// them, without duplicates. An address, a name that is not a host, and a
/// public suffix or a name above one (`com`, `co.uk`, `github.io`) are
/// refused: single-label hosts such as `nas` and pages at a local address
/// count as local without being listed.
fn clean_local_sites(list: &[String], suffixes: &SuffixList) -> Result<Vec<String>> {
    clean_list(
        list,
        |entry| {
            local_network::local_site(entry, suffixes)
                .map(Option::unwrap_or_default)
                .map_err(|why| not_a_local_site(entry.trim(), why))
        },
        &LOCAL_SITES,
    )
}

fn not_a_local_site(entry: &str, why: NotALocalSite) -> SettingsError {
    SettingsError::Invalid(match why {
        NotALocalSite::Unreadable => format!(
            "\"{entry}\" is not a site. Type a host name such as wiki.example.com, or a web address."
        ),
        NotALocalSite::Address => format!(
            "\"{entry}\" is an address, not a site name. A page at an address on your network already counts as on your network."
        ),
        NotALocalSite::Shared => format!(
            "\"{entry}\" is shared by the sites of many owners, so it would cover every site that ends in it. Enter your own site, such as wiki.example.com or example.com."
        ),
    })
}

/// Most subtitle languages remembered for media downloads.
pub const MAX_SUBTITLE_LANGUAGES: usize = 20;
/// Longest subtitle language code kept.
pub const MAX_LANGUAGE_CHARS: usize = 35;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Saves {
    Video,
    Audio,
}

/// Checks the starting choices for media downloads (D24): a format of the
/// right kind, an audio quality in a sensible range and plain language codes
/// (lower case, without duplicates).
fn check_media_defaults(defaults: &mut OutputDefaults, saves: Saves) -> Result<()> {
    let fits = match saves {
        Saves::Audio => defaults.format.saves_audio(),
        Saves::Video => defaults.format.saves_video(),
    };
    if !fits {
        return Err(SettingsError::Invalid(if saves == Saves::Audio {
            "Audio downloads can be saved as Original, MP3, M4A, Opus, FLAC or WAV.".into()
        } else {
            "Videos can be saved as Original, MP4, MKV or WebM.".into()
        }));
    }
    if defaults
        .audio_kbps
        .is_some_and(|k| !(32..=512).contains(&k))
    {
        return Err(SettingsError::Invalid(
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
            return Err(SettingsError::Invalid(format!(
                "\"{language}\" is not a language code. Use codes such as en, fr or pt-BR."
            )));
        }
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    if languages.len() > MAX_SUBTITLE_LANGUAGES {
        return Err(SettingsError::Invalid(format!(
            "At most {MAX_SUBTITLE_LANGUAGES} subtitle languages can be remembered. Remove some."
        )));
    }
    defaults.subtitle_languages = languages;
    Ok(())
}

/// Checks that `count` queues are allowed.
pub fn check_queue_count(count: usize) -> Result<()> {
    if count > MAX_QUEUES {
        return Err(SettingsError::LimitReached(format!(
            "There can be at most {MAX_QUEUES} queues. Delete one first."
        )));
    }
    Ok(())
}

/// Checks a queue's name, limits, folder and schedule, trimming the name.
pub fn validate_queue(mut queue: QueueSettings, folders: &KnownFolders) -> Result<QueueSettings> {
    queue.name = queue.name.trim().to_string();
    if queue.name.is_empty() {
        return Err(SettingsError::Invalid("Give the queue a name.".into()));
    }
    if queue.name.chars().count() > MAX_QUEUE_NAME_CHARS {
        return Err(SettingsError::Invalid(format!(
            "Queue names can be at most {MAX_QUEUE_NAME_CHARS} characters."
        )));
    }
    queue.description = queue.description.trim().to_string();
    if queue.description.chars().count() > MAX_QUEUE_DESCRIPTION_CHARS {
        return Err(SettingsError::Invalid(format!(
            "A queue's description can be at most {MAX_QUEUE_DESCRIPTION_CHARS} characters."
        )));
    }
    if let Setting::Own(max) = queue.max_concurrent {
        check_concurrency(max, "A queue’s downloads at the same time")?;
    }
    if let Setting::Own(limit) = queue.speed_limit {
        check_speed_limit(Some(limit))?;
    }
    if let Setting::Own(connections) = queue.connections {
        check_connections(connections)?;
    }
    if let Setting::Own(folder) = &queue.folder {
        check_folder(folder, folders)?;
    }
    queue.exceptions = crate::proxy::validate_queue_exceptions(queue.exceptions)?;
    if let AddDownloads::Auto(rules) = queue.add_downloads {
        queue.add_downloads =
            AddDownloads::Auto(rules::checked(rules).map_err(SettingsError::Invalid)?);
    }
    if let Some(s) = &queue.schedule {
        let times_valid =
            schedule::is_valid_time(s.start) && s.stop.is_none_or(schedule::is_valid_time);
        if !times_valid {
            return Err(SettingsError::Invalid(
                "Schedule times must be between 00:00 and 23:59.".into(),
            ));
        }
    }
    Ok(queue)
}

/// A category from a `Content-Type`, for a file whose name says nothing (no
/// extension, or one no rule lists).
#[must_use]
pub fn category_from_mime(content_type: &str) -> Category {
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
#[must_use]
pub fn category_of(settings: &AppSettings, file_name: &str) -> Category {
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

/// The folder new downloads of `category` go to in `queue` (D94.2).
pub fn folder_for(
    settings: &AppSettings,
    queue: &QueueSettings,
    category: Category,
    folders: &KnownFolders,
) -> Result<PathBuf> {
    match &queue.folder {
        Setting::Own(folder) => Ok(folders.resolve(folder)?),
        Setting::Global => type_folder(settings, category, folders),
    }
}

/// The folder for `category` in a queue that follows the app.
pub fn type_folder(
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

fn check_connections(value: u8) -> Result<()> {
    if (1..=MAX_CONNECTIONS_PER_DOWNLOAD).contains(&value) {
        Ok(())
    } else {
        Err(SettingsError::Invalid(format!(
            "Connections per download must be between 1 and {MAX_CONNECTIONS_PER_DOWNLOAD}."
        )))
    }
}

fn check_concurrency(value: u32, what: &str) -> Result<()> {
    if (1..=MAX_CONCURRENT_DOWNLOADS).contains(&value) {
        Ok(())
    } else {
        Err(SettingsError::Invalid(format!(
            "{what} must be between 1 and {MAX_CONCURRENT_DOWNLOADS}."
        )))
    }
}

fn check_speed_limit(limit: Option<u64>) -> Result<()> {
    match limit {
        Some(0) => Err(SettingsError::Invalid(
            "A speed limit must be above zero. Turn the limit off instead to download at full speed."
                .into(),
        )),
        Some(l) if l > MAX_SPEED_LIMIT => Err(SettingsError::Invalid(
            "That speed limit is too large. Turn the limit off instead.".into(),
        )),
        _ => Ok(()),
    }
}

fn check_folder(folder: &PortablePath, folders: &KnownFolders) -> Result<()> {
    if folder.0.chars().count() > MAX_FOLDER_CHARS {
        return Err(SettingsError::Invalid(format!(
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

const VIDEO_SITES: ListRules = ListRules {
    what: "video sites",
    max_items: MAX_VIDEO_SITES,
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
        |kind| Ok(normalize_file_type(kind).unwrap_or_default()),
        &FILE_TYPES,
    )
}

/// Applies `normalize`, drops empty entries and duplicates, keeps the order, and enforces the list's limits, the number of entries before any is read. Linear in the length of the list.
fn clean_list(
    list: &[String],
    normalize: impl Fn(&str) -> Result<String>,
    rules: &ListRules,
) -> Result<Vec<String>> {
    let ListRules {
        what,
        max_items,
        max_chars,
    } = rules;
    if list.len() > *max_items {
        return Err(too_many(rules));
    }
    let mut seen = HashSet::with_capacity(list.len());
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter().map(|s| normalize(s)) {
        let item = item?;
        if item.chars().count() > *max_chars {
            return Err(SettingsError::Invalid(format!(
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
    use wirecatch_protocol::app::{OutputFormat, Schedule, TimeOfDay};

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

    /// D36: a fresh profile follows the system's light or dark, with Wirecatch's cyan.
    #[test]
    fn a_fresh_install_follows_the_system_theme_with_the_cyan_accent() {
        let settings = default_settings();
        assert_eq!(
            (settings.theme, settings.accent),
            (Theme::System, Accent::Cyan)
        );
    }

    #[test]
    fn defaults_are_valid() {
        let settings = default_settings();
        let checked = validate(
            settings.clone(),
            &[default_queue()],
            &folders(),
            &SuffixList::built_in(),
        )
        .unwrap();
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
            validate(s, &[default_queue()], &folders(), &SuffixList::built_in())
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
        let s = validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).unwrap();
        assert_eq!(s.media.video.subtitle_languages, ["en", "pt-br"]);
        for bad in [Some(8), Some(10_000)] {
            let mut s = default_settings();
            s.media.audio.audio_kbps = bad;
            assert!(validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).is_err());
        }
        let mut s = default_settings();
        s.media.video.subtitle_languages = vec!["en; rm -rf".into()];
        assert!(validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).is_err());
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

    /// Names open to anyone's registration, and addresses, which suffix
    /// matching makes no sense of (`2.3.4` would match `1.2.3.4`).
    const NOT_ONE_OWNERS_SITE: [&str; 7] = [
        "com",
        "co.uk",
        "github.io",
        "blogspot.com",
        "1.2.3.4",
        "2.3.4",
        "[::1]",
    ];

    #[test]
    fn stored_sites_on_the_network_never_cover_a_whole_domain() {
        let mut settings = default_settings();
        settings.local_sites = NOT_ONE_OWNERS_SITE
            .iter()
            .map(|site| (*site).to_string())
            .chain(["Corp.Example.Com.".into(), "team.github.io".into()])
            .collect();
        assert_eq!(
            within_limits(settings, &SuffixList::built_in()).local_sites,
            ["corp.example.com", "team.github.io"]
        );
    }

    #[test]
    fn a_site_on_the_network_is_one_owners_name() {
        let queues = [default_queue()];
        for site in NOT_ONE_OWNERS_SITE {
            let mut settings = default_settings();
            settings.local_sites = vec![site.into()];
            assert!(
                matches!(
                    validate(settings, &queues, &folders(), &SuffixList::built_in()),
                    Err(SettingsError::Invalid(_))
                ),
                "{site}"
            );
        }
        let mut settings = default_settings();
        settings.local_sites = vec![
            "team.github.io".into(),
            "Bücher.Example".into(),
            "nas.lan".into(),
        ];
        assert_eq!(
            validate(settings, &queues, &folders(), &SuffixList::built_in())
                .unwrap()
                .local_sites,
            ["team.github.io", "xn--bcher-kva.example", "nas.lan"]
        );
    }

    #[test]
    fn sites_on_the_network_are_cleaned_and_need_a_domain() {
        let mut settings = default_settings();
        settings.local_sites = vec![
            " Wiki.Corp.Example.Com ".into(),
            ".corp.example.com.".into(),
            "wiki.corp.example.com/".into(),
            "https://user:secret@Files.Corp.Example.Com:8443/share?x#y".into(),
            "*.lab.example.com".into(),
            "  ".into(),
        ];
        let queues = [default_queue()];
        let clean = validate(
            settings.clone(),
            &queues,
            &folders(),
            &SuffixList::built_in(),
        )
        .unwrap();
        assert_eq!(
            clean.local_sites,
            [
                "wiki.corp.example.com",
                "corp.example.com",
                "files.corp.example.com",
                "lab.example.com"
            ]
        );
        for refused in ["com", "exa mple.com", "https://192.168.1.10/"] {
            settings.local_sites = vec![refused.into()];
            assert!(
                matches!(
                    validate(
                        settings.clone(),
                        &queues,
                        &folders(),
                        &SuffixList::built_in()
                    ),
                    Err(SettingsError::Invalid(_))
                ),
                "{refused}"
            );
        }
    }

    #[test]
    fn validation_cleans_lists() {
        let mut s = default_settings();
        s.extension.capture_extensions = vec![
            ".ZIP".into(),
            "zip".into(),
            " ".into(),
            "Mp4".into(),
            "*.MP4".into(),
            "*.7Z".into(),
        ];
        s.extension.ignored_sites = vec![
            "Example.com/".into(),
            "example.com".into(),
            "https://www.Example.org/page".into(),
            "*.cdn.net:8080".into(),
        ];
        s.categories[0].extensions = vec![".MKV".into()];
        let s = validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).unwrap();
        assert_eq!(s.extension.capture_extensions, vec!["zip", "mp4", "7z"]);
        assert_eq!(
            s.extension.ignored_sites,
            vec!["example.com", "www.example.org", "cdn.net"]
        );
        assert_eq!(s.categories[0].extensions, vec!["mkv"]);
    }

    #[test]
    fn validation_rejects_bad_values_with_a_reason() {
        let queues = [default_queue()];
        let check = |change: fn(&mut AppSettings)| {
            let mut s = default_settings();
            change(&mut s);
            validate(s, &queues, &folders(), &SuffixList::built_in())
                .unwrap_err()
                .to_string()
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
            validate(s, &queues, &folders(), &SuffixList::built_in())
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
    fn validation_names_a_site_that_names_no_host() {
        let mut s = default_settings();
        s.extension.ignored_sites = vec!["example.com".into(), "  ".into(), "not a site".into()];
        let refused = validate(s, &[default_queue()], &folders(), &SuffixList::built_in())
            .unwrap_err()
            .to_string();
        assert!(refused.contains("\"not a site\""), "{refused}");
    }

    /// A stored list is read back without the checks a change goes through, so an entry a change would refuse must not stay in it: every later change of any setting would be refused.
    #[test]
    fn stored_sites_that_could_never_be_saved_are_dropped_on_load() {
        let mut s = default_settings();
        s.extension.ignored_sites = vec!["a".repeat(300), "x.example".into(), "not a site".into()];
        let s = within_limits(s, &SuffixList::built_in());
        assert_eq!(s.extension.ignored_sites, ["x.example"]);
        assert!(validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).is_ok());
    }

    #[test]
    fn duplicates_in_a_full_list_collapse() {
        let mut s = default_settings();
        s.extension.ignored_sites = (0..MAX_IGNORED_SITES)
            .map(|i| format!("S{}.example", i % 10))
            .collect();
        let s = validate(s, &[default_queue()], &folders(), &SuffixList::built_in()).unwrap();
        assert_eq!(s.extension.ignored_sites.len(), 10);
        assert_eq!(s.extension.ignored_sites[0], "s0.example");
    }

    #[test]
    fn queue_validation() {
        let mut q = default_queue().settings;
        q.name = "  Night  ".into();
        assert_eq!(validate_queue(q.clone(), &folders()).unwrap().name, "Night");
        q.name = " ".into();
        assert!(validate_queue(q.clone(), &folders()).is_err());
        q.name = "Night".into();
        q.schedule = Some(Schedule {
            start: TimeOfDay {
                hour: 24,
                minute: 0,
            },
            stop: None,
            days: vec![],
        });
        assert!(
            validate_queue(q, &folders())
                .unwrap_err()
                .to_string()
                .contains("00:00")
        );
    }

    #[test]
    fn an_auto_queue_keeps_its_claims_cleaned_and_refuses_one_that_names_nothing() {
        use wirecatch_protocol::rules::QueueRules;
        let mut q = default_queue().settings;
        q.add_downloads = AddDownloads::Auto(QueueRules {
            enabled: true,
            sites: vec![" example.com ".into(), String::new()],
            ..QueueRules::default()
        });
        let AddDownloads::Auto(kept) = validate_queue(q.clone(), &folders()).unwrap().add_downloads
        else {
            panic!("the queue stopped being an auto queue");
        };
        assert_eq!(kept.sites, ["example.com"]);
        q.add_downloads = AddDownloads::Auto(QueueRules {
            sites: vec!["*".into()],
            ..QueueRules::default()
        });
        assert!(
            validate_queue(q, &folders())
                .unwrap_err()
                .to_string()
                .contains("\"*\"")
        );
    }

    #[test]
    fn category_and_folder_come_from_the_rules() {
        let s = default_settings();
        let f = folders();
        assert_eq!(category_of(&s, "Movie.MKV"), Category::Video);
        assert_eq!(category_of(&s, "backup.tar.gz"), Category::Archive);
        assert_eq!(category_of(&s, "README"), Category::Other);
        let mut queue = default_queue().settings;
        assert_eq!(
            folder_for(&s, &queue, Category::Video, &f).unwrap(),
            f.downloads.join("Video")
        );
        assert_eq!(
            folder_for(&s, &queue, Category::Other, &f).unwrap(),
            f.downloads
        );
        queue.folder = Setting::Own(PortablePath("{Downloads}/Night".into()));
        assert_eq!(
            folder_for(&s, &queue, Category::Video, &f).unwrap(),
            f.downloads.join("Night")
        );
    }

    #[test]
    fn a_queue_folder_is_checked_like_the_others() {
        let mut q = default_queue().settings;
        q.folder = Setting::Own(PortablePath("relative/folder".into()));
        assert!(validate_queue(q.clone(), &folders()).is_err());
        q.folder = Setting::Own(PortablePath(format!(
            "{{Downloads}}/{}",
            "a".repeat(MAX_FOLDER_CHARS)
        )));
        assert!(validate_queue(q, &folders()).is_err());
    }
}
