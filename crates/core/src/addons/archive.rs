//! Checking a release file against its checksum and taking the expected files
//! out of it. Blocks on the disk; run it off async threads.
//!
//! Only the files the manifest names are written, each to the path the
//! manifest gives, so an archive cannot place anything elsewhere: entry names
//! are compared, never used as paths. Links and devices are refused, and every
//! named file must be present exactly once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::manifest::{AddonComponent, ArchiveKind};

/// Largest single file accepted from an archive (Deno is about 100 MB).
const MAX_FILE: u64 = 1024 * 1024 * 1024;

/// Opens a downloaded release file for checking and unpacking. On Windows no
/// other program may write to, replace or delete it while it is open, so the
/// bytes that are unpacked are the bytes that were checked. (On Unix the open
/// file stays the one checked even if another is renamed over it.)
pub(crate) fn open_release(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// Others may read it, nothing else.
        const FILE_SHARE_READ: u32 = 0x1;
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
    let mut buf = vec![0u8; 1024 * 1024];
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

/// Puts the component's files from the verified release file `release` (the
/// same open file that was checked) into the empty folder `stage`. `Err` says
/// what was wrong, in a sentence.
pub(crate) fn unpack(
    component: &AddonComponent,
    release: &mut File,
    stage: &Path,
) -> Result<Installed, String> {
    release.seek(SeekFrom::Start(0)).map_err(|e| disk(&e))?;
    let targets: HashMap<&str, (PathBuf, bool)> = component
        .files
        .iter()
        .map(|f| Ok((f.from.as_str(), (safe_join(stage, &f.to)?, f.executable))))
        .collect::<Result<_, String>>()?;
    let mut written: HashMap<String, (u64, String)> = HashMap::new();
    match component.archive {
        ArchiveKind::Plain => {
            let file = component
                .files
                .first()
                .ok_or_else(|| "The add-on manifest names no file.".to_string())?;
            let (target, executable) = &targets[file.from.as_str()];
            let written_file = write_file(release, target, *executable)?;
            written.insert(file.from.clone(), written_file);
        }
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(&mut *release))
                .map_err(|e| format!("The archive could not be read ({e})."))?;
            for index in 0..zip.len() {
                let mut entry = zip
                    .by_index(index)
                    .map_err(|e| format!("The archive could not be read ({e})."))?;
                let name = entry.name().replace('\\', "/");
                let Some((target, executable)) = targets.get(name.as_str()) else {
                    continue;
                };
                if entry.is_dir() || entry.is_symlink() {
                    return Err(format!("{name} in the archive is not a plain file."));
                }
                claim(&mut written, &name)?;
                let written_file = write_file(&mut entry, target, *executable)?;
                written.insert(name, written_file);
            }
        }
        ArchiveKind::TarXz => {
            let xz = lzma_rust2::XzReader::new(BufReader::new(&mut *release), true);
            let mut tar = tar::Archive::new(xz);
            let entries = tar
                .entries()
                .map_err(|e| format!("The archive could not be read ({e})."))?;
            for entry in entries {
                let mut entry =
                    entry.map_err(|e| format!("The archive could not be read ({e})."))?;
                let name = String::from_utf8_lossy(&entry.path_bytes()).replace('\\', "/");
                let name = name.trim_start_matches("./").to_string();
                let Some((target, executable)) = targets.get(name.as_str()) else {
                    continue;
                };
                if !matches!(
                    entry.header().entry_type(),
                    tar::EntryType::Regular | tar::EntryType::Continuous
                ) {
                    return Err(format!("{name} in the archive is not a plain file."));
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
            None => return Err(format!("{} is missing from the archive.", file.from)),
        }
    }
    Ok(installed)
}

/// Refuses an entry that appears twice.
fn claim(written: &mut HashMap<String, (u64, String)>, name: &str) -> Result<(), String> {
    if written.contains_key(name) {
        return Err(format!("{name} appears twice in the archive."));
    }
    Ok(())
}

/// `base` joined with the relative path `relative` (`/` separators), refusing
/// anything that could leave `base`: absolute paths, drive letters, `..`.
pub(crate) fn safe_join(base: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    let plain = !relative.is_empty()
        && !relative.contains('\\')
        && !relative.contains(':')
        && path.components().all(|c| matches!(c, Component::Normal(_)));
    if !plain {
        return Err(format!(
            "The add-on manifest names an unsafe path ({relative})."
        ));
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

/// Copies `input` to a new file at `target`, at most [`MAX_FILE`] bytes.
/// Returns its size and SHA-256.
fn write_file(
    input: &mut impl Read,
    target: &Path,
    executable: bool,
) -> Result<(u64, String), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| disk(&e))?;
    }
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| disk(&e))?;
    let mut out = Hashing {
        file,
        hasher: Sha256::new(),
    };
    let copied = io::copy(&mut input.take(MAX_FILE + 1), &mut out).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            format!("The archive is damaged ({e}).")
        } else {
            disk(&e)
        }
    })?;
    if copied > MAX_FILE {
        return Err("A file in the archive is far larger than expected.".into());
    }
    out.flush().map_err(|e| disk(&e))?;
    let sha256 = hex(&out.hasher.finalize());
    drop(out.file);
    set_mode(target, executable).map_err(|e| disk(&e))?;
    Ok((copied, sha256))
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

fn disk(error: &io::Error) -> String {
    format!(
        "The add-on could not be written to disk ({error}). Check that the disk has free space."
    )
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
}
