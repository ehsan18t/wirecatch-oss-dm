//! Choosing the final path of a new download. The manager owns naming and runs the
//! engine with `exact_path`, so two downloads never share a file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wirecatch_engine::{PART_EXT, STATE_EXT, Url, naming};

use crate::error::{CoreError, Result};

/// Paths already promised to unfinished downloads, compared the way the file
/// system compares them.
#[derive(Debug, Default, Clone)]
pub(crate) struct Claimed(HashSet<String>);

impl Claimed {
    /// Marks `path` as taken.
    pub fn insert(&mut self, path: &Path) {
        self.0.insert(key(path));
    }

    /// Marks `path` as free again.
    pub fn remove(&mut self, path: &Path) {
        self.0.remove(&key(path));
    }

    /// How many paths are taken.
    #[cfg(debug_assertions)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether `path` is taken.
    pub fn contains(&self, path: &Path) -> bool {
        self.0.contains(&key(path))
    }
}

/// The name a download settles on once the server has answered: the user's
/// choice; else the name the server gave (`Content-Disposition`); else the
/// provisional name, which may come from the browser and know better than the
/// link (a page can name a download the server never names); else the link's.
///
/// `suggested` is the engine's suggestion, which falls back to the name in the
/// link (`final_url`) or a generic name when the server gave none.
pub(crate) fn settled_name(
    choice: Option<&str>,
    suggested: &str,
    final_url: &Url,
    provisional: &str,
) -> Result<String> {
    let from_link = naming::from_url(final_url)
        .map_or_else(|| GENERIC_NAME.to_string(), |name| naming::sanitize(&name));
    let server_named_it = suggested != from_link && suggested != GENERIC_NAME;
    let name = match choice.map(str::trim).filter(|n| !n.is_empty()) {
        Some(chosen) => chosen,
        None if server_named_it || provisional.is_empty() => suggested,
        None => provisional,
    };
    checked(naming::sanitize(name))
}

/// The engine's name for a file when neither the server nor the link offers one.
const GENERIC_NAME: &str = "download";

/// A safe file name from the user's or browser's choice, else the link. Names
/// ending like the engine's own part and resume files are refused, so a download
/// can never be mistaken for another one's partial data.
pub(crate) fn file_name(preferred: Option<&str>, url: &Url) -> Result<String> {
    let raw = preferred
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| naming::from_url(url))
        .unwrap_or_default();
    checked(naming::sanitize(&raw))
}

/// Refuses names ending like the engine's part and resume files.
fn checked(name: String) -> Result<String> {
    let lower = name.to_lowercase();
    if [PART_EXT, STATE_EXT].iter().any(|ext| lower.ends_with(ext)) {
        return Err(CoreError::Invalid(format!(
            "File names cannot end in {PART_EXT} or {STATE_EXT}; Wirecatch uses those for downloads in progress. Choose another name."
        )));
    }
    Ok(name)
}

/// The first of `name`, `name (1)`, `name (2)`... in `dir` that is free on disk
/// (no file, part file or resume file) and not claimed by another download.
/// Touches the file system, so call it off async threads.
pub(crate) fn unique_path(dir: &Path, name: &str, claimed: &Claimed) -> PathBuf {
    let mut n = 0;
    loop {
        let candidate = candidate(dir, name, n);
        if !claimed.contains(&candidate) && is_free_on_disk(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The `n`th name tried for `name` in `dir`: `name` itself, then `name (1)`...
pub(crate) fn candidate(dir: &Path, name: &str, n: u64) -> PathBuf {
    if n == 0 {
        return dir.join(name);
    }
    let (stem, ext) = split_ext(name);
    dir.join(format!("{stem} ({n}){ext}"))
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

/// Whether nothing is at `path`: no file, part file or resume file. Touches the
/// file system.
pub(crate) fn is_free_on_disk(path: &Path) -> bool {
    !path.exists() && partial_files(path).iter().all(|p| !p.exists())
}

/// Splits `archive.tar.gz` into (`archive.tar`, `.gz`). Dotfiles keep their name as the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Whether `a` and `b` name the same file, compared the way claims are.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
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
        assert_eq!(file_name(Some("my:file.zip"), &url).unwrap(), "my_file.zip");
        assert_eq!(file_name(Some("  "), &url).unwrap(), "report final.pdf");
        assert_eq!(file_name(None, &url).unwrap(), "report final.pdf");
        let bare = Url::parse("https://example.com/").unwrap();
        assert_eq!(file_name(None, &bare).unwrap(), "download");
    }

    #[test]
    fn the_settled_name_prefers_choice_then_server_then_browser_then_link() {
        let link = Url::parse("https://example.com/get?id=7").unwrap();
        let settle = |choice, suggested| settled_name(choice, suggested, &link, "report.pdf");
        assert_eq!(settle(Some("mine.pdf"), "server.pdf").unwrap(), "mine.pdf");
        // The server named it (Content-Disposition): that wins over the browser.
        assert_eq!(settle(None, "server.pdf").unwrap(), "server.pdf");
        // Only the link's name or the generic one: the browser's name wins.
        assert_eq!(settle(None, "get").unwrap(), "report.pdf");
        assert_eq!(settle(None, "download").unwrap(), "report.pdf");
        let no_hint = settled_name(None, "get", &link, "").unwrap();
        assert_eq!(no_hint, "get");
        assert!(settle(Some("x.wcpart"), "server.pdf").is_err());
    }

    #[test]
    fn names_of_the_engines_own_files_are_refused() {
        let url = Url::parse("https://example.com/x").unwrap();
        for name in ["a.zip.wcpart", "a.zip.WCSTATE", "b.wcpart"] {
            let err = file_name(Some(name), &url).unwrap_err();
            assert!(err.to_string().contains("Choose another name"), "{err}");
        }
        let from_link = Url::parse("https://example.com/files/movie.mkv.wcpart").unwrap();
        assert!(file_name(None, &from_link).is_err());
        assert!(file_name(Some("notes.wcpart.txt"), &url).is_ok());
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
