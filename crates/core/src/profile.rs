//! Settings export and import (D6): one versioned JSON file with portable paths.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use wirecatch_engine::numeric::saturating_cast;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AppSettings, Category, ImportSummary, ImportedFolder, PROFILE_FORMAT_VERSION, PortablePath,
    ProfileQueue, Queue, QueueStatus, SettingsProfile,
};

use crate::error::{CoreError, Result};
use crate::folders::KnownFolders;
use crate::settings::{self, DEFAULT_QUEUE_ID};
use wirecatch_engine::units::MIB;
use wirecatch_sites::public_suffix::SuffixList;

/// Largest profile file accepted. Real ones are a few kilobytes; the limits on
/// queues and lists keep even a full one far below this.
const MAX_PROFILE_BYTES: u64 = MIB;

/// A profile of the current settings and queues, with every folder written
/// relative to the user's standard folders where possible.
pub(crate) fn export(
    settings: &AppSettings,
    queues: &[Queue],
    folders: &KnownFolders,
    exported_at: i64,
) -> SettingsProfile {
    let mut settings = settings.clone();
    // Passwords stay on this computer (A6), so the profile has none to speak of.
    crate::proxy::mark_passwords(&mut settings.proxy, &crate::proxy::Logins::new());
    settings.default_folder = folders.to_portable(&settings.default_folder);
    for rule in &mut settings.categories {
        rule.folder = folders.to_portable(&rule.folder);
    }
    SettingsProfile {
        format_version: PROFILE_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at,
        settings,
        queues: queues
            .iter()
            .map(|q| ProfileQueue {
                id: q.id,
                is_default: q.is_default,
                settings: q.settings.clone(),
            })
            .collect(),
        addons: Vec::new(),
    }
}

/// Checks the format version first, so another version's file gets its own error.
pub(crate) fn parse(bytes: &[u8]) -> Result<SettingsProfile> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| CoreError::InvalidProfile(e.to_string()))?;
    let version = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| CoreError::InvalidProfile("it has no format version".into()))?;
    if version > u64::from(PROFILE_FORMAT_VERSION) {
        return Err(CoreError::ProfileTooNew {
            found: version,
            supported: PROFILE_FORMAT_VERSION,
        });
    }
    if version < u64::from(PROFILE_FORMAT_VERSION) {
        return Err(CoreError::ProfileTooOld { found: version });
    }
    serde_json::from_value(value).map_err(|e| CoreError::InvalidProfile(e.to_string()))
}

/// Settings and queues after applying an imported profile, and what changed.
#[derive(Debug)]
pub(crate) struct Imported {
    pub settings: AppSettings,
    pub queues: Vec<Queue>,
    pub summary: ImportSummary,
    /// Names of the queues added and of those updated, for a preview.
    pub added: Vec<String>,
    pub updated: Vec<String>,
}

/// A profile whose folders were resolved on this machine, with the report of
/// where each one ended up. Resolving looks at the disk (to refuse startup and
/// system folders whatever they are called), so it is done off the async
/// threads, before the merge.
#[derive(Debug)]
pub(crate) struct Localized {
    pub profile: SettingsProfile,
    pub folders: Vec<ImportedFolder>,
}

/// Resolves the profile's folders on this machine; see [`Localized`]. Blocks on
/// the disk.
pub(crate) fn localize(mut profile: SettingsProfile, folders: &KnownFolders) -> Localized {
    let report = localize_folders(&mut profile.settings, folders);
    Localized {
        profile,
        folders: report,
    }
}

/// Reads a profile file, refusing one larger than any real profile could be
/// before reading it whole. Blocks on the disk.
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file_error = |source| CoreError::File {
        path: path.display().to_string(),
        source,
    };
    let file = std::fs::File::open(path).map_err(file_error)?;
    let mut bytes = Vec::new();
    file.take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err(CoreError::InvalidProfile(format!(
            "it is larger than {} MB",
            MAX_PROFILE_BYTES / MIB
        )));
    }
    Ok(bytes)
}

/// [`localize`] then [`merge_localized`], for tests.
#[cfg(test)]
pub(crate) fn merge(
    profile: SettingsProfile,
    current: &[Queue],
    folders: &KnownFolders,
    next_queue_id: impl FnMut() -> QueueId,
) -> Result<Imported> {
    merge_localized(
        localize(profile, folders),
        current,
        folders,
        &SuffixList::built_in(),
        next_queue_id,
    )
}

/// Merges a profile whose folders were resolved here into the current queues
/// and checks the result.
///
/// Queue ids from another machine mean nothing here, so queues are matched by
/// name: the profile's default queue updates this default queue, a queue with a
/// known name updates that queue, and the rest are added. Queues missing from the
/// profile are kept, because downloads may still be in them. A folder that does not
/// exist on this machine (an absolute path from another system), or that is a
/// startup or system folder, falls back to the default folder.
pub(crate) fn merge_localized(
    localized: Localized,
    current: &[Queue],
    folders: &KnownFolders,
    suffixes: &SuffixList,
    mut next_queue_id: impl FnMut() -> QueueId,
) -> Result<Imported> {
    let Localized {
        profile,
        folders: folder_report,
    } = localized;
    // Checked before the merge so a huge file costs nothing.
    settings::check_queue_count(profile.queues.len())?;
    let mut queues = current.to_vec();
    let mut ids = HashMap::new();
    let (mut added, mut updated) = (Vec::new(), Vec::new());
    for incoming in profile.queues {
        let settings = settings::validate_queue(incoming.settings)?;
        let target = queues.iter_mut().find(|q| {
            q.is_default == incoming.is_default
                && (incoming.is_default || q.settings.name.eq_ignore_ascii_case(&settings.name))
        });
        let local_id = if let Some(existing) = target {
            updated.push(existing.settings.name.clone());
            existing.settings = settings;
            existing.id
        } else {
            added.push(settings.name.clone());
            let id = next_queue_id();
            queues.push(Queue {
                id,
                is_default: false,
                settings,
                status: QueueStatus::default(),
            });
            id
        };
        ids.insert(incoming.id, local_id);
    }

    let mut settings = profile.settings;
    // A file edited by hand may hold sites no host matches.
    settings.extension.ignored_sites = settings::stored_sites(settings.extension.ignored_sites);
    settings.extension.default_queue = ids
        .get(&settings.extension.default_queue)
        .copied()
        .unwrap_or(DEFAULT_QUEUE_ID);
    settings::check_queue_count(queues.len())?;
    let settings = settings::validate(settings, &queues, folders, suffixes)?;
    let count = |names: &[String]| saturating_cast::<u32, _>(names.len());
    Ok(Imported {
        settings,
        queues,
        summary: ImportSummary {
            folders: folder_report,
            queues_added: count(&added),
            queues_updated: count(&updated),
            addons_to_offer: Vec::new(),
        },
        added,
        updated,
    })
}

/// Replaces folders that do not exist on this machine, or that are startup or
/// system folders, with a fallback, and reports every folder setting with where
/// it ended up and why. Blocks on the disk.
fn localize_folders(settings: &mut AppSettings, folders: &KnownFolders) -> Vec<ImportedFolder> {
    let mut report = Vec::with_capacity(settings.categories.len() + 1);
    let mut place = |setting: String, folder: &mut PortablePath, fallback: &PortablePath| {
        let requested = folder.clone();
        // A file from elsewhere may not point downloads at a network folder, or
        // at a folder where a downloaded file could run by itself.
        let (resolved, reason) = match folders.resolve_local(folder) {
            Ok(path) => (path, None),
            Err(e) => {
                *folder = fallback.clone();
                (
                    folders.resolve(folder).unwrap_or_default(),
                    Some(e.to_string()),
                )
            }
        };
        report.push(ImportedFolder {
            setting,
            requested,
            used: folder.clone(),
            outside_user_folders: folders.is_outside_user_folders(&resolved),
            resolved: resolved.to_string_lossy().into_owned(),
            reason,
        });
    };
    let downloads = PortablePath("{Downloads}".into());
    place(
        "Default folder".into(),
        &mut settings.default_folder,
        &downloads,
    );
    let default_folder = settings.default_folder.clone();
    for rule in &mut settings.categories {
        place(
            category_folder_name(rule.category).into(),
            &mut rule.folder,
            &default_folder,
        );
    }
    report
}

fn category_folder_name(category: Category) -> &'static str {
    match category {
        Category::Video => "Video folder",
        Category::Audio => "Music folder",
        Category::Archive => "Compressed files folder",
        Category::Document => "Documents folder",
        Category::Program => "Programs folder",
        Category::Image => "Images folder",
        Category::Other => "Other files folder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{default_queue, default_settings};
    use wirecatch_protocol::app::{MediaDefaults, QueueNotify, QueueSettings};

    fn folders() -> KnownFolders {
        KnownFolders::under_home(std::env::temp_dir().join("wc-profile-home"))
    }

    fn queue(id: i64, name: &str) -> Queue {
        Queue {
            id: QueueId(id),
            is_default: false,
            settings: QueueSettings {
                name: name.into(),
                max_concurrent: 1,
                speed_limit: None,
                schedule: None,
                notify: QueueNotify::EachDownload,
            },
            status: QueueStatus::default(),
        }
    }

    fn in_profile(queue: Queue) -> ProfileQueue {
        ProfileQueue {
            id: queue.id,
            is_default: queue.is_default,
            settings: queue.settings,
        }
    }

    #[test]
    fn newer_profiles_are_refused_before_parsing() {
        let next = PROFILE_FORMAT_VERSION + 1;
        let future = format!(r#"{{"format_version": {next}, "something_new": true}}"#);
        let err = parse(future.as_bytes()).unwrap_err();
        assert!(matches!(
            err,
            CoreError::ProfileTooNew {
                found,
                supported: PROFILE_FORMAT_VERSION
            } if found == u64::from(next)
        ));
        assert!(err.to_string().contains("Update Wirecatch"));
        // A version past what 32 bits hold is reported as written.
        let far = br#"{"format_version": 4294967296}"#;
        let err = parse(far).unwrap_err();
        assert!(err.to_string().contains("format 4294967296,"), "{err}");
    }

    #[test]
    fn junk_is_not_a_profile() {
        assert!(matches!(
            parse(b"not json"),
            Err(CoreError::InvalidProfile(_))
        ));
        assert!(matches!(parse(b"{}"), Err(CoreError::InvalidProfile(_))));
        assert!(matches!(
            parse(format!(r#"{{"format_version": {PROFILE_FORMAT_VERSION}}}"#).as_bytes()),
            Err(CoreError::InvalidProfile(_))
        ));
    }

    #[test]
    fn export_then_parse_round_trips() {
        let f = folders();
        let mut s = default_settings();
        s.default_folder = PortablePath(f.downloads.join("Stuff").to_string_lossy().into_owned());
        let profile = export(&s, &[default_queue()], &f, 42);
        assert_eq!(
            profile.settings.default_folder,
            PortablePath("{Downloads}/Stuff".into())
        );
        let bytes = serde_json::to_vec(&profile).unwrap();
        assert_eq!(parse(&bytes).unwrap(), profile);
    }

    #[test]
    fn media_defaults_are_in_the_profile() {
        let f = folders();
        let mut s = default_settings();
        s.media.video.format = wirecatch_protocol::app::OutputFormat::Mkv;
        s.media.video.subtitle_languages = vec!["en".into()];
        s.media.audio.format = wirecatch_protocol::app::OutputFormat::Mp3;
        s.media.audio.audio_kbps = Some(256);
        let profile = export(&s, &[default_queue()], &f, 42);
        let bytes = serde_json::to_vec(&profile).unwrap();
        assert_eq!(parse(&bytes).unwrap().settings.media, s.media);

        let mut old = serde_json::to_value(&profile).unwrap();
        old["settings"].as_object_mut().unwrap().remove("media");
        let read = parse(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(read.format_version, PROFILE_FORMAT_VERSION);
        assert_eq!(read.settings.media, MediaDefaults::default());
    }

    #[test]
    fn a_profile_of_an_earlier_format_is_refused() {
        let mut old = serde_json::to_value(export(
            &default_settings(),
            &[default_queue()],
            &folders(),
            42,
        ))
        .unwrap();
        old["format_version"] = (PROFILE_FORMAT_VERSION - 1).into();
        let err = parse(&serde_json::to_vec(&old).unwrap()).unwrap_err();
        assert!(
            matches!(err, CoreError::ProfileTooOld { found } if found == u64::from(PROFILE_FORMAT_VERSION - 1))
        );
        assert!(
            err.to_string().contains("Export the settings again"),
            "{err}"
        );
    }

    #[test]
    fn a_profile_carries_queue_settings_without_their_status() {
        let mut paused = default_queue();
        paused.status.paused = true;
        let profile =
            serde_json::to_value(export(&default_settings(), &[paused], &folders(), 42)).unwrap();
        let queue = profile["queues"][0].as_object().unwrap();
        let mut keys: Vec<_> = queue.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["id", "is_default", "settings"]);
    }

    #[test]
    fn queues_merge_by_name_and_remap_the_extension_queue() {
        let current = vec![default_queue(), queue(2, "Night")];
        let mut incoming_default = default_queue();
        incoming_default.id = QueueId(10);
        incoming_default.settings.max_concurrent = 7;
        let mut night = queue(11, "night");
        night.settings.max_concurrent = 4;
        let mut settings = default_settings();
        settings.extension.default_queue = QueueId(12);
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: [incoming_default, night, queue(12, "Videos")]
                .into_iter()
                .map(in_profile)
                .collect(),
            addons: vec![],
        };
        let mut next = 3;
        let imported = merge(profile, &current, &folders(), || {
            next += 1;
            QueueId(next - 1)
        })
        .unwrap();

        let q = &imported.queues;
        assert_eq!(q.len(), 3);
        assert_eq!(
            (q[0].id, q[0].settings.max_concurrent, q[0].is_default),
            (QueueId(1), 7, true)
        );
        assert_eq!((q[1].id, q[1].settings.max_concurrent), (QueueId(2), 4));
        assert_eq!(
            (q[2].id, q[2].settings.name.as_str(), q[2].is_default),
            (QueueId(3), "Videos", false)
        );
        assert_eq!(imported.settings.extension.default_queue, QueueId(3));
    }

    #[test]
    fn an_oversized_file_is_refused_before_it_is_read_whole() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        std::fs::write(
            &big,
            vec![b' '; usize::try_from(MAX_PROFILE_BYTES).unwrap() + 1],
        )
        .unwrap();
        let err = read_file(&big).unwrap_err();
        assert!(err.to_string().contains("larger than 1 MB"), "{err}");
        let small = dir.path().join("small.json");
        std::fs::write(&small, b"{}").unwrap();
        assert_eq!(read_file(&small).unwrap(), b"{}");
    }

    #[test]
    fn a_profile_with_too_many_queues_is_refused() {
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings: default_settings(),
            queues: (0..=i64::try_from(settings::MAX_QUEUES).unwrap())
                .map(|i| in_profile(queue(i + 10, &format!("Q{i}"))))
                .collect(),
            addons: vec![],
        };
        let err = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap_err();
        assert!(err.to_string().contains("at most 100 queues"), "{err}");
    }

    #[test]
    fn a_profile_imports_without_its_sites_that_name_no_host() {
        let mut settings = default_settings();
        settings.extension.ignored_sites = vec![
            "Example.com".into(),
            "my intranet".into(),
            "localhost:abc".into(),
        ];
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: vec![],
            addons: vec![],
        };
        let imported = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap();
        assert_eq!(imported.settings.extension.ignored_sites, ["example.com"]);
    }

    /// A settings file may come from anywhere: it must not point downloads at a
    /// device, or quietly at a network share. Those fall back like a missing
    /// folder, and the import summary says so.
    #[test]
    fn device_and_network_folders_from_a_file_fall_back() {
        let mut settings = default_settings();
        settings.default_folder = PortablePath(r"\\server\share\Downloads".into());
        let special = [
            r"\\?\UNC\server\share\Video",
            r"\\.\C:\Music",
            r"\\?\C:\Compressed",
            r"\\.\PhysicalDrive0",
        ];
        for (rule, folder) in settings.categories.iter_mut().zip(special) {
            rule.folder = PortablePath(folder.into());
        }
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: vec![],
            addons: vec![],
        };
        let imported = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap();
        let downloads = PortablePath("{Downloads}".into());
        assert_eq!(imported.settings.default_folder, downloads);
        for rule in &imported.settings.categories[..special.len()] {
            assert_eq!(rule.folder, downloads, "{:?}", rule.category);
        }
        let report = &imported.summary.folders;
        assert_eq!(
            report[0].requested,
            PortablePath(r"\\server\share\Downloads".into())
        );
        assert_eq!(report[0].used, downloads);
        for (entry, folder) in report[1..].iter().zip(special) {
            assert_eq!(entry.requested, PortablePath(folder.into()));
            assert_eq!(entry.used, downloads);
        }
    }

    #[test]
    fn foreign_absolute_folders_fall_back_to_downloads() {
        let foreign = if cfg!(windows) {
            "/home/ada/Films"
        } else {
            r"D:\Films"
        };
        let mut settings = default_settings();
        settings.default_folder = PortablePath(foreign.into());
        settings.categories[0].folder = PortablePath(foreign.into());
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: vec![],
            addons: vec![],
        };
        let imported = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap();
        assert_eq!(
            imported.settings.default_folder,
            PortablePath("{Downloads}".into())
        );
        let first = &imported.summary.folders[0];
        assert_eq!(first.setting, "Default folder");
        assert_eq!(first.requested, PortablePath(foreign.into()));
        assert_eq!(first.used, PortablePath("{Downloads}".into()));
        assert_eq!(
            first.resolved,
            folders().downloads.to_string_lossy().into_owned()
        );
        assert_eq!(imported.summary.folders[1].setting, "Video folder");
        assert_eq!(
            imported.settings.categories[0].folder,
            PortablePath("{Downloads}".into())
        );
    }

    /// A settings file may not send downloads into a startup or system folder,
    /// where a file could run by itself: such a folder falls back like a
    /// missing one, and the report says why. A folder outside the person's own
    /// folders is used but flagged.
    #[test]
    fn startup_folders_from_a_file_fall_back_and_outside_folders_are_flagged() {
        let startup = if cfg!(windows) {
            "{Home}/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup"
        } else {
            "{Home}/.config/autostart"
        };
        let system = if cfg!(windows) { r"C:\Windows" } else { "/etc" };
        let outside = std::env::temp_dir().join("wc-profile-elsewhere");
        let mut settings = default_settings();
        settings.default_folder = PortablePath(startup.into());
        settings.categories[0].folder = PortablePath(system.into());
        settings.categories[1].folder = PortablePath(outside.to_string_lossy().into_owned());
        let profile = SettingsProfile {
            format_version: PROFILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: vec![],
            addons: vec![],
        };
        let imported = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap();
        let downloads = PortablePath("{Downloads}".into());
        assert_eq!(imported.settings.default_folder, downloads);
        assert_eq!(imported.settings.categories[0].folder, downloads);
        let report = &imported.summary.folders;
        for entry in &report[..2] {
            assert_eq!(entry.used, downloads);
            let reason = entry.reason.as_deref().unwrap_or_default();
            assert!(reason.contains("system or startup folder"), "{reason}");
            assert!(!entry.outside_user_folders);
        }
        let flagged = &report[2];
        assert_eq!(flagged.used, flagged.requested);
        assert_eq!(flagged.reason, None);
        assert!(flagged.outside_user_folders);
    }
}
