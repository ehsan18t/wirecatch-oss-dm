//! The media add-on (D20): yt-dlp, ffmpeg and Deno, installed together into
//! `<data_dir>/addons/media/<component>/`.
//!
//! Files on disk:
//!
//! - `installed.json`: what is installed (written last, so a half-finished
//!   install never counts), each file's size, and when updates were last checked.
//! - `<component>/`: the files of each part, swapped in whole.
//! - `.work/`: release files being downloaded (resumable) and parts being unpacked.

pub(crate) mod archive;
pub(crate) mod manifest;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use manifest::AddonComponent;

/// The add-on's folder inside the data folder.
pub(crate) fn media_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("addons").join(manifest::MEDIA_ADDON_ID)
}

const RECORD_FILE: &str = "installed.json";

/// What is installed, as recorded on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// By component name.
    pub components: BTreeMap<String, InstalledComponent>,
    /// When the update check last ran, in Unix milliseconds.
    #[serde(default)]
    pub last_update_check: Option<i64>,
}

/// One installed part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstalledComponent {
    pub version: String,
    /// The program to run, relative to the component folder.
    pub program: String,
    /// Every installed file (relative path) and its size.
    pub files: Vec<(String, u64)>,
}

impl Record {
    /// Reads the record; a missing or unreadable one means nothing is installed.
    /// Blocks on the disk.
    pub fn load(dir: &Path) -> Self {
        match std::fs::read(dir.join(RECORD_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                warn!(error = %e, "the media add-on record is unreadable; treating it as not installed");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Writes the record, replacing the old one in one step. Blocks on the disk.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{RECORD_FILE}.tmp"));
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join(RECORD_FILE))
    }

    pub fn version(&self, name: &str) -> Option<&str> {
        self.components.get(name).map(|c| c.version.as_str())
    }
}

/// The programs of an installed, intact media add-on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaTools {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: PathBuf,
}

/// The add-on's programs if every recorded file is there with its recorded
/// size, so nothing half-installed or damaged is ever run. Blocks on the disk.
pub(crate) fn tools(dir: &Path, record: &Record) -> Option<MediaTools> {
    let program = |name: &str| -> Option<PathBuf> {
        let component = record.components.get(name)?;
        let folder = dir.join(name);
        for (file, size) in &component.files {
            let path = archive::safe_join(&folder, file).ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.len() != *size {
                warn!(path = %path.display(), "a media add-on file is missing or changed");
                return None;
            }
        }
        archive::safe_join(&folder, &component.program).ok()
    };
    Some(MediaTools {
        ytdlp: program("yt-dlp")?,
        ffmpeg: program("ffmpeg")?,
        deno: program("deno")?,
    })
}

/// Whether yt-dlp version `candidate` is newer than `installed`. Versions are
/// dates such as `2026.08.19`, sometimes with a fourth number; they compare
/// number by number.
pub(crate) fn newer(candidate: &str, installed: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    match (parse(candidate), parse(installed)) {
        (Some(a), Some(b)) => a > b,
        _ => candidate != installed,
    }
}

/// A GitHub release, as far as the update check reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A file attached to a GitHub release.
#[derive(Debug, Deserialize)]
pub(crate) struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// The checksum of `file` in a `SHA2-256SUMS` list (`<hex>  <name>` lines; a
/// `*` before the name marks binary mode).
pub(crate) fn checksum_for(list: &str, file: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == file && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// The components whose installed version differs from `wanted`, keeping a yt-dlp
/// that is already newer than the pin (the update check installed it).
pub(crate) fn outdated<'a>(
    record: &Record,
    wanted: impl IntoIterator<Item = &'a AddonComponent>,
) -> Vec<AddonComponent> {
    wanted
        .into_iter()
        .filter(|c| match record.version(&c.name) {
            None => true,
            Some(have) if c.name == "yt-dlp" => newer(&c.version, have),
            Some(have) => have != c.version,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yt_dlp_versions_compare_as_dates() {
        assert!(newer("2026.09.01", "2026.08.19"));
        assert!(newer("2026.08.19.1", "2026.08.19"));
        assert!(!newer("2026.08.19", "2026.08.19"));
        assert!(!newer("2025.12.31", "2026.01.01"));
    }

    #[test]
    fn checksums_are_found_by_exact_name() {
        let list = "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a  yt-dlp.exe\n\
                    58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a *yt-dlp_linux\n\
                    00  yt-dlp_x86.exe\n";
        assert_eq!(
            checksum_for(list, "yt-dlp_linux").as_deref(),
            Some("58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a")
        );
        assert!(checksum_for(list, "yt-dlp").is_none());
        assert!(checksum_for(list, "yt-dlp_x86.exe").is_none());
    }

    #[test]
    fn the_record_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = Record::default();
        record.components.insert(
            "deno".into(),
            InstalledComponent {
                version: "2.9.7".into(),
                program: "deno".into(),
                files: vec![("deno".into(), 3)],
            },
        );
        record.save(dir.path()).unwrap();
        assert_eq!(Record::load(dir.path()), record);
        // Nothing to run: the file itself is missing.
        assert!(tools(dir.path(), &record).is_none());
    }
}
