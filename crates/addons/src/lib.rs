//! The media add-on (D20): yt-dlp, ffmpeg and Deno, installed together into
//! `<data_dir>/addons/media/<component>/`.
//!
//! Files on disk:
//!
//! - `installed.json`: what is installed (written last, so a half-finished
//!   install never counts), each file's size, and when updates were last checked.
//! - `<component>/`: the files of each part, swapped in whole.
//! - `.work/`: release files being downloaded (resumable) and parts being unpacked.

#![forbid(unsafe_code)]

pub mod archive;
pub mod manifest;
pub mod signature;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use manifest::{AddonComponent, ComponentId, Sha256Digest};

/// The add-on's folder inside the data folder.
#[must_use]
pub fn media_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("addons").join(manifest::MEDIA_ADDON_FOLDER)
}

/// The record of what is installed, in the add-on folder.
pub const RECORD_FILE: &str = "installed.json";

/// What is installed, as recorded on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// By part.
    pub components: BTreeMap<ComponentId, InstalledComponent>,
    /// When the update check last ran, in Unix milliseconds.
    #[serde(default)]
    pub last_update_check: Option<i64>,
}

/// One installed part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledComponent {
    pub version: String,
    /// The program to run, relative to the component folder.
    pub program: String,
    /// Every installed file.
    pub files: Vec<InstalledFile>,
}

/// One installed file, as unpacked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledFile {
    /// Relative to the part's folder, with `/` separators.
    pub path: String,
    pub size: u64,
    /// Taken while it was written.
    pub sha256: Sha256Digest,
}

impl Record {
    /// Reads the record. A missing one means nothing is installed, and so does one whose content is damaged (nothing it names can be trusted).
    ///
    /// # Errors
    ///
    /// When the file is there but cannot be read, for example while an antivirus holds it: what is installed is then not known, which is not the same as nothing. Blocks on the disk.
    pub fn load(dir: &Path) -> std::io::Result<Self> {
        match std::fs::read(dir.join(RECORD_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                warn!(error = %e, "the media add-on record is damaged; treating it as not installed");
                Self::default()
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes the record, replacing the old one in one step, on disk before it replaces the old one, so a power cut leaves one or the other whole. Blocks on the disk.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        wirecatch_files::whole_file::replace(dir, RECORD_FILE, &json)
    }

    #[must_use]
    pub fn version(&self, id: ComponentId) -> Option<&str> {
        self.components.get(&id).map(|c| c.version.as_str())
    }
}

/// How closely the add-on's files are looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Each file is there with its recorded size: on every start of the app.
    Sizes,
    /// Each file's SHA-256 matches too: when repairing, and after a program could not be started.
    Hashes,
}

/// The programs of an installed, intact media add-on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPaths {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: PathBuf,
    /// yt-dlp's own cache (player code, signatures), in the add-on folder: never the one another yt-dlp on the machine writes, and emptied when yt-dlp is replaced.
    pub ytdlp_cache: PathBuf,
}

/// yt-dlp's cache folder in the add-on folder `dir`.
#[must_use]
pub fn ytdlp_cache(dir: &Path) -> PathBuf {
    dir.join("cache")
}

/// The add-on's programs if every recorded file is there with its recorded size, so nothing half-installed or damaged is ever run, and with [`Check::Hashes`] its SHA-256. Each program must be one of those files: a record naming any other (written by hand, or by a bug) runs nothing. Blocks on the disk.
#[must_use]
pub fn tools(dir: &Path, record: &Record, check: Check) -> Option<ToolPaths> {
    let program = |id: ComponentId| -> Option<PathBuf> {
        let component = record.components.get(&id)?;
        if !component.files.iter().any(|f| f.path == component.program) {
            warn!(part = id.as_str(), program = %component.program, "the media add-on's program is not one of its checked files");
            return None;
        }
        let folder = dir.join(id.as_str());
        for file in &component.files {
            let path = archive::safe_join(&folder, &file.path).ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            let intact = meta.is_file()
                && meta.len() == file.size
                && (check == Check::Sizes
                    || archive::sha256_file(&path).is_ok_and(|h| h == file.sha256));
            if !intact {
                warn!(path = %path.display(), "a media add-on file is missing or changed");
                return None;
            }
        }
        archive::safe_join(&folder, &component.program).ok()
    };
    Some(ToolPaths {
        ytdlp: program(ComponentId::YtDlp)?,
        ffmpeg: program(ComponentId::Ffmpeg)?,
        deno: program(ComponentId::Deno)?,
        ytdlp_cache: ytdlp_cache(dir),
    })
}

/// Whether yt-dlp version `candidate` is newer than `installed`. Versions are
/// dates such as `2026.08.19`, sometimes with a fourth number; they compare
/// number by number.
#[must_use]
pub fn newer(candidate: &str, installed: &str) -> bool {
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
pub struct Release {
    pub tag_name: String,
    /// When it was published (RFC 3339).
    #[serde(default)]
    pub published_at: Option<String>,
    /// A draft is not published yet.
    #[serde(default)]
    pub draft: bool,
    /// Marked as not ready for everyone.
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A file attached to a GitHub release.
#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// The checksum of `file` in a `SHA2-256SUMS` list (`<hex>  <name>` lines; a
/// `*` before the name marks binary mode).
#[must_use]
pub fn checksum_for(list: &str, file: &str) -> Option<Sha256Digest> {
    list.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == file)
            .then(|| Sha256Digest::parse(hash).ok())
            .flatten()
    })
}

/// Whether `wanted` must be installed: its installed version differs, keeping a yt-dlp that is already newer than `wanted` (the update check installed it).
#[must_use]
pub fn outdated(record: &Record, wanted: &AddonComponent) -> bool {
    match record.version(wanted.id()) {
        None => true,
        Some(have) if wanted.id() == ComponentId::YtDlp => newer(wanted.version(), have),
        Some(have) => have != wanted.version(),
    }
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
            checksum_for(list, "yt-dlp_linux")
                .map(|d| d.to_string())
                .as_deref(),
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
            ComponentId::Deno,
            InstalledComponent {
                version: "2.9.7".into(),
                program: "deno".into(),
                files: vec![InstalledFile {
                    path: "deno".into(),
                    size: 3,
                    sha256: Sha256Digest::from_bytes([0; 32]),
                }],
            },
        );
        record.save(dir.path()).unwrap();
        assert_eq!(Record::load(dir.path()).unwrap(), record);
        assert!(!dir.path().join(format!("{RECORD_FILE}.tmp")).exists());
        // Nothing to run: the file itself is missing.
        assert!(tools(dir.path(), &record, Check::Sizes).is_none());
    }

    /// Parts are named only by the app: a record naming any other (a folder outside the add-on's, say) or holding a malformed checksum vouches for nothing.
    #[test]
    fn a_record_naming_an_unknown_part_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let file = |part: &str, sha256: &str| {
            format!(
                r#"{{"components":{{"{part}":{{"version":"1","program":"tool","files":[{{"path":"tool","size":4,"sha256":"{sha256}"}}]}}}}}}"#
            )
        };
        let good = "0".repeat(64);
        std::fs::write(dir.path().join(RECORD_FILE), file("deno", &good)).unwrap();
        assert!(
            Record::load(dir.path())
                .unwrap()
                .components
                .contains_key(&ComponentId::Deno)
        );
        for (part, sha256) in [
            ("../victim", good.as_str()),
            ("Deno", &good),
            ("deno", "00"),
            ("deno", &"G".repeat(64)),
        ] {
            std::fs::write(dir.path().join(RECORD_FILE), file(part, sha256)).unwrap();
            assert_eq!(
                Record::load(dir.path()).unwrap(),
                Record::default(),
                "{part} {sha256}"
            );
        }
    }

    /// A record whose program is not one of its checked files would run a file nothing ever checked.
    #[test]
    fn a_program_that_is_not_one_of_the_checked_files_is_never_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = Record::default();
        for id in ComponentId::ALL {
            let folder = dir.path().join(id.as_str());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("tool"), b"checked").unwrap();
            std::fs::write(folder.join("other"), b"never checked").unwrap();
            record.components.insert(
                id,
                InstalledComponent {
                    version: "1".into(),
                    program: "tool".into(),
                    files: vec![InstalledFile {
                        path: "tool".into(),
                        size: 7,
                        sha256: archive::sha256_file(&folder.join("tool")).unwrap(),
                    }],
                },
            );
        }
        assert!(tools(dir.path(), &record, Check::Hashes).is_some());
        record
            .components
            .get_mut(&ComponentId::Ffmpeg)
            .unwrap()
            .program = "other".into();
        for check in [Check::Sizes, Check::Hashes] {
            assert!(tools(dir.path(), &record, check).is_none(), "{check:?}");
        }
    }

    /// Only a missing record means nothing is installed; one that cannot be read (held by an antivirus, say) is an error, so the add-on is not offered for a full reinstall over a passing lock.
    #[test]
    fn a_record_that_cannot_be_read_is_not_taken_for_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Record::load(dir.path()).unwrap(), Record::default());
        std::fs::create_dir(dir.path().join(RECORD_FILE)).unwrap();
        assert!(Record::load(dir.path()).is_err());
    }
}
