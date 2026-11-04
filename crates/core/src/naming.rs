//! Choosing the final path of a new download. The manager owns naming and runs the
//! engine with `exact_path`, so two downloads never share a file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use wirecatch_engine::{PART_EXT, STATE_EXT, Url, naming};
use wirecatch_media::{stream_dir, work_dir};
use wirecatch_protocol::DownloadId;

use crate::error::{CoreError, Result};

/// Which download holds each path, compared the way the file system compares them. Only the holder can give a path up, and a path another download holds is never taken over.
///
/// Besides each unfinished download's own path, a download may reserve one: a question about the file there, a move to another folder, the server's answer choosing a name, a finished video being put in place. Those are counted apart, so the consistency check can still demand that every other claim is an unfinished download's path.
#[derive(Debug, Default, Clone)]
pub(crate) struct Claimed {
    holders: HashMap<String, Claim>,
}

#[derive(Debug, Clone, Copy)]
struct Claim {
    holder: DownloadId,
    reserved: bool,
}

impl Claimed {
    /// Makes `path` `id`'s own, also when it had reserved it; a path another download holds stays with that download.
    pub fn insert(&mut self, path: &Path, id: DownloadId) {
        let claim = self.holders.entry(key(path)).or_insert(Claim {
            holder: id,
            reserved: false,
        });
        if claim.holder == id {
            claim.reserved = false;
        }
    }

    /// Gives up `path`, own or reserved, if `id` holds it.
    pub fn remove(&mut self, path: &Path, id: DownloadId) {
        let key = key(path);
        if self.holders.get(&key).is_some_and(|c| c.holder == id) {
            self.holders.remove(&key);
        }
    }

    /// Reserves `path` for `id`, unless another download holds it. A path already `id`'s own stays its own.
    #[must_use]
    pub fn reserve(&mut self, path: &Path, id: DownloadId) -> bool {
        self.holders
            .entry(key(path))
            .or_insert(Claim {
                holder: id,
                reserved: true,
            })
            .holder
            == id
    }

    /// Gives back `path` if `id` holds it as a reservation; a path that is `id`'s own stays.
    pub fn release(&mut self, path: &Path, id: DownloadId) {
        let key = key(path);
        if self
            .holders
            .get(&key)
            .is_some_and(|c| c.holder == id && c.reserved)
        {
            self.holders.remove(&key);
        }
    }

    pub fn holder(&self, path: &Path) -> Option<DownloadId> {
        self.holders.get(&key(path)).map(|c| c.holder)
    }

    pub fn held_by_another(&self, path: &Path, id: DownloadId) -> bool {
        self.holder(path).is_some_and(|holder| holder != id)
    }

    /// How many paths are taken by downloads, not counting reservations.
    #[cfg(debug_assertions)]
    pub fn len(&self) -> usize {
        self.holders.values().filter(|c| !c.reserved).count()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.holders.contains_key(&key(path))
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
        // The user can choose again, so their name is refused rather than changed.
        Some(chosen) => return checked(naming::sanitize(chosen)),
        None if server_named_it || provisional.is_empty() => suggested,
        None => provisional,
    };
    // A server cannot be asked for another name: change it instead, or the
    // download would fail on every try.
    let mut name = naming::sanitize(name);
    while checked(name.clone()).is_err() {
        name.push('_');
    }
    Ok(name)
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
/// (no file, part file or resume file) and not `claimed` by another download.
/// Touches the file system, so call it off async threads.
pub(crate) fn unique_path(dir: &Path, name: &str, claimed: impl Fn(&Path) -> bool) -> PathBuf {
    let mut n = 0;
    loop {
        let candidate = candidate(dir, name, n);
        if !claimed(&candidate) && is_free_on_disk(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The first of `name`, `name (1)`... in `dir` that no other download `claimed`. A file already there is left for the download to decide about when its name is final or its file is put in place (D96).
pub(crate) fn unclaimed_path(dir: &Path, name: &str, claimed: impl Fn(&Path) -> bool) -> PathBuf {
    let mut n = 0;
    loop {
        let candidate = candidate(dir, name, n);
        if !claimed(&candidate) {
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

/// The size of the file at `path` if it is a finished download: the file is there and its part file is not. A resume file alone does not count: the engine renames the part file first and deletes the resume file after. Touches the file system.
pub(crate) fn finished_size(path: &Path) -> Option<u64> {
    let [part, _] = partial_files(path);
    if part.exists() {
        return None;
    }
    // A media download still has its work folders until its file is complete.
    if [work_dir(path), stream_dir(path)]
        .iter()
        .any(|p| p.exists())
    {
        return None;
    }
    file_size(path)
}

/// The size of the file at `path`, if one is there. Touches the file system.
pub(crate) fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
}

/// Whether the only things at `path` are a part file and a resume file that a
/// download of `url` left there, with no finished file: what a run leaves when the
/// app stops right after the server answered, before the chosen path was saved.
/// The download may take that path again and continue. Touches the file system.
pub(crate) fn left_by(path: &Path, url: &str) -> bool {
    let [part, state] = partial_files(path);
    if path.exists() || !part.is_file() {
        return false;
    }
    // A kill before its resume file was saved: nothing of it can continue, and the engine starts over.
    !state.exists() || resumes_link(&state, url)
}

/// Deletes the part and resume files beside `path` unless they are a run of `url`. The engine continues whatever part file is at its exact path, so another link's would end up in this download's file. Touches the file system.
pub(crate) fn discard_partials_of_other_links(path: &Path, url: &str) {
    let [part, state] = partial_files(path);
    if !resumes_link(&state, url) {
        wirecatch_files::discard::file(&part);
        wirecatch_files::discard::file(&state);
    }
}

fn resumes_link(state: &Path, url: &str) -> bool {
    wirecatch_engine::resume_file_url(state)
        .is_some_and(|saved| normalized_url(&saved) == normalized_url(url))
}

/// In the form a resume file saves. Not `without_credentials`: the user name tells two accounts' files apart.
fn normalized_url(url: &str) -> String {
    Url::parse(url).map_or_else(
        |_| url.to_string(),
        |u| wirecatch_engine::url::without_password(&u).to_string(),
    )
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

/// The file name of `path`, without its folder.
pub(crate) fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `a` and `b` name the same file, compared the way claims are.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// Windows file names are case-insensitive; Linux ones are not.
pub(crate) fn key(path: &Path) -> String {
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
    fn a_part_file_with_no_resume_file_leaves_its_name_usable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let [part, state] = partial_files(&path);
        std::fs::write(&part, b"cut off by a kill").unwrap();
        assert!(left_by(&path, "https://example.com/a.bin"));

        std::fs::write(&state, b"not a resume file").unwrap();
        assert!(!left_by(&path, "https://example.com/a.bin"));
        std::fs::remove_file(&state).unwrap();

        std::fs::write(&path, b"a finished file").unwrap();
        assert!(!left_by(&path, "https://example.com/a.bin"));
    }

    #[test]
    fn only_the_holder_gives_up_a_path() {
        let (path, one, two) = (Path::new("a.zip"), DownloadId(1), DownloadId(2));
        let mut claimed = Claimed::default();
        claimed.insert(path, one);
        claimed.insert(path, two);
        assert!(!claimed.reserve(path, two));
        claimed.remove(path, two);
        claimed.release(path, two);
        assert_eq!(claimed.holder(path), Some(one));
        assert!(claimed.held_by_another(path, two));
        assert!(!claimed.held_by_another(path, one));
        claimed.remove(path, one);
        assert_eq!(claimed.holder(path), None);
    }

    #[test]
    fn a_reservation_never_costs_a_download_its_own_path() {
        let (path, one) = (Path::new("a.zip"), DownloadId(1));
        let mut claimed = Claimed::default();
        claimed.insert(path, one);
        assert!(claimed.reserve(path, one));
        claimed.release(path, one);
        assert_eq!(claimed.holder(path), Some(one));
    }

    #[test]
    fn a_reservation_becomes_the_downloads_own_path() {
        let (path, one) = (Path::new("a.zip"), DownloadId(1));
        let mut claimed = Claimed::default();
        assert!(claimed.reserve(path, one));
        claimed.insert(path, one);
        claimed.release(path, one);
        assert_eq!(claimed.holder(path), Some(one));
    }

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

    /// The user can pick another name; a server cannot be asked to. Its name is
    /// changed instead of failing the download on every try.
    #[test]
    fn a_server_name_like_the_engines_own_files_is_changed() {
        let link = Url::parse("https://example.com/get?id=7").unwrap();
        for (server, kept) in [
            ("movie.mkv.wcpart", "movie.mkv.wcpart_"),
            ("state.WCSTATE", "state.WCSTATE_"),
        ] {
            assert_eq!(settled_name(None, server, &link, "").unwrap(), kept);
            assert_eq!(settled_name(None, server, &link, "a.pdf").unwrap(), kept);
        }
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
        claimed.insert(&dir.path().join("a (2).zip"), DownloadId(1));
        assert_eq!(
            unique_path(dir.path(), "a.zip", |p| claimed.contains(p)),
            dir.path().join("a (3).zip")
        );
        assert_eq!(
            unique_path(dir.path(), ".bashrc", |p| claimed.contains(p)),
            dir.path().join(".bashrc")
        );
    }

    /// The engine saves a link without its password; the row keeps it.
    #[test]
    fn partial_files_of_a_link_with_a_password_are_taken_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.zip");
        let [part, state] = partial_files(&path);
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(
            &state,
            br#"{"version":1,"url":"https://ada@example.com/a.zip","total":1,"ranged":true,"remaining":[]}"#,
        )
        .unwrap();
        assert!(left_by(&path, "https://ada:secret@example.com/a.zip"));
        assert!(!left_by(&path, "https://ada:secret@example.com/other.zip"));
    }

    /// One account's partial file is not another's, as the engine also holds.
    #[test]
    fn partial_files_of_another_account_are_not_taken_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        let [part, state] = partial_files(&path);
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(
            &state,
            br#"{"version":1,"url":"ftp://alice@host/f","total":1,"ranged":true,"remaining":[]}"#,
        )
        .unwrap();
        assert!(!left_by(&path, "ftp://bob:pw@host/f"));
        assert!(left_by(&path, "ftp://alice:pw@host/f"));
    }

    #[test]
    fn only_partial_files_of_the_same_link_are_taken_up_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.zip");
        let [part, state] = partial_files(&path);
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(
            &state,
            br#"{"version":1,"url":"https://example.com/a.zip","total":1,"ranged":true,"remaining":[]}"#,
        )
        .unwrap();
        assert!(left_by(&path, "https://example.com/a.zip"));
        assert!(!left_by(&path, "https://example.com/other.zip"));
        std::fs::write(&path, b"finished").unwrap();
        assert!(!left_by(&path, "https://example.com/a.zip"));
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&state, b"not json").unwrap();
        assert!(!left_by(&path, "https://example.com/a.zip"));
    }

    #[cfg(windows)]
    #[test]
    fn claims_ignore_case_on_windows() {
        let mut claimed = Claimed::default();
        claimed.insert(Path::new(r"C:\D\File.ZIP"), DownloadId(1));
        assert!(claimed.contains(Path::new(r"c:\d\file.zip")));
    }
}
