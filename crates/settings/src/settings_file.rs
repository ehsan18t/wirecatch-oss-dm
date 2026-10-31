//! Settings export and import (D6): one versioned JSON file with portable paths.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use wirecatch_engine::numeric::saturating_cast;
use wirecatch_files::FileError;
use wirecatch_protocol::app::{
    AppSettings, Category, ImportSummary, ImportedFolder, PortablePath, Queue, QueueStatus,
    SETTINGS_FILE_FORMAT_VERSION, Setting, SettingsFile, SettingsFileProxy, SettingsFileQueue,
};
use wirecatch_protocol::proxy::ProxyUse;
use wirecatch_protocol::{ProxyServerId, QueueId, UnixMillis};

use crate::DEFAULT_QUEUE_ID;
use crate::error::{Result, SettingsFileError};
use crate::folders::KnownFolders;
use crate::proxy::SavedProxyServer;
use wirecatch_engine::units::MIB;
use wirecatch_sites::public_suffix::SuffixList;

/// Largest profile file accepted. Real ones are a few kilobytes; the limits on
/// queues and lists keep even a full one far below this.
const MAX_SETTINGS_FILE_BYTES: u64 = MIB;

/// A profile of the current settings, queues and proxy servers, with every folder written
/// relative to the user's standard folders where possible. Proxy passwords stay on this
/// computer (A6).
#[must_use]
pub fn export(
    settings: &AppSettings,
    queues: &[Queue],
    proxies: &[SavedProxyServer],
    folders: &KnownFolders,
    exported_at: UnixMillis,
) -> SettingsFile {
    let mut settings = settings.clone();
    settings.default_folder = folders.to_portable(&settings.default_folder);
    for rule in &mut settings.categories {
        rule.folder = folders.to_portable(&rule.folder);
    }
    SettingsFile {
        format_version: SETTINGS_FILE_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at,
        settings,
        queues: queues
            .iter()
            .map(|q| {
                let mut settings = q.settings.clone();
                if let Setting::Own(folder) = &mut settings.folder {
                    *folder = folders.to_portable(folder);
                }
                SettingsFileQueue {
                    id: q.id,
                    is_default: q.is_default,
                    settings,
                }
            })
            .collect(),
        proxy_servers: proxies
            .iter()
            .map(|p| SettingsFileProxy {
                id: p.id,
                settings: p.settings.clone(),
            })
            .collect(),
        addons: Vec::new(),
    }
}

/// Checks the format version first, so another version's file gets its own error.
pub fn parse(bytes: &[u8]) -> Result<SettingsFile, SettingsFileError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| SettingsFileError::Invalid(e.to_string()))?;
    let version = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| SettingsFileError::Invalid("it has no format version".into()))?;
    if version > u64::from(SETTINGS_FILE_FORMAT_VERSION) {
        return Err(SettingsFileError::TooNew {
            found: version,
            supported: SETTINGS_FILE_FORMAT_VERSION,
        });
    }
    if version < u64::from(SETTINGS_FILE_FORMAT_VERSION) {
        return Err(SettingsFileError::TooOld { found: version });
    }
    serde_json::from_value(value).map_err(|e| SettingsFileError::Invalid(e.to_string()))
}

/// Settings, queues and proxy servers after applying an imported profile, and what changed.
#[derive(Debug)]
pub struct Imported {
    pub settings: AppSettings,
    pub queues: Vec<Queue>,
    /// Every proxy server, those already saved first; an updated one keeps its last test.
    pub proxies: Vec<SavedProxyServer>,
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
pub struct Localized {
    pub profile: SettingsFile,
    pub folders: Vec<ImportedFolder>,
}

/// Resolves the profile's folders on this machine; see [`Localized`]. Blocks on
/// the disk.
#[must_use]
pub fn localize(mut profile: SettingsFile, folders: &KnownFolders) -> Localized {
    let report = localize_folders(&mut profile, folders);
    Localized {
        profile,
        folders: report,
    }
}

/// Reads a profile file, refusing one larger than any real profile could be
/// before reading it whole. Blocks on the disk.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file_error = |source| FileError::new(path, source);
    let file = std::fs::File::open(path).map_err(file_error)?;
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() as u64 > MAX_SETTINGS_FILE_BYTES {
        return Err(SettingsFileError::Invalid(format!(
            "it is larger than {} MB",
            MAX_SETTINGS_FILE_BYTES / MIB
        ))
        .into());
    }
    Ok(bytes)
}

/// [`localize`] then [`merge_localized`], for tests.
#[cfg(test)]
pub(crate) fn merge(
    profile: SettingsFile,
    current: &[Queue],
    folders: &KnownFolders,
    next_queue_id: impl FnMut() -> QueueId,
) -> Result<Imported> {
    let mut next_proxy = 0;
    merge_localized(
        localize(profile, folders),
        Current {
            queues: current,
            proxies: &[],
        },
        folders,
        &SuffixList::built_in(),
        NewIds {
            queue: next_queue_id,
            proxy: || {
                next_proxy += 1;
                ProxyServerId(next_proxy)
            },
        },
    )
}

/// The queues and proxy servers a profile merges into.
#[derive(Debug, Clone, Copy)]
pub struct Current<'a> {
    pub queues: &'a [Queue],
    pub proxies: &'a [SavedProxyServer],
}

/// Hands out the ids of the queues and proxy servers a profile adds.
pub struct NewIds<Q, P> {
    pub queue: Q,
    pub proxy: P,
}

/// Merges a profile whose folders were resolved here into the current queues
/// and checks the result.
///
/// Queue ids from another machine mean nothing here, so queues are matched by
/// name: the profile's default queue updates this default queue, a queue with a
/// known name updates that queue, and the rest are added. Queues missing from the
/// profile are kept, because downloads may still be in them. Proxy servers are matched by
/// name the same way and never removed. A folder that does not
/// exist on this machine (an absolute path from another system), or that is a
/// startup or system folder, falls back to the default folder.
pub fn merge_localized(
    localized: Localized,
    current: Current<'_>,
    folders: &KnownFolders,
    suffixes: &SuffixList,
    mut new_ids: NewIds<impl FnMut() -> QueueId, impl FnMut() -> ProxyServerId>,
) -> Result<Imported> {
    let Localized {
        profile,
        folders: folder_report,
    } = localized;
    // Checked before the merge so a huge file costs nothing.
    crate::check_queue_count(profile.queues.len())?;
    crate::proxy::check_count(profile.proxy_servers.len())?;
    let (proxies, proxy_ids) =
        merge_proxies(profile.proxy_servers, current.proxies, &mut new_ids.proxy)?;
    let local_proxy = |proxy: ProxyUse| match proxy {
        ProxyUse::Server(id) => proxy_ids.get(&id).copied().map(ProxyUse::Server),
        other => Some(other),
    };
    let mut queues = current.queues.to_vec();
    let mut ids = HashMap::new();
    let (mut added, mut updated) = (Vec::new(), Vec::new());
    for incoming in profile.queues {
        let mut settings = crate::validate_queue(incoming.settings, folders)?;
        if let Setting::Own(proxy) = settings.proxy {
            settings.proxy = local_proxy(proxy).map_or(Setting::Global, Setting::Own);
        }
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
            let id = (new_ids.queue)();
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
    settings.proxy.proxy = local_proxy(settings.proxy.proxy).unwrap_or_default();
    // A file edited by hand may hold sites no host matches.
    settings.extension.ignored_sites = crate::stored_sites(settings.extension.ignored_sites);
    settings.extension.default_queue = ids
        .get(&settings.extension.default_queue)
        .copied()
        .unwrap_or(DEFAULT_QUEUE_ID);
    crate::check_queue_count(queues.len())?;
    let settings = crate::validate(settings, &queues, folders, suffixes)?;
    let count = |names: &[String]| saturating_cast::<u32, _>(names.len());
    Ok(Imported {
        settings,
        queues,
        proxies,
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

/// The proxy servers after adding or updating `incoming` by name, and the local id of each
/// incoming one by its id in the file.
fn merge_proxies(
    incoming: Vec<SettingsFileProxy>,
    current: &[SavedProxyServer],
    next_id: &mut impl FnMut() -> ProxyServerId,
) -> Result<(Vec<SavedProxyServer>, HashMap<ProxyServerId, ProxyServerId>)> {
    let mut proxies = current.to_vec();
    let mut ids = HashMap::with_capacity(incoming.len());
    for profile in incoming {
        let shown = profile.settings.shown_name();
        let target = proxies
            .iter()
            .position(|p| p.settings.shown_name().eq_ignore_ascii_case(&shown));
        let others: Vec<_> = proxies
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != target)
            .map(|(_, p)| &p.settings)
            .collect();
        let settings = crate::proxy::validate_settings_file(profile.settings, &others)?;
        let local_id = if let Some(existing) = target.and_then(|i| proxies.get_mut(i)) {
            existing.settings = settings;
            existing.id
        } else {
            let id = next_id();
            proxies.push(SavedProxyServer {
                id,
                settings,
                last_test: None,
            });
            id
        };
        ids.insert(profile.id, local_id);
    }
    crate::proxy::check_count(proxies.len())?;
    Ok((proxies, ids))
}

/// Replaces folders that do not exist on this machine, or that are startup or
/// system folders, with a fallback, and reports every folder setting with where
/// it ended up and why. Blocks on the disk.
fn localize_folders(profile: &mut SettingsFile, folders: &KnownFolders) -> Vec<ImportedFolder> {
    let settings = &mut profile.settings;
    let mut report = Vec::with_capacity(settings.categories.len() + profile.queues.len() + 1);
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
    for queue in &mut profile.queues {
        if let Setting::Own(folder) = &mut queue.settings.folder {
            place(
                format!("{} queue folder", queue.settings.name.trim()),
                folder,
                &default_folder,
            );
        }
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
    use crate::{default_queue, default_settings};
    use wirecatch_protocol::app::{MediaDefaults, QueueSettings, Setting};

    fn folders() -> KnownFolders {
        KnownFolders::under_home(std::env::temp_dir().join("wc-profile-home"))
    }

    fn queue(id: i64, name: &str) -> Queue {
        Queue {
            id: QueueId(id),
            is_default: false,
            settings: QueueSettings {
                max_concurrent: Setting::Own(1),
                ..QueueSettings::new(name)
            },
            status: QueueStatus::default(),
        }
    }

    fn in_profile(queue: Queue) -> SettingsFileQueue {
        SettingsFileQueue {
            id: queue.id,
            is_default: queue.is_default,
            settings: queue.settings,
        }
    }

    #[test]
    fn newer_profiles_are_refused_before_parsing() {
        let next = SETTINGS_FILE_FORMAT_VERSION + 1;
        let future = format!(r#"{{"format_version": {next}, "something_new": true}}"#);
        let err = parse(future.as_bytes()).unwrap_err();
        assert!(matches!(
            err,
            SettingsFileError::TooNew {
                found,
                supported: SETTINGS_FILE_FORMAT_VERSION
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
            Err(SettingsFileError::Invalid(_))
        ));
        assert!(matches!(parse(b"{}"), Err(SettingsFileError::Invalid(_))));
        assert!(matches!(
            parse(format!(r#"{{"format_version": {SETTINGS_FILE_FORMAT_VERSION}}}"#).as_bytes()),
            Err(SettingsFileError::Invalid(_))
        ));
    }

    #[test]
    fn export_then_parse_round_trips() {
        let f = folders();
        let mut s = default_settings();
        s.default_folder = PortablePath(f.downloads.join("Stuff").to_string_lossy().into_owned());
        let profile = export(&s, &[default_queue()], &[], &f, UnixMillis(42));
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
        let profile = export(&s, &[default_queue()], &[], &f, UnixMillis(42));
        let bytes = serde_json::to_vec(&profile).unwrap();
        assert_eq!(parse(&bytes).unwrap().settings.media, s.media);

        let mut old = serde_json::to_value(&profile).unwrap();
        old["settings"].as_object_mut().unwrap().remove("media");
        let read = parse(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(read.format_version, SETTINGS_FILE_FORMAT_VERSION);
        assert_eq!(read.settings.media, MediaDefaults::default());
    }

    #[test]
    fn a_profile_of_an_earlier_format_is_refused() {
        let mut old = serde_json::to_value(export(
            &default_settings(),
            &[default_queue()],
            &[],
            &folders(),
            UnixMillis(42),
        ))
        .unwrap();
        old["format_version"] = (SETTINGS_FILE_FORMAT_VERSION - 1).into();
        let err = parse(&serde_json::to_vec(&old).unwrap()).unwrap_err();
        assert!(
            matches!(err, SettingsFileError::TooOld { found } if found == u64::from(SETTINGS_FILE_FORMAT_VERSION - 1))
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
        let profile = serde_json::to_value(export(
            &default_settings(),
            &[paused],
            &[],
            &folders(),
            UnixMillis(42),
        ))
        .unwrap();
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
        incoming_default.settings.max_concurrent = Setting::Own(7);
        let mut night = queue(11, "night");
        night.settings.max_concurrent = Setting::Own(4);
        let mut settings = default_settings();
        settings.extension.default_queue = QueueId(12);
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: [incoming_default, night, queue(12, "Videos")]
                .into_iter()
                .map(in_profile)
                .collect(),
            proxy_servers: Vec::new(),
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
            (QueueId(1), Setting::Own(7), true)
        );
        assert_eq!(
            (q[1].id, q[1].settings.max_concurrent),
            (QueueId(2), Setting::Own(4))
        );
        assert_eq!(
            (q[2].id, q[2].settings.name.as_str(), q[2].is_default),
            (QueueId(3), "Videos", false)
        );
        assert_eq!(imported.settings.extension.default_queue, QueueId(3));
    }

    #[test]
    fn proxy_servers_merge_by_name_and_the_queues_follow_them() {
        use wirecatch_protocol::proxy::{ProxyKind, ProxyServerSettings, ProxyUse};

        let named = |name: &str, host: &str| ProxyServerSettings {
            name: name.into(),
            kind: ProxyKind::Http,
            host: host.into(),
            port: 8080,
            user: String::new(),
        };
        let office = SavedProxyServer {
            id: ProxyServerId(1),
            settings: named("Office", "old.example"),
            last_test: None,
        };
        let mut videos = queue(12, "Videos");
        videos.settings.proxy = Setting::Own(ProxyUse::Server(ProxyServerId(21)));
        let mut night = queue(13, "Night");
        night.settings.proxy = Setting::Own(ProxyUse::Server(ProxyServerId(99)));
        let mut settings = default_settings();
        settings.proxy.proxy = ProxyUse::Server(ProxyServerId(20));
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: [default_queue(), videos, night]
                .into_iter()
                .map(in_profile)
                .collect(),
            proxy_servers: vec![
                SettingsFileProxy {
                    id: ProxyServerId(20),
                    settings: named("office", "new.example"),
                },
                SettingsFileProxy {
                    id: ProxyServerId(21),
                    settings: named("Home", "home.example"),
                },
            ],
            addons: vec![],
        };
        let mut next_queue = 1;
        let imported = merge_localized(
            localize(profile, &folders()),
            Current {
                queues: &[default_queue()],
                proxies: &[office],
            },
            &folders(),
            &SuffixList::built_in(),
            NewIds {
                queue: || {
                    next_queue += 1;
                    QueueId(next_queue)
                },
                proxy: || ProxyServerId(5),
            },
        )
        .unwrap();

        let proxies: Vec<_> = imported
            .proxies
            .iter()
            .map(|p| (p.id, p.settings.host.as_str()))
            .collect();
        assert_eq!(
            proxies,
            [
                (ProxyServerId(1), "new.example"),
                (ProxyServerId(5), "home.example")
            ]
        );
        assert_eq!(
            imported.settings.proxy.proxy,
            ProxyUse::Server(ProxyServerId(1))
        );
        assert_eq!(
            imported.queues[1].settings.proxy,
            Setting::Own(ProxyUse::Server(ProxyServerId(5)))
        );
        assert_eq!(imported.queues[2].settings.proxy, Setting::Global);
    }

    #[test]
    fn an_oversized_file_is_refused_before_it_is_read_whole() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        std::fs::write(
            &big,
            vec![b' '; usize::try_from(MAX_SETTINGS_FILE_BYTES).unwrap() + 1],
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
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings: default_settings(),
            queues: (0..=i64::try_from(crate::MAX_QUEUES).unwrap())
                .map(|i| in_profile(queue(i + 10, &format!("Q{i}"))))
                .collect(),
            proxy_servers: Vec::new(),
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
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: vec![],
            proxy_servers: Vec::new(),
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
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: vec![],
            proxy_servers: Vec::new(),
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
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: vec![],
            proxy_servers: Vec::new(),
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

    /// A queue's folder travels like the other folder settings (D94.5).
    #[test]
    fn queue_folders_are_exported_portable_and_checked_on_import() {
        let foreign = if cfg!(windows) {
            "/home/ada/Films"
        } else {
            r"D:\Films"
        };
        let mut night = queue(4, "Night");
        let home = folders().downloads.join("Night");
        night.settings.folder = Setting::Own(PortablePath(home.to_string_lossy().into_owned()));
        let exported = export(
            &default_settings(),
            &[night.clone()],
            &[],
            &folders(),
            UnixMillis(0),
        );
        assert_eq!(
            exported.queues[0].settings.folder,
            Setting::Own(PortablePath("{Downloads}/Night".into()))
        );

        let mut profile = exported;
        profile.queues[0].settings.folder = Setting::Own(PortablePath(foreign.into()));
        let imported = merge(profile, &[default_queue()], &folders(), || QueueId(5)).unwrap();
        let added = imported
            .queues
            .iter()
            .find(|q| q.settings.name == "Night")
            .unwrap();
        assert_eq!(
            added.settings.folder,
            Setting::Own(imported.settings.default_folder.clone())
        );
        let reported = imported.summary.folders.last().unwrap();
        assert_eq!(reported.setting, "Night queue folder");
        assert_eq!(reported.requested, PortablePath(foreign.into()));
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
        let profile = SettingsFile {
            format_version: SETTINGS_FILE_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            exported_at: UnixMillis(0),
            settings,
            queues: vec![],
            proxy_servers: Vec::new(),
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
