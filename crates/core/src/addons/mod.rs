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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// Every installed file.
    pub files: Vec<InstalledFile>,
}

/// One installed file, as unpacked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstalledFile {
    /// Relative to the part's folder, with `/` separators.
    pub path: String,
    pub size: u64,
    /// Lower-case hex, taken while it was written.
    pub sha256: String,
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
#[derive(Debug, Clone)]
pub(crate) struct MediaTools {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: PathBuf,
    /// Set (for every clone) when one of the programs could not be started, for
    /// example because an antivirus took it away: the manager then looks at the
    /// files again instead of trusting what it found earlier.
    broken: Arc<AtomicBool>,
}

impl PartialEq for MediaTools {
    fn eq(&self, other: &Self) -> bool {
        (&self.ytdlp, &self.ffmpeg, &self.deno) == (&other.ytdlp, &other.ffmpeg, &other.deno)
    }
}

impl Eq for MediaTools {}

impl MediaTools {
    /// Records that a program could not be started.
    pub fn mark_broken(&self) {
        self.broken.store(true, Ordering::Relaxed);
    }

    /// Whether a program could not be started since these were found.
    pub fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }

    /// Tools whose ffmpeg is `ffmpeg`, for tests of code that runs it.
    #[cfg(test)]
    pub fn for_test(ffmpeg: &Path) -> MediaTools {
        MediaTools {
            ytdlp: PathBuf::from("yt-dlp"),
            ffmpeg: ffmpeg.to_path_buf(),
            deno: PathBuf::from("deno"),
            broken: Arc::default(),
        }
    }
}

/// The add-on's programs if every recorded file is there with its recorded
/// size, so nothing half-installed or damaged is ever run. With `deep`, each
/// file's SHA-256 is checked too: when repairing, and after a program could not
/// be started, not on every start of the app. Blocks on the disk.
pub(crate) fn tools(dir: &Path, record: &Record, deep: bool) -> Option<MediaTools> {
    let program = |name: &str| -> Option<PathBuf> {
        let component = record.components.get(name)?;
        let folder = dir.join(name);
        for file in &component.files {
            let path = archive::safe_join(&folder, &file.path).ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            let intact = meta.is_file()
                && meta.len() == file.size
                && (!deep || archive::sha256_file(&path).is_ok_and(|h| h == file.sha256));
            if !intact {
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
        broken: Arc::default(),
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
        // A version that cannot be read is never taken as newer.
        _ => false,
    }
}

/// A GitHub release, as far as the update check reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct Release {
    pub tag_name: String,
    /// When it was published (RFC 3339).
    #[serde(default)]
    pub published_at: Option<String>,
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
        for unreadable in ["nightly", "2026.09.30-evil", "../2027.01.01", "", "2026..1"] {
            assert!(!newer(unreadable, "2026.08.19"), "{unreadable}");
        }
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
                files: vec![InstalledFile {
                    path: "deno".into(),
                    size: 3,
                    sha256: "0".repeat(64),
                }],
            },
        );
        record.save(dir.path()).unwrap();
        assert_eq!(Record::load(dir.path()), record);
        // Nothing to run: the file itself is missing.
        assert!(tools(dir.path(), &record, false).is_none());
    }
}
