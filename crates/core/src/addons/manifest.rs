//! Where the media add-on's parts come from: pinned releases with their SHA-256
//! checksums (D20, A8). Nothing is run unless it matched these.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use wirecatch_engine::Url;
use wirecatch_product::ADDON_FILES_URL;

pub use super::signature::{Fingerprint, PinnedKey};

/// The media add-on's folder in the data folder's `addons` folder; the add-on itself is
/// `AddonId::Media`. Written out so a rename of the id never moves anyone's install.
pub const MEDIA_ADDON_FOLDER: &str = "media";

/// One part of the media add-on. Its [`as_str`](Self::as_str) names its folder and its entry in the record of what is installed, so no other text can name a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComponentId {
    /// yt-dlp, which reads pages and downloads what Wirecatch cannot.
    #[serde(rename = "yt-dlp")]
    YtDlp,
    /// ffmpeg and the libraries it needs.
    #[serde(rename = "ffmpeg")]
    Ffmpeg,
    /// The Deno JavaScript runtime yt-dlp uses for some sites.
    #[serde(rename = "deno")]
    Deno,
}

impl ComponentId {
    /// Every part, in the order they are installed and shown.
    pub const ALL: [ComponentId; 3] = [ComponentId::YtDlp, ComponentId::Ffmpeg, ComponentId::Deno];

    /// The part's folder name and its name in the record.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ComponentId::YtDlp => "yt-dlp",
            ComponentId::Ffmpeg => "ffmpeg",
            ComponentId::Deno => "deno",
        }
    }
}

/// The part's name as a person reads it.
impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ComponentId::YtDlp => "yt-dlp",
            ComponentId::Ffmpeg => "ffmpeg",
            ComponentId::Deno => "Deno",
        })
    }
}

/// A SHA-256 digest. Parsed once from hex in either case and compared as bytes; shown and stored as lower-case hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// The digest with these bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Reads 64 hex digits, upper or lower case.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Sha256`] for anything else.
    pub fn parse(hex: &str) -> Result<Self, ManifestError> {
        Self::from_hex(hex).ok_or_else(|| ManifestError::Sha256(hex.chars().take(80).collect()))
    }

    /// [`parse`](Self::parse) for constants: `None` for anything that is not 64 hex digits.
    #[must_use]
    pub const fn from_hex(hex: &str) -> Option<Self> {
        const fn digit(b: u8) -> Option<u8> {
            match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            }
        }
        let hex = hex.as_bytes();
        let (64, Some(hex)) = (hex.len(), hex.first_chunk::<64>()) else {
            return None;
        };
        let mut bytes = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            let (Some(high), Some(low)) = (digit(hex[2 * i]), digit(hex[2 * i + 1])) else {
                return None;
            };
            bytes[i] = (high << 4) | low;
            i += 1;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl std::fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sha256Digest({self})")
    }
}

impl std::str::FromStr for Sha256Digest {
    type Err = ManifestError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        Self::parse(hex)
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Self::parse(&hex).map_err(serde::de::Error::custom)
    }
}

/// A digest written in the app's own manifest, checked while the app is compiled: it is only ever evaluated in a constant.
const fn pinned(hex: &str) -> Sha256Digest {
    match Sha256Digest::from_hex(hex) {
        Some(digest) => digest,
        None => panic!("a pinned SHA-256 is not 64 hex digits"),
    }
}

/// Why add-on sources were refused. Each message is a phrase naming what is wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// A SHA-256 that is not 64 hex digits.
    #[error("{0:?} is not a SHA-256 (64 hex digits)")]
    Sha256(String),
    /// A release file whose size is not known.
    #[error("the release file {0} has no size")]
    UnknownSize(String),
    /// A key fingerprint that is not 40 or 64 hex digits.
    #[error("{0:?} is not a full OpenPGP key fingerprint")]
    Fingerprint(String),
    /// The part names no file to install.
    #[error("{0} names no file to install")]
    NoFiles(ComponentId),
    /// A release file that is the program itself names more than one file.
    #[error("{0} is a single program but names more than one file")]
    PlainTakesOneFile(ComponentId),
    /// A path that could leave the part's folder, or that Windows reads as something else.
    #[error("{component} names the unsafe path {path:?}")]
    UnsafePath {
        /// The part.
        component: ComponentId,
        /// The path.
        path: String,
    },
    /// Two files would be the same file (Windows does not tell case apart).
    #[error("{component} names {path:?} twice")]
    Duplicate {
        /// The part.
        component: ComponentId,
        /// The path.
        path: String,
    },
    /// The program is not one of the files installed and checked.
    #[error("{component}'s program {program:?} is not one of its files")]
    ProgramNotListed {
        /// The part.
        component: ComponentId,
        /// The program.
        program: String,
    },
    /// The program is installed as a file that is not a program.
    #[error("{component}'s program {program:?} is not marked as a program")]
    ProgramNotExecutable {
        /// The part.
        component: ComponentId,
        /// The program.
        program: String,
    },
}

/// The release file a part is installed from: where it is, its SHA-256 (the file is refused, and never unpacked or run, when it does not match) and its size in bytes (progress before the server answers, and a cap on what is downloaded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFile {
    url: Url,
    sha256: Sha256Digest,
    size: u64,
}

impl ReleaseFile {
    /// The release file at `url` with this checksum and size.
    ///
    /// # Errors
    ///
    /// [`ManifestError::UnknownSize`] for a size of 0: a file of unknown size is never downloaded, since its size caps what is.
    pub fn new(url: Url, sha256: Sha256Digest, size: u64) -> Result<Self, ManifestError> {
        if size == 0 {
            return Err(ManifestError::UnknownSize(url.to_string()));
        }
        Ok(Self { url, sha256, size })
    }

    /// Its download address.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Its SHA-256.
    #[must_use]
    pub fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    /// Its size in bytes, never 0.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// One part of the media add-on (yt-dlp, ffmpeg or Deno) as published by its project: where to get it, its checksum, and which files to take from it. One that exists is valid: its program is one of its files, and every file goes to a safe path of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonComponent {
    id: ComponentId,
    version: String,
    release: ReleaseFile,
    archive: ArchiveKind,
    files: Vec<AddonFile>,
    program: String,
}

impl AddonComponent {
    /// A part installed from `release`, packed as `archive`, taking `files` from it, whose program is `program`: the [`to`](AddonFile::to) of one of `files`, marked as a program.
    ///
    /// # Errors
    ///
    /// When no file is named, a plain release names more than one, a path could leave the part's folder or two paths are the same file, or the program is not one of the files or not marked as a program.
    pub fn new(
        id: ComponentId,
        version: impl Into<String>,
        release: ReleaseFile,
        archive: ArchiveKind,
        files: Vec<AddonFile>,
        program: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        let component = Self {
            id,
            version: version.into(),
            release,
            archive,
            files,
            program: program.into(),
        };
        component.check()?;
        Ok(component)
    }

    fn check(&self) -> Result<(), ManifestError> {
        let id = self.id;
        if self.files.is_empty() {
            return Err(ManifestError::NoFiles(id));
        }
        if self.archive == ArchiveKind::Plain && self.files.len() > 1 {
            return Err(ManifestError::PlainTakesOneFile(id));
        }
        let mut targets = std::collections::HashSet::new();
        let mut sources = std::collections::HashSet::new();
        for file in &self.files {
            if !super::archive::is_safe_relative(&file.to) {
                return Err(ManifestError::UnsafePath {
                    component: id,
                    path: file.to.clone(),
                });
            }
            let duplicate = !targets.insert(file.to.to_lowercase())
                || (self.archive != ArchiveKind::Plain && !sources.insert(file.from.as_str()));
            if duplicate {
                return Err(ManifestError::Duplicate {
                    component: id,
                    path: file.to.clone(),
                });
            }
        }
        match self.files.iter().find(|f| f.to == self.program) {
            None => Err(ManifestError::ProgramNotListed {
                component: id,
                program: self.program.clone(),
            }),
            Some(file) if !file.executable => Err(ManifestError::ProgramNotExecutable {
                component: id,
                program: self.program.clone(),
            }),
            Some(_) => Ok(()),
        }
    }

    /// The same part from another release: its files and program stay.
    #[must_use]
    pub fn for_release(&self, version: impl Into<String>, release: ReleaseFile) -> Self {
        Self {
            version: version.into(),
            release,
            ..self.clone()
        }
    }

    /// Which part this is.
    #[must_use]
    pub fn id(&self) -> ComponentId {
        self.id
    }

    /// The release, shown to the user and compared on updates.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The release file.
    #[must_use]
    pub fn release(&self) -> &ReleaseFile {
        &self.release
    }

    /// How the release file is packed.
    #[must_use]
    pub fn archive(&self) -> ArchiveKind {
        self.archive
    }

    /// The files to install. Anything else in the archive is ignored.
    #[must_use]
    pub fn files(&self) -> &[AddonFile] {
        &self.files
    }

    /// The program to run, relative to the part's folder: always one of the [`files`](Self::files).
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }
}

/// How a release file is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// The release file is the program itself; [`AddonFile::from`] is ignored.
    Plain,
    /// A `.zip` archive.
    Zip,
    /// A `.tar.xz` archive.
    TarXz,
}

/// One file to take from a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonFile {
    /// Its path inside the archive, with `/` separators. For a symbolic link
    /// in the archive, name the file it points to: links are never installed.
    pub from: String,
    /// Where it goes, relative to the component's folder, with `/` separators.
    pub to: String,
    /// Whether it is a program (Linux: mode 0755 instead of 0644).
    pub executable: bool,
}

/// Everything the media add-on is installed and updated from.
///
/// [`MediaAddonSources::official`] is what the app uses; tests point the
/// manager at a local server instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAddonSources {
    /// The yt-dlp program, pinned. Newer releases found by the update check
    /// replace it.
    pub ytdlp: AddonComponent,
    /// ffmpeg and the libraries it needs. Updated only when this pin changes.
    pub ffmpeg: AddonComponent,
    /// The Deno JavaScript runtime yt-dlp uses for some sites. Updated only when
    /// this pin changes.
    pub deno: AddonComponent,
    /// The GitHub API address of yt-dlp's list of releases (newest first),
    /// read at most once a day to keep up with sites that change; `None` turns
    /// that off. The list, not just the latest release, so the newest release
    /// at least two days old can be found while a newer one waits (D21.1).
    pub ytdlp_releases: Option<Url>,
    /// The name of this platform's yt-dlp file in a release, for example
    /// `yt-dlp.exe`.
    pub ytdlp_asset: String,
    /// The name of the checksum list in a yt-dlp release. Its detached OpenPGP
    /// signature is the file of the same name with `.sig` added.
    pub ytdlp_checksums_asset: String,
    /// The key the checksum list of a newer yt-dlp must be signed with; a
    /// release whose signature does not verify is never installed.
    pub ytdlp_signing_key: PinnedKey,
    /// Where yt-dlp's release files must come from: a newer release's files
    /// are only taken from `<this><tag>/<file name>`, whatever the release
    /// description says.
    pub ytdlp_release_downloads: Url,
}

impl MediaAddonSources {
    /// The parts in installation order.
    #[must_use]
    pub fn components(&self) -> [&AddonComponent; 3] {
        [&self.ytdlp, &self.ffmpeg, &self.deno]
    }
}

const YTDLP_VERSION: &str = "2026.08.19";
const FFMPEG_VERSION: &str = "n9.0.2-36-g6882969ce9";
const DENO_VERSION: &str = "2.9.7";

/// The ffmpeg shared libraries `ffmpeg` links against (`BtbN` LGPL shared build,
/// FFmpeg 9.0): avcodec 63, avdevice 63, avfilter 12, avformat 63, avutil 61,
/// swresample 7 and swscale 10.
#[cfg(windows)]
const FFMPEG_LIBRARIES: [&str; 7] = [
    "avcodec-63.dll",
    "avdevice-63.dll",
    "avfilter-12.dll",
    "avformat-63.dll",
    "avutil-61.dll",
    "swresample-7.dll",
    "swscale-10.dll",
];

/// The same for Linux: the real file in the archive and the name the
/// program asks for (`ffmpeg` finds them through its run path `$ORIGIN/../lib`).
#[cfg(not(windows))]
const FFMPEG_LIBRARIES: [(&str, &str); 7] = [
    ("libavcodec.so.63.1.102", "libavcodec.so.63"),
    ("libavdevice.so.63.1.102", "libavdevice.so.63"),
    ("libavfilter.so.12.1.102", "libavfilter.so.12"),
    ("libavformat.so.63.1.102", "libavformat.so.63"),
    ("libavutil.so.61.1.102", "libavutil.so.61"),
    ("libswresample.so.7.1.102", "libswresample.so.7"),
    ("libswscale.so.10.1.102", "libswscale.so.10"),
];

impl MediaAddonSources {
    /// The pinned official releases for this platform (Windows or Linux on
    /// x86-64): yt-dlp and Deno from their GitHub releases, ffmpeg from the copy of a `BtbN`
    /// LGPL shared build kept in the add-on files repository (D80), whose builds expire.
    /// The checksums are each release's published ones.
    #[must_use]
    pub fn official() -> Self {
        Self {
            ytdlp: official_ytdlp(),
            ffmpeg: official_ffmpeg(),
            deno: official_deno(),
            ytdlp_releases: Some(pinned_address(
                "https://api.github.com/repos/yt-dlp/yt-dlp/releases?per_page=10",
            )),
            ytdlp_asset: ytdlp_asset().into(),
            ytdlp_checksums_asset: "SHA2-256SUMS".into(),
            ytdlp_signing_key: PinnedKey::ytdlp(),
            ytdlp_release_downloads: pinned_address(YTDLP_RELEASE_DOWNLOADS),
        }
    }
}

/// Where yt-dlp's release files are, followed by a version and a file name.
const YTDLP_RELEASE_DOWNLOADS: &str = "https://github.com/yt-dlp/yt-dlp/releases/download/";

/// One of the official manifest's addresses, put together from this file's constants.
#[expect(
    clippy::expect_used,
    reason = "given only addresses put together from this file's constants, every one of which the_official_sources_are_pinned_and_consistent parses on each system it builds for"
)]
fn pinned_address(url: &str) -> Url {
    Url::parse(url).expect("a pinned add-on address is not a URL")
}

#[cfg(windows)]
fn ytdlp_asset() -> &'static str {
    "yt-dlp.exe"
}

#[cfg(not(windows))]
fn ytdlp_asset() -> &'static str {
    "yt-dlp_linux"
}

/// The program's name inside the add-on folder.
pub(crate) fn program_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// A part of the app's own manifest. It is checked by a test like any other, so it is put together directly rather than through [`AddonComponent::new`], whose error has nowhere to go here.
fn official(
    id: ComponentId,
    version: &str,
    release: ReleaseFile,
    archive: ArchiveKind,
    files: Vec<AddonFile>,
    program: String,
) -> AddonComponent {
    AddonComponent {
        id,
        version: version.into(),
        release,
        archive,
        files,
        program,
    }
}

fn official_ytdlp() -> AddonComponent {
    #[cfg(windows)]
    const RELEASE: (Sha256Digest, u64) = (
        pinned("66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a"),
        17_840_399,
    );
    #[cfg(not(windows))]
    const RELEASE: (Sha256Digest, u64) = (
        pinned("58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a"),
        40_446_224,
    );
    let program = program_name("yt-dlp");
    official(
        ComponentId::YtDlp,
        YTDLP_VERSION,
        ReleaseFile {
            url: pinned_address(&format!(
                "{YTDLP_RELEASE_DOWNLOADS}{YTDLP_VERSION}/{}",
                ytdlp_asset()
            )),
            sha256: RELEASE.0,
            size: RELEASE.1,
        },
        ArchiveKind::Plain,
        vec![AddonFile {
            from: String::new(),
            to: program.clone(),
            executable: true,
        }],
        program,
    )
}

fn official_ffmpeg() -> AddonComponent {
    #[cfg(windows)]
    const RELEASE: (&str, Sha256Digest, u64, ArchiveKind) = (
        "win64-lgpl-shared-9.0.zip",
        pinned("72e3fbaf17f8d5162c3010f9e58872fcb93ba2b03731fd0fff1ca9aabb616ea6"),
        77_231_366,
        ArchiveKind::Zip,
    );
    #[cfg(not(windows))]
    const RELEASE: (&str, Sha256Digest, u64, ArchiveKind) = (
        "linux64-lgpl-shared-9.0.tar.xz",
        pinned("a73032b9313945304cb75b8a96015d28e553057265d076157b05b1de1b6d6a3d"),
        63_528_100,
        ArchiveKind::TarXz,
    );
    let (asset, sha256, size, archive) = RELEASE;
    let top = format!("ffmpeg-{FFMPEG_VERSION}-{}", asset_stem(asset));
    #[cfg(windows)]
    let (files, program) = {
        let mut files = vec![AddonFile {
            from: format!("{top}/bin/ffmpeg.exe"),
            to: "ffmpeg.exe".into(),
            executable: true,
        }];
        files.extend(FFMPEG_LIBRARIES.iter().map(|dll| AddonFile {
            from: format!("{top}/bin/{dll}"),
            to: (*dll).into(),
            executable: false,
        }));
        (files, "ffmpeg.exe".to_string())
    };
    #[cfg(not(windows))]
    let (files, program) = {
        let mut files = vec![AddonFile {
            from: format!("{top}/bin/ffmpeg"),
            to: "bin/ffmpeg".into(),
            executable: true,
        }];
        files.extend(FFMPEG_LIBRARIES.iter().map(|(real, soname)| AddonFile {
            from: format!("{top}/lib/{real}"),
            to: format!("lib/{soname}"),
            executable: false,
        }));
        (files, "bin/ffmpeg".to_string())
    };
    official(
        ComponentId::Ffmpeg,
        FFMPEG_VERSION,
        ReleaseFile {
            url: pinned_address(&format!(
                "{ADDON_FILES_URL}/releases/download/ffmpeg-{FFMPEG_VERSION}/ffmpeg-{FFMPEG_VERSION}-{asset}"
            )),
            sha256,
            size,
        },
        archive,
        files,
        program,
    )
}

/// `win64-lgpl-shared-9.0.zip` without its archive extension.
fn asset_stem(asset: &str) -> &str {
    asset
        .strip_suffix(".zip")
        .or_else(|| asset.strip_suffix(".tar.xz"))
        .unwrap_or(asset)
}

fn official_deno() -> AddonComponent {
    #[cfg(windows)]
    const RELEASE: (&str, Sha256Digest, u64) = (
        "x86_64-pc-windows-msvc",
        pinned("a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238"),
        42_630_221,
    );
    #[cfg(not(windows))]
    const RELEASE: (&str, Sha256Digest, u64) = (
        "x86_64-unknown-linux-gnu",
        pinned("c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490"),
        41_596_794,
    );
    let (target, sha256, size) = RELEASE;
    let program = program_name("deno");
    official(
        ComponentId::Deno,
        DENO_VERSION,
        ReleaseFile {
            url: pinned_address(&format!(
                "https://github.com/denoland/deno/releases/download/v{DENO_VERSION}/deno-{target}.zip"
            )),
            sha256,
            size,
        },
        ArchiveKind::Zip,
        vec![AddonFile {
            from: program.clone(),
            to: program.clone(),
            executable: true,
        }],
        program,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_sources_are_pinned_and_consistent() {
        let sources = MediaAddonSources::official();
        assert_eq!(
            sources.components().map(AddonComponent::id),
            ComponentId::ALL
        );
        for c in sources.components() {
            // What `new` would refuse is refused here too.
            let rebuilt = AddonComponent::new(
                c.id(),
                c.version(),
                c.release().clone(),
                c.archive(),
                c.files().to_vec(),
                c.program(),
            );
            assert_eq!(rebuilt.as_ref(), Ok(c));
            let release = ReleaseFile::new(
                c.release().url().clone(),
                c.release().sha256(),
                c.release().size(),
            );
            assert_eq!(release.as_ref(), Ok(c.release()));
            assert!(
                c.release()
                    .url()
                    .as_str()
                    .starts_with("https://github.com/"),
                "{}",
                c.release().url()
            );
            assert!(c.release().size() > 1_000_000);
        }
        for url in [
            sources.ytdlp_releases.as_ref().map(Url::as_str),
            Some(sources.ytdlp_release_downloads.as_str()),
        ] {
            assert!(url.is_some_and(|u| u.starts_with("https://")), "{url:?}");
        }
        assert_eq!(
            sources.ytdlp_release_downloads.as_str(),
            YTDLP_RELEASE_DOWNLOADS
        );
        assert_eq!(sources.ffmpeg.files().len(), 8);
        assert!(
            sources
                .ffmpeg
                .release()
                .url()
                .as_str()
                .starts_with(ADDON_FILES_URL)
        );
        assert!(
            sources
                .ffmpeg
                .release()
                .url
                .as_str()
                .contains("lgpl-shared")
        );
    }

    #[test]
    fn a_checksum_is_read_in_either_case_and_shown_in_lower_case() {
        let lower = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let digest = Sha256Digest::parse(&lower.to_ascii_uppercase()).unwrap();
        assert_eq!(digest, Sha256Digest::parse(lower).unwrap());
        assert_eq!(digest.to_string(), lower);
        assert_eq!(
            serde_json::to_string(&digest).unwrap(),
            format!("\"{lower}\"")
        );
        for bad in [
            "",
            "00",
            &"g".repeat(64),
            &"0".repeat(63),
            &"0".repeat(65),
            &format!(" {}", &lower[1..]),
        ] {
            assert!(Sha256Digest::parse(bad).is_err(), "{bad:?}");
            assert!(serde_json::from_str::<Sha256Digest>(&format!("\"{bad}\"")).is_err());
        }
    }

    fn release() -> ReleaseFile {
        ReleaseFile {
            url: Url::parse("https://example.com/tool").unwrap(),
            sha256: Sha256Digest::from_bytes([0; 32]),
            size: 1,
        }
    }

    fn file(to: &str, executable: bool) -> AddonFile {
        AddonFile {
            from: format!("in/{to}"),
            to: to.into(),
            executable,
        }
    }

    /// A part whose program is not one of its files would run a file nothing checked; one whose paths leave its folder or meet in one file is never installed.
    #[test]
    fn a_malformed_part_is_refused() {
        let new = |archive, files: Vec<AddonFile>, program: &str| {
            AddonComponent::new(ComponentId::Ffmpeg, "1", release(), archive, files, program)
        };
        assert!(new(ArchiveKind::Zip, vec![file("bin/tool", true)], "bin/tool").is_ok());
        let refused = [
            (new(ArchiveKind::Zip, Vec::new(), "tool"), "no files"),
            (
                new(ArchiveKind::Zip, vec![file("tool", true)], "other"),
                "unlisted program",
            ),
            (
                new(ArchiveKind::Zip, vec![file("tool", false)], "tool"),
                "not a program",
            ),
            (
                new(ArchiveKind::Zip, vec![file("../tool", true)], "../tool"),
                "escaping path",
            ),
            (
                new(ArchiveKind::Zip, vec![file("NUL", true)], "NUL"),
                "device name",
            ),
            (
                new(
                    ArchiveKind::Zip,
                    vec![file("tool", true), file("TOOL", false)],
                    "tool",
                ),
                "same file twice",
            ),
            (
                new(
                    ArchiveKind::Plain,
                    vec![file("tool", true), file("lib", false)],
                    "tool",
                ),
                "plain with two files",
            ),
        ];
        for (result, case) in refused {
            assert!(result.is_err(), "{case}");
        }
        assert!(ReleaseFile::new(release().url().clone(), release().sha256(), 0).is_err());
        let mut same_source = vec![file("a", true), file("b", false)];
        same_source[1].from = same_source[0].from.clone();
        assert!(new(ArchiveKind::Zip, same_source, "a").is_err());
    }

    /// The record names parts by these, so a renamed variant would orphan every install.
    #[test]
    fn part_names_are_stable() {
        assert_eq!(
            ComponentId::ALL.map(ComponentId::as_str),
            ["yt-dlp", "ffmpeg", "deno"]
        );
        for id in ComponentId::ALL {
            assert_eq!(
                serde_json::to_string(&id).unwrap(),
                format!("\"{}\"", id.as_str())
            );
        }
        assert!(serde_json::from_str::<ComponentId>("\"../x\"").is_err());
    }
}
