//! Settings export and import (D6): one versioned JSON file with portable paths.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    AppSettings, Category, ImportSummary, ImportedFolder, PROFILE_FORMAT_VERSION, PortablePath,
    Queue, SettingsProfile,
};

use crate::error::{CoreError, Result};
use crate::folders::KnownFolders;
use crate::settings::{self, DEFAULT_QUEUE_ID};

/// Largest profile file accepted. Real ones are a few kilobytes; the limits on
/// queues and lists keep even a full one far below this.
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// A profile of the current settings and queues, with every folder written
/// relative to the user's standard folders where possible.
pub(crate) fn export(
    settings: &AppSettings,
    queues: &[Queue],
    folders: &KnownFolders,
    exported_at: i64,
) -> SettingsProfile {
    let mut settings = settings.clone();
    settings.default_folder = folders.to_portable(&settings.default_folder);
    for rule in &mut settings.categories {
        rule.folder = folders.to_portable(&rule.folder);
    }
    SettingsProfile {
        format_version: PROFILE_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at,
        settings,
        queues: queues.to_vec(),
        addons: Vec::new(),
    }
}

/// Reads a profile file. The version is checked before anything else, so a file
/// from a newer Wirecatch gets a clear message instead of a parse error.
pub(crate) fn parse(bytes: &[u8]) -> Result<SettingsProfile> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| CoreError::InvalidProfile(e.to_string()))?;
    let version = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| CoreError::InvalidProfile("it has no format version".into()))?;
    let found = u32::try_from(version).unwrap_or(u32::MAX);
    if found > PROFILE_FORMAT_VERSION {
        return Err(CoreError::ProfileTooNew {
            found,
            supported: PROFILE_FORMAT_VERSION,
        });
    }
    if found == 0 {
        return Err(CoreError::InvalidProfile("its format version is 0".into()));
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

/// Resolves `profile`'s folders on this machine; see [`Localized`]. Blocks on
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
            MAX_PROFILE_BYTES / (1024 * 1024)
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
    merge_localized(localize(profile, folders), current, folders, next_queue_id)
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
        let incoming = settings::validate_queue(incoming)?;
        let target = if incoming.is_default {
            queues.iter().position(|q| q.is_default)
        } else {
            queues
                .iter()
                .position(|q| !q.is_default && q.name.eq_ignore_ascii_case(&incoming.name))
        };
        let local_id = match target {
            Some(i) => {
                let existing = &mut queues[i];
                updated.push(existing.name.clone());
                *existing = Queue {
                    id: existing.id,
                    is_default: existing.is_default,
                    paused: existing.paused,
                    running_outside_schedule: existing.running_outside_schedule,
                    ..incoming.clone()
                };
                existing.id
            }
            None => {
                added.push(incoming.name.clone());
                let id = next_queue_id();
                queues.push(Queue {
                    id,
                    is_default: false,
                    paused: false,
                    running_outside_schedule: false,
                    ..incoming.clone()
                });
                id
            }
        };
        ids.insert(incoming.id, local_id);
    }

    let mut settings = profile.settings;
    settings.extension.default_queue = ids
        .get(&settings.extension.default_queue)
        .copied()
        .unwrap_or(DEFAULT_QUEUE_ID);
    settings::check_queue_count(queues.len())?;
    let settings = settings::validate(settings, &queues, folders)?;
    let count = |names: &[String]| u32::try_from(names.len()).unwrap_or(u32::MAX);
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

    fn folders() -> KnownFolders {
        KnownFolders::under_home(std::env::temp_dir().join("wc-profile-home"))
    }

    fn queue(id: i64, name: &str) -> Queue {
        Queue {
            id: QueueId(id),
            name: name.into(),
            max_concurrent: 1,
            speed_limit: None,
            schedule: None,
            is_default: false,
            paused: false,
            running_outside_schedule: false,
        }
    }

    #[test]
    fn newer_profiles_are_refused_before_parsing() {
        let future = br#"{"format_version": 2, "something_new": true}"#;
        let err = parse(future).unwrap_err();
        assert!(matches!(
            err,
            CoreError::ProfileTooNew {
                found: 2,
                supported: 1
            }
        ));
        assert!(err.to_string().contains("Update Wirecatch"));
    }

    #[test]
    fn junk_is_not_a_profile() {
        assert!(matches!(
            parse(b"not json"),
            Err(CoreError::InvalidProfile(_))
        ));
        assert!(matches!(parse(b"{}"), Err(CoreError::InvalidProfile(_))));
        assert!(matches!(
            parse(br#"{"format_version": 1}"#),
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

    /// The media defaults travel with the profile (D24), and a profile written
    /// before they existed reads with the original format, without a new
    /// format version.
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
        assert_eq!(read.format_version, 1);
        assert_eq!(read.settings.media, Default::default());
    }

    #[test]
    fn queues_merge_by_name_and_remap_the_extension_queue() {
        let current = vec![default_queue(), queue(2, "Night")];
        let mut incoming_default = default_queue();
        incoming_default.id = QueueId(10);
        incoming_default.max_concurrent = 7;
        let mut night = queue(11, "night");
        night.max_concurrent = 4;
        let mut settings = default_settings();
        settings.extension.default_queue = QueueId(12);
        let profile = SettingsProfile {
            format_version: 1,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings,
            queues: vec![incoming_default, night, queue(12, "Videos")],
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
            (q[0].id, q[0].max_concurrent, q[0].is_default),
            (QueueId(1), 7, true)
        );
        assert_eq!((q[1].id, q[1].max_concurrent), (QueueId(2), 4));
        assert_eq!(
            (q[2].id, q[2].name.as_str(), q[2].is_default),
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
            format_version: 1,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings: default_settings(),
            queues: (0..=i64::try_from(settings::MAX_QUEUES).unwrap())
                .map(|i| queue(i + 10, &format!("Q{i}")))
                .collect(),
            addons: vec![],
        };
        let err = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap_err();
        assert!(err.to_string().contains("at most 100 queues"), "{err}");
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
            format_version: 1,
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
            format_version: 1,
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
            format_version: 1,
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
