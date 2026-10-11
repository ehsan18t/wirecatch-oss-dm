//! Where the media add-on's parts come from: pinned releases with their SHA-256
//! checksums (D20, A8). Nothing is run unless it matched these.

/// The add-on id used by the UI and in settings profiles.
pub const MEDIA_ADDON_ID: &str = "media";

/// One part of the media add-on (yt-dlp, ffmpeg or Deno) as published by its
/// project: where to get it, its checksum, and which files to take from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonComponent {
    /// Folder name under the add-on folder, for example `ffmpeg`.
    pub name: String,
    /// The release, shown to the user and compared on updates.
    pub version: String,
    /// The download address of the release file.
    pub url: String,
    /// The release file's SHA-256, lower-case hex. The file is refused, and
    /// never unpacked or run, when it does not match.
    pub sha256: String,
    /// Size of the release file in bytes, for progress before the server answers.
    pub size: u64,
    /// How the release file is packed.
    pub archive: ArchiveKind,
    /// The files to install. Anything else in the archive is ignored.
    pub files: Vec<AddonFile>,
    /// The program to run, as a path relative to the component's folder (one of
    /// the [`files`](Self::files)' `to`).
    pub program: String,
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
    /// The GitHub API address of yt-dlp's latest release, read at most once a
    /// day to keep up with sites that change; `None` turns that off.
    pub ytdlp_latest_release: Option<String>,
    /// The name of this platform's yt-dlp file in a release, for example
    /// `yt-dlp.exe`.
    pub ytdlp_asset: String,
    /// The name of the checksum list in a yt-dlp release.
    pub ytdlp_checksums_asset: String,
}

impl MediaAddonSources {
    /// The parts in installation order.
    pub fn components(&self) -> [&AddonComponent; 3] {
        [&self.ytdlp, &self.ffmpeg, &self.deno]
    }
}

const YTDLP_VERSION: &str = "2026.08.19";
const FFMPEG_TAG: &str = "autobuild-2026-10-10-13-04";
const FFMPEG_VERSION: &str = "n9.0.2-25-g67b60c310b";
const DENO_VERSION: &str = "2.9.7";

/// The ffmpeg shared libraries `ffmpeg` links against (BtbN LGPL shared build,
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
    /// x86-64): yt-dlp from its GitHub releases, ffmpeg from BtbN's
    /// FFmpeg-Builds (LGPL, shared libraries) and Deno from its GitHub releases.
    /// The checksums were copied from each release's published checksum list.
    pub fn official() -> Self {
        Self {
            ytdlp: official_ytdlp(),
            ffmpeg: official_ffmpeg(),
            deno: official_deno(),
            ytdlp_latest_release: Some(
                "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest".into(),
            ),
            ytdlp_asset: ytdlp_asset().into(),
            ytdlp_checksums_asset: "SHA2-256SUMS".into(),
        }
    }
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

fn official_ytdlp() -> AddonComponent {
    #[cfg(windows)]
    let (sha256, size) = (
        "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a",
        17_840_399,
    );
    #[cfg(not(windows))]
    let (sha256, size) = (
        "58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a",
        40_446_224,
    );
    ytdlp_component(
        YTDLP_VERSION,
        format!(
            "https://github.com/yt-dlp/yt-dlp/releases/download/{YTDLP_VERSION}/{}",
            ytdlp_asset()
        ),
        sha256,
        size,
    )
}

/// The yt-dlp component for one release file.
pub(crate) fn ytdlp_component(
    version: &str,
    url: String,
    sha256: &str,
    size: u64,
) -> AddonComponent {
    let program = program_name("yt-dlp");
    AddonComponent {
        name: "yt-dlp".into(),
        version: version.into(),
        url,
        sha256: sha256.to_ascii_lowercase(),
        size,
        archive: ArchiveKind::Plain,
        files: vec![AddonFile {
            from: String::new(),
            to: program.clone(),
            executable: true,
        }],
        program,
    }
}

fn official_ffmpeg() -> AddonComponent {
    #[cfg(windows)]
    let (asset, sha256, size, archive) = (
        "win64-lgpl-shared-9.0.zip",
        "a68c86127a4185bcb9035d39d54f8b6873681896b860af999d8987be3c696de4",
        77_230_479,
        ArchiveKind::Zip,
    );
    #[cfg(not(windows))]
    let (asset, sha256, size, archive) = (
        "linux64-lgpl-shared-9.0.tar.xz",
        "7596b51ac2303f596a32b1a4f09f98dc0196415aa16dffd153cfab6d2ea5f4c9",
        63_523_780,
        ArchiveKind::TarXz,
    );
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
    AddonComponent {
        name: "ffmpeg".into(),
        version: FFMPEG_VERSION.into(),
        url: format!(
            "https://github.com/BtbN/FFmpeg-Builds/releases/download/{FFMPEG_TAG}/ffmpeg-{FFMPEG_VERSION}-{asset}"
        ),
        sha256: sha256.into(),
        size,
        archive,
        files,
        program,
    }
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
    let (target, sha256, size) = (
        "x86_64-pc-windows-msvc",
        "a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238",
        42_630_221,
    );
    #[cfg(not(windows))]
    let (target, sha256, size) = (
        "x86_64-unknown-linux-gnu",
        "c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490",
        41_596_794,
    );
    let program = program_name("deno");
    AddonComponent {
        name: "deno".into(),
        version: DENO_VERSION.into(),
        url: format!(
            "https://github.com/denoland/deno/releases/download/v{DENO_VERSION}/deno-{target}.zip"
        ),
        sha256: sha256.into(),
        size,
        archive: ArchiveKind::Zip,
        files: vec![AddonFile {
            from: program.clone(),
            to: program.clone(),
            executable: true,
        }],
        program,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_sources_are_pinned_and_consistent() {
        let sources = MediaAddonSources::official();
        for c in sources.components() {
            assert_eq!(c.sha256.len(), 64, "{}", c.name);
            assert!(
                c.sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert!(c.url.starts_with("https://github.com/"), "{}", c.url);
            assert!(c.size > 1_000_000);
            assert!(c.files.iter().any(|f| f.to == c.program && f.executable));
        }
        assert_eq!(sources.ffmpeg.files.len(), 8);
        assert!(sources.ffmpeg.url.contains(FFMPEG_TAG));
        assert!(sources.ffmpeg.url.contains("lgpl-shared"));
    }
}
