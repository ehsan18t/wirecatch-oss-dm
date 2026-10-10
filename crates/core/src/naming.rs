//! Choosing the final path of a new download. The manager owns naming and runs the
//! engine with `exact_path`, so two downloads never share a file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wirecatch_engine::{Url, naming};

/// Paths already promised to unfinished downloads, compared the way the file
/// system compares them.
#[derive(Debug, Default)]
pub(crate) struct Claimed(HashSet<String>);

impl Claimed {
    /// Marks `path` as taken.
    pub fn insert(&mut self, path: &Path) {
        self.0.insert(key(path));
    }

    /// Whether `path` is taken.
    pub fn contains(&self, path: &Path) -> bool {
        self.0.contains(&key(path))
    }
}

/// A safe file name from the user's or browser's choice, else the link.
pub(crate) fn file_name(preferred: Option<&str>, url: &Url) -> String {
    let raw = preferred
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| naming::from_url(url))
        .unwrap_or_default();
    naming::sanitize(&raw)
}

/// The first of `name`, `name (1)`, `name (2)`... in `dir` that is free on disk
/// (no file, part file or resume file) and not claimed by another download.
/// Touches the file system, so call it off async threads.
pub(crate) fn unique_path(dir: &Path, name: &str, claimed: &Claimed) -> PathBuf {
    let (stem, ext) = split_ext(name);
    let mut n = 0u64;
    loop {
        let candidate = if n == 0 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({n}){ext}"))
        };
        if !claimed.contains(&candidate) && is_free_on_disk(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The part and resume files the engine keeps next to `path` while downloading.
pub(crate) fn partial_files(path: &Path) -> [PathBuf; 2] {
    let (part, state) = naming::part_paths(path);
    [part, state]
}

/// The size of the file at `path` if it is a finished download: the file is there
/// and neither its part file nor its resume file is. Touches the file system.
pub(crate) fn finished_size(path: &Path) -> Option<u64> {
    if partial_files(path).iter().any(|p| p.exists()) {
        return None;
    }
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

fn is_free_on_disk(path: &Path) -> bool {
    !path.exists() && partial_files(path).iter().all(|p| !p.exists())
}

/// Splits `archive.tar.gz` into (`archive.tar`, `.gz`). Dotfiles keep their name as the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Windows file names are case-insensitive; Linux ones are not.
fn key(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_prefers_the_given_one_then_the_link() {
        let url = Url::parse("https://example.com/files/report%20final.pdf?x=1").unwrap();
        assert_eq!(file_name(Some("my:file.zip"), &url), "my_file.zip");
        assert_eq!(file_name(Some("  "), &url), "report final.pdf");
        assert_eq!(file_name(None, &url), "report final.pdf");
        let bare = Url::parse("https://example.com/").unwrap();
        assert_eq!(file_name(None, &bare), "download");
    }

    #[test]
    fn unique_path_skips_disk_and_claimed_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.zip"), b"x").unwrap();
        std::fs::write(dir.path().join("a (1).zip.wcpart"), b"x").unwrap();
        let mut claimed = Claimed::default();
        claimed.insert(&dir.path().join("a (2).zip"));
        assert_eq!(
            unique_path(dir.path(), "a.zip", &claimed),
            dir.path().join("a (3).zip")
        );
        assert_eq!(
            unique_path(dir.path(), ".bashrc", &claimed),
            dir.path().join(".bashrc")
        );
    }

    #[cfg(windows)]
    #[test]
    fn claims_ignore_case_on_windows() {
        let mut claimed = Claimed::default();
        claimed.insert(Path::new(r"C:\D\File.ZIP"));
        assert!(claimed.contains(Path::new(r"c:\d\file.zip")));
    }
}
