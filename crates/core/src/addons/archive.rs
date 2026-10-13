//! Checking a release file against its checksum and taking the expected files
//! out of it. Blocks on the disk; run it off async threads.
//!
//! Only the files the manifest names are written, each to the path the
//! manifest gives, so an archive cannot place anything elsewhere: entry names
//! are compared, never used as paths. Links and devices are refused, and every
//! named file must be present exactly once.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::manifest::{AddonComponent, ArchiveKind};
use wirecatch_engine::units::{GIB, MIB_USIZE};

/// Largest single file accepted from an archive (Deno is about 100 MB).
const MAX_FILE: u64 = GIB;

/// Opens a downloaded release file for checking and unpacking. On Windows no
/// other program may write to, replace or delete it while it is open, so the
/// bytes that are unpacked are the bytes that were checked. (On Unix the open
/// file stays the one checked even if another is renamed over it.)
pub(crate) fn open_release(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Others may read it, nothing else.
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        File::options()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// The SHA-256 of a file, as lower-case hex.
pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    sha256_of(&mut open_release(path)?)
}

/// The SHA-256 of everything in `file`, from its start, as lower-case hex.
pub(crate) fn sha256_of(file: &mut File) -> io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; MIB_USIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0xf)]));
    }
    out
}

/// The installed files of a part.
pub(crate) type Installed = Vec<super::InstalledFile>;

/// Why a release could not be unpacked. Each message is a sentence for a person, and says its cause, so the cause is a field rather than a source.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ArchiveError {
    /// The manifest names no file to install.
    #[error("The add-on manifest names no file.")]
    NoFiles,
    /// The zip archive could not be read.
    #[error("The archive could not be read ({0}).")]
    UnreadableZip(zip::result::ZipError),
    /// The tar archive could not be read.
    #[error("The archive could not be read ({0}).")]
    UnreadableTar(io::Error),
    /// The archive's data is damaged.
    #[error("The archive is damaged ({0}).")]
    Damaged(io::Error),
    /// An entry to install is a folder, link or device instead of a plain file.
    #[error("{0} in the archive is not a plain file.")]
    NotPlainFile(String),
    /// An entry to install appears twice.
    #[error("{0} appears twice in the archive.")]
    Duplicate(String),
    /// A file to install is not in the archive.
    #[error("{0} is missing from the archive.")]
    Missing(String),
    /// A file is larger than any file of the add-on can be.
    #[error("A file in the archive is far larger than expected.")]
    TooLarge,
    /// The manifest names a path that could leave the add-on's folder.
    #[error("The add-on manifest names an unsafe path ({0}).")]
    UnsafePath(String),
    /// Writing the files failed.
    #[error("The add-on could not be written to disk ({0}). Check that the disk has free space.")]
    Disk(io::Error),
}

/// Puts the component's files from the verified release file `release` (the
/// same open file that was checked) into the empty folder `stage`. `Err` says
/// what was wrong, in a sentence.
pub(crate) fn unpack(
    component: &AddonComponent,
    release: &mut File,
    stage: &Path,
) -> Result<Installed, ArchiveError> {
    release
        .seek(SeekFrom::Start(0))
        .map_err(ArchiveError::Disk)?;
    let targets: HashMap<&str, (PathBuf, bool)> = component
        .files
        .iter()
        .map(|f| Ok((f.from.as_str(), (safe_join(stage, &f.to)?, f.executable))))
        .collect::<Result<_, ArchiveError>>()?;
    let mut written: HashMap<String, (u64, String)> = HashMap::new();
    match component.archive {
        ArchiveKind::Plain => {
            let file = component.files.first().ok_or(ArchiveError::NoFiles)?;
            let (target, executable) = &targets[file.from.as_str()];
            let written_file = write_file(release, target, *executable)?;
            written.insert(file.from.clone(), written_file);
        }
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(&mut *release))
                .map_err(ArchiveError::UnreadableZip)?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).map_err(ArchiveError::UnreadableZip)?;
                let name = entry.name().replace('\\', "/");
                let name = name.trim_start_matches("./").to_string();
                let Some((target, executable)) = targets.get(name.as_str()) else {
                    continue;
                };
                if entry.is_dir() || entry.is_symlink() {
                    return Err(ArchiveError::NotPlainFile(name));
                }
                claim(&mut written, &name)?;
                let written_file = write_file(&mut entry, target, *executable)?;
                written.insert(name, written_file);
            }
        }
        ArchiveKind::TarXz => {
            let xz = lzma_rust2::XzReader::new(BufReader::new(&mut *release), true);
            let mut tar = tar::Archive::new(xz);
            let entries = tar.entries().map_err(ArchiveError::UnreadableTar)?;
            for entry in entries {
                let mut entry = entry.map_err(ArchiveError::UnreadableTar)?;
                let name = String::from_utf8_lossy(&entry.path_bytes()).replace('\\', "/");
                let name = name.trim_start_matches("./").to_string();
                let Some((target, executable)) = targets.get(name.as_str()) else {
                    continue;
                };
                if !matches!(
                    entry.header().entry_type(),
                    tar::EntryType::Regular | tar::EntryType::Continuous
                ) {
                    return Err(ArchiveError::NotPlainFile(name));
                }
                claim(&mut written, &name)?;
                let written_file = write_file(&mut entry, target, *executable)?;
                written.insert(name, written_file);
            }
        }
    }
    let mut installed = Installed::new();
    for file in &component.files {
        match written.get(&file.from) {
            Some((size, sha256)) => installed.push(super::InstalledFile {
                path: file.to.clone(),
                size: *size,
                sha256: sha256.clone(),
            }),
            None => return Err(ArchiveError::Missing(file.from.clone())),
        }
    }
    Ok(installed)
}

/// Refuses an entry that appears twice.
fn claim(written: &mut HashMap<String, (u64, String)>, name: &str) -> Result<(), ArchiveError> {
    if written.contains_key(name) {
        return Err(ArchiveError::Duplicate(name.to_string()));
    }
    Ok(())
}

/// `base` joined with the relative path `relative` (`/` separators), refusing
/// anything that could leave `base`: absolute paths, drive letters, `..`.
pub(crate) fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, ArchiveError> {
    let path = Path::new(relative);
    let plain = !relative.is_empty()
        && !relative.contains('\\')
        && !relative.contains(':')
        && path.components().all(|c| matches!(c, Component::Normal(_)));
    if !plain {
        return Err(ArchiveError::UnsafePath(relative.to_string()));
    }
    Ok(base.join(path))
}

/// Writes into a file and hashes what it writes.
struct Hashing {
    file: File,
    hasher: Sha256,
}

impl Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Copies `input` to a new file at `target` with [`copy_file`] and marks it
/// executable or not.
#[cfg(unix)]
fn write_file(
    input: &mut impl Read,
    target: &Path,
    executable: bool,
) -> Result<(u64, String), ArchiveError> {
    use std::os::unix::fs::PermissionsExt;
    let written = copy_file(input, target, MAX_FILE)?;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(target, std::fs::Permissions::from_mode(mode))
        .map_err(ArchiveError::Disk)?;
    Ok(written)
}

/// Copies `input` to a new file at `target` with [`copy_file`]. Windows has no
/// executable bit: a file runs by its name.
#[cfg(not(unix))]
fn write_file(
    input: &mut impl Read,
    target: &Path,
    _executable: bool,
) -> Result<(u64, String), ArchiveError> {
    copy_file(input, target, MAX_FILE)
}

/// Copies `input` to a new file at `target`, at most `limit` bytes ([`MAX_FILE`]).
/// Returns its size and SHA-256.
fn copy_file(
    input: &mut impl Read,
    target: &Path,
    limit: u64,
) -> Result<(u64, String), ArchiveError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(ArchiveError::Disk)?;
    }
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(ArchiveError::Disk)?;
    let mut out = Hashing {
        file,
        hasher: Sha256::new(),
    };
    let copied = io::copy(&mut input.take(limit.saturating_add(1)), &mut out).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            ArchiveError::Damaged(e)
        } else {
            ArchiveError::Disk(e)
        }
    })?;
    if copied > limit {
        return Err(ArchiveError::TooLarge);
    }
    out.flush().map_err(ArchiveError::Disk)?;
    let sha256 = hex(&out.hasher.finalize());
    drop(out.file);
    Ok((copied, sha256))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::manifest::AddonFile;

    #[test]
    fn hashes_are_lower_case_hex() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("abc");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// What is unpacked is what was checked, even if something tries to put
    /// another file in its place in between.
    #[test]
    fn the_file_checked_is_the_file_unpacked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("release");
        std::fs::write(&path, b"original").unwrap();
        let mut release = open_release(&path).unwrap();
        let checked = sha256_of(&mut release).unwrap();
        assert_eq!(checked, hex(&Sha256::digest(b"original")));

        let other = dir.path().join("other");
        std::fs::write(&other, b"replaced").unwrap();
        let replaced = std::fs::rename(&other, &path);
        if cfg!(windows) {
            assert!(
                replaced.is_err(),
                "the checked file was replaced while open"
            );
            assert!(
                File::options().write(true).open(&path).is_err(),
                "the checked file could be written while open"
            );
        }

        let stage = dir.path().join("stage");
        let component = AddonComponent {
            name: "x".into(),
            version: "1".into(),
            url: String::new(),
            sha256: checked,
            size: 8,
            archive: ArchiveKind::Plain,
            files: vec![AddonFile {
                from: String::new(),
                to: "tool".into(),
                executable: true,
            }],
            program: "tool".into(),
        };
        unpack(&component, &mut release, &stage).unwrap();
        assert_eq!(std::fs::read(stage.join("tool")).unwrap(), b"original");
    }

    #[test]
    fn unsafe_target_paths_are_refused() {
        let base = Path::new("/addons");
        for bad in ["../x", "/etc/passwd", "a/../../b", "C:/x", "a\\..\\b", ""] {
            assert!(safe_join(base, bad).is_err(), "{bad}");
        }
        assert!(safe_join(base, "lib/libavcodec.so.63").is_ok());
    }

    #[test]
    fn a_manifest_with_an_unsafe_target_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        std::fs::write(&path, b"tool").unwrap();
        let mut release = open_release(&path).unwrap();
        let stage = dir.path().join("stage");
        let component = AddonComponent {
            name: "x".into(),
            version: "1".into(),
            url: String::new(),
            sha256: String::new(),
            size: 4,
            archive: ArchiveKind::Plain,
            files: vec![AddonFile {
                from: String::new(),
                to: "../escaped".into(),
                executable: true,
            }],
            program: "../escaped".into(),
        };
        assert!(unpack(&component, &mut release, &stage).is_err());
        assert!(!dir.path().join("escaped").exists());
    }

    fn component(archive: ArchiveKind, from: &str) -> AddonComponent {
        AddonComponent {
            name: "x".into(),
            version: "1".into(),
            url: String::new(),
            sha256: String::new(),
            size: 0,
            archive,
            files: vec![AddonFile {
                from: from.into(),
                to: "tool".into(),
                executable: true,
            }],
            program: "tool".into(),
        }
    }

    /// A `.tar.xz` holding `entries`: a name with content is a file, one without is a folder.
    fn tar_xz(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o755);
            let content = content.unwrap_or_default();
            header.set_entry_type(if content.is_empty() {
                tar::EntryType::Directory
            } else {
                tar::EntryType::Regular
            });
            header.set_size(content.len() as u64);
            tar.append_data(&mut header, name, content).unwrap();
        }
        let tar = tar.into_inner().unwrap();
        let mut xz =
            lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
        xz.write_all(&tar).unwrap();
        xz.finish().unwrap()
    }

    fn unpack_bytes(component: &AddonComponent, bytes: &[u8]) -> Result<Installed, ArchiveError> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("release");
        std::fs::write(&path, bytes).unwrap();
        let mut release = open_release(&path).unwrap();
        unpack(component, &mut release, &dir.path().join("stage"))
    }

    #[test]
    fn an_entry_that_appears_twice_is_refused() {
        let bytes = tar_xz(&[("bin/tool", Some(b"one")), ("bin/tool", Some(b"two"))]);
        let error = unpack_bytes(&component(ArchiveKind::TarXz, "bin/tool"), &bytes).unwrap_err();
        assert!(
            matches!(error, ArchiveError::Duplicate(ref n) if n == "bin/tool"),
            "{error}"
        );
    }

    #[test]
    fn a_folder_named_like_an_expected_file_is_refused() {
        let bytes = tar_xz(&[("bin/tool", None)]);
        let error = unpack_bytes(&component(ArchiveKind::TarXz, "bin/tool"), &bytes).unwrap_err();
        assert!(matches!(error, ArchiveError::NotPlainFile(_)), "{error}");
    }

    #[test]
    fn a_zip_entry_written_with_a_leading_dot_is_found() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("./bin/tool", options).unwrap();
        zip.write_all(b"tool").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let installed = unpack_bytes(&component(ArchiveKind::Zip, "bin/tool"), &bytes).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].size, 4);
    }

    #[test]
    fn a_file_past_the_size_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("big");
        let error = copy_file(&mut &b"12345"[..], &target, 4).unwrap_err();
        assert!(matches!(error, ArchiveError::TooLarge), "{error}");
        let fits = dir.path().join("fits");
        assert_eq!(copy_file(&mut &b"1234"[..], &fits, 4).unwrap().0, 4);
    }
}
