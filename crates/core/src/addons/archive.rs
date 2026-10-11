//! Checking a release file against its checksum and taking the expected files
//! out of it. Blocks on the disk; run it off async threads.
//!
//! Only the files the manifest names are written, each to the path the
//! manifest gives, so an archive cannot place anything elsewhere: entry names
//! are compared, never used as paths. Links and devices are refused, and every
//! named file must be present exactly once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::manifest::{AddonComponent, ArchiveKind};

/// Largest single file accepted from an archive (Deno is about 100 MB).
const MAX_FILE: u64 = 1024 * 1024 * 1024;

/// The SHA-256 of a file, as lower-case hex.
pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
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

/// One installed file: its path relative to the component folder and its size.
pub(crate) type Installed = Vec<(String, u64)>;

/// Puts the component's files from the verified release file `release` into
/// the empty folder `stage`. `Err` says what was wrong, in a sentence.
pub(crate) fn unpack(
    component: &AddonComponent,
    release: &Path,
    stage: &Path,
) -> Result<Installed, String> {
    let targets: HashMap<&str, (PathBuf, bool)> = component
        .files
        .iter()
        .map(|f| Ok((f.from.as_str(), (safe_join(stage, &f.to)?, f.executable))))
        .collect::<Result<_, String>>()?;
    let mut written: HashMap<String, u64> = HashMap::new();
    match component.archive {
        ArchiveKind::Plain => {
            let file = component
                .files
                .first()
                .ok_or_else(|| "The add-on manifest names no file.".to_string())?;
            let (target, executable) = &targets[file.from.as_str()];
            let mut input = File::open(release).map_err(|e| disk(&e))?;
            let size = write_file(&mut input, target, *executable)?;
            written.insert(file.from.clone(), size);
        }
        ArchiveKind::Zip => {
            let input = File::open(release).map_err(|e| disk(&e))?;
            let mut zip = zip::ZipArchive::new(BufReader::new(input))
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
                let size = write_file(&mut entry, target, *executable)?;
                written.insert(name, size);
            }
        }
        ArchiveKind::TarXz => {
            let input = File::open(release).map_err(|e| disk(&e))?;
            let xz = lzma_rust2::XzReader::new(BufReader::new(input), true);
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
                let size = write_file(&mut entry, target, *executable)?;
                written.insert(name, size);
            }
        }
    }
    let mut installed = Installed::new();
    for file in &component.files {
        match written.get(&file.from) {
            Some(size) => installed.push((file.to.clone(), *size)),
            None => return Err(format!("{} is missing from the archive.", file.from)),
        }
    }
    Ok(installed)
}

/// Refuses an entry that appears twice.
fn claim(written: &mut HashMap<String, u64>, name: &str) -> Result<(), String> {
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

/// Copies `input` to a new file at `target`, at most [`MAX_FILE`] bytes.
fn write_file(input: &mut impl Read, target: &Path, executable: bool) -> Result<u64, String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| disk(&e))?;
    }
    let mut out = File::options()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| disk(&e))?;
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
    set_mode(target, executable).map_err(|e| disk(&e))?;
    Ok(copied)
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
        let release = dir.path().join("tool");
        std::fs::write(&release, b"tool").unwrap();
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
        assert!(unpack(&component, &release, &stage).is_err());
        assert!(!dir.path().join("escaped").exists());
    }
}
