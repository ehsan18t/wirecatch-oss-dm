//! Settings export and import (D6): one versioned JSON file with portable paths.

use std::collections::HashMap;

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AppSettings, PortablePath, Queue, SettingsProfile};

use crate::error::{CoreError, Result};
use crate::folders::KnownFolders;
use crate::settings::{self, DEFAULT_QUEUE_ID};

/// Version of the profile format this build writes and the newest it reads.
pub const PROFILE_FORMAT_VERSION: u32 = 1;

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

/// Settings and queues after applying an imported profile.
#[derive(Debug)]
pub(crate) struct Imported {
    pub settings: AppSettings,
    pub queues: Vec<Queue>,
}

/// Merges `profile` into the current queues and checks the result.
///
/// Queue ids from another machine mean nothing here, so queues are matched by
/// name: the profile's default queue updates this default queue, a queue with a
/// known name updates that queue, and the rest are added. Queues missing from the
/// profile are kept, because downloads may still be in them. A folder that does not
/// exist on this machine (an absolute path from another system) falls back to the
/// default folder.
pub(crate) fn merge(
    profile: SettingsProfile,
    current: &[Queue],
    folders: &KnownFolders,
    mut next_queue_id: impl FnMut() -> QueueId,
) -> Result<Imported> {
    // Checked before the merge so a huge file costs nothing.
    settings::check_queue_count(profile.queues.len())?;
    let mut queues = current.to_vec();
    let mut ids = HashMap::new();
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
                *existing = Queue {
                    id: existing.id,
                    is_default: existing.is_default,
                    ..incoming.clone()
                };
                existing.id
            }
            None => {
                let id = next_queue_id();
                queues.push(Queue {
                    id,
                    is_default: false,
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
    localize_folders(&mut settings, folders);
    settings::check_queue_count(queues.len())?;
    let settings = settings::validate(settings, &queues, folders)?;
    Ok(Imported { settings, queues })
}

fn localize_folders(settings: &mut AppSettings, folders: &KnownFolders) {
    let fallback = PortablePath("{Downloads}".into());
    if folders.resolve(&settings.default_folder).is_err() {
        settings.default_folder = fallback;
    }
    for rule in &mut settings.categories {
        if folders.resolve(&rule.folder).is_err() {
            rule.folder = settings.default_folder.clone();
        }
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
    fn a_profile_with_too_many_queues_is_refused() {
        let profile = SettingsProfile {
            format_version: 1,
            app_version: "0.1.0".into(),
            exported_at: 0,
            settings: default_settings(),
            queues: (0..=settings::MAX_QUEUES as i64)
                .map(|i| queue(i + 10, &format!("Q{i}")))
                .collect(),
            addons: vec![],
        };
        let err = merge(profile, &[default_queue()], &folders(), || QueueId(2)).unwrap_err();
        assert!(err.to_string().contains("at most 100 queues"), "{err}");
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
        assert_eq!(
            imported.settings.categories[0].folder,
            PortablePath("{Downloads}".into())
        );
    }
}
