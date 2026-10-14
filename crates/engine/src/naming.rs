//! Picking a safe, non-clashing file name for a download.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::storage::{self, PART_EXT, STATE_EXT};

/// Most bytes in one file name on Linux (`NAME_MAX`). Windows counts 255
/// UTF-16 units instead, which a name of at most 255 bytes never exceeds.
const SYSTEM_NAME_BYTES: usize = 255;
/// Most bytes in a name the engine takes: its part and resume files, named
/// after it, fit too.
const MAX_NAME_BYTES: usize = SYSTEM_NAME_BYTES - longer(PART_EXT, STATE_EXT).len();
/// Most bytes [`sanitize`] leaves, short of [`MAX_NAME_BYTES`] by room for
/// what is added to a name after it: ` (1)` to make it free, or a caller's
/// extension, such as a stream's container.
const SANITIZED_NAME_BYTES: usize = 200;
const FALLBACK_NAME: &str = "download";
/// Longest text after the last dot that still counts as an extension and is
/// kept whole when a name is shortened. Real extensions are a few bytes; a
/// longer "extension" is part of the name and is shortened with it.
const MAX_EXTENSION_BYTES: usize = 32;

/// Extracts the file name from a `Content-Disposition` header value.
/// `filename*` (RFC 6266 / 5987) wins over plain `filename`.
#[must_use]
pub(crate) fn from_content_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for part in split_params(value) {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        if key == "filename*" {
            // charset'language'percent-encoded
            let encoded = val.splitn(3, '\'').nth(2).unwrap_or(val);
            let decoded = percent_decode_str(encoded.trim_matches('"')).decode_utf8_lossy();
            if !decoded.trim().is_empty() {
                return Some(decoded.into_owned());
            }
        } else if key == "filename" {
            let unquoted = val.trim_matches('"').replace("\\\"", "\"");
            if !unquoted.trim().is_empty() {
                plain = Some(unquoted);
            }
        }
    }
    plain
}

/// Splits on `;` while respecting double quotes.
fn split_params(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let bytes = value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' if i == 0 || bytes[i - 1] != b'\\' => in_quotes = !in_quotes,
            b';' if !in_quotes => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

/// The last non-empty path segment of the URL, percent-decoded.
#[must_use]
pub fn from_url(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    (!decoded.trim().is_empty()).then(|| decoded.into_owned())
}

/// Makes a name safe on both Windows and Linux, and at most 200 bytes long,
/// which leaves room for ` (1)` or another extension within every system's
/// limit. Whatever it returns, it returns unchanged when given it again, and
/// [`is_safe_name`] takes it.
#[must_use]
pub fn sanitize(name: &str) -> String {
    let cut = truncate_keeping_extension(&clean(name), SANITIZED_NAME_BYTES);
    // After the cut, which can leave a device name: `CON` followed by spaces.
    if is_windows_reserved(&cut) {
        truncate_keeping_extension(&format!("_{cut}"), SANITIZED_NAME_BYTES)
    } else {
        cut
    }
}

/// Whether `name` is one ordinary file in a folder on Windows and on Linux,
/// so the engine takes it as it is: no folder part, no character or ending
/// [`sanitize`] replaces or trims (such as a `:`, which names a stream inside
/// another file on Windows, or a trailing dot or space, which Windows drops),
/// not a Windows device such as `NUL` or `COM1.txt`, and short enough that the
/// system takes it with the engine's part and resume files beside it. Any
/// length up to that is taken, unlike what `sanitize` returns: a caller adds
/// ` (1)` or another extension to its names.
#[must_use]
pub fn is_safe_name(name: &str) -> bool {
    name.len() <= MAX_NAME_BYTES && !is_windows_reserved(name) && clean(name) == name
}

/// `name` without a folder part, with every character Windows or Linux refuses
/// in a name, or that disguises one, replaced, and without the leading
/// whitespace and trailing dots and spaces Windows drops; `download` when
/// nothing is left. At any length, and possibly a device name.
fn clean(name: &str) -> String {
    // Drop any directory part a server may have sent.
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let out: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            // Invisible format characters can make `exe.mp4` display as `mp4.exe`.
            c if is_format_char(c) => '_',
            c => c,
        })
        .collect();
    // Windows rejects trailing dots and spaces.
    let out = out.trim_end_matches(['.', ' ']).trim_start();
    if out.is_empty() || out == "." || out == ".." {
        FALLBACK_NAME.to_string()
    } else {
        out.to_string()
    }
}

/// Whether `c` is a Unicode format character (general category Cf): the bidirectional marks,
/// embeddings, overrides and isolates (U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069)
/// that can make a file name display in a different order than it is, and the zero-width and
/// other invisible characters that can hide part of it.
fn is_format_char(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// Whether Windows reads `name` as a device rather than a file: `CON`, `PRN`, `AUX`, `NUL`,
/// `CONIN$`, `CONOUT$`, and `COM` or `LPT` followed by a digit, including the superscript
/// digits `¹`, `²` and `³`, whatever the case and with any extension (`NUL.txt` is `NUL`).
fn is_windows_reserved(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(number) = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{B9}' | '\u{B2}' | '\u{B3}'), None)
    )
}

/// `name` cut to at most `max` bytes, on a character boundary, keeping its
/// extension unless that is longer than [`MAX_EXTENSION_BYTES`].
fn truncate_keeping_extension(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match split_ext(name) {
        (stem, ext) if ext.len() <= MAX_EXTENSION_BYTES && ext.len() < max => (stem, ext),
        _ => (name, ""),
    };
    let mut cut = (max - ext.len()).min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    // Windows refuses a name that ends in a dot or a space, which a cut can leave.
    let stem = match stem[..cut].trim_end_matches(['.', ' ']) {
        "" => FALLBACK_NAME,
        stem => stem,
    };
    format!("{stem}{ext}")
}

/// Splits `archive.tar.gz` into (`archive.tar`, `.gz`). Dotfiles keep their name as the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Paths of the in-progress data file and its resume state for a final path.
#[must_use]
pub fn part_paths(final_path: &Path) -> (PathBuf, PathBuf) {
    let mut part = final_path.as_os_str().to_owned();
    part.push(PART_EXT);
    let mut state = final_path.as_os_str().to_owned();
    state.push(STATE_EXT);
    (PathBuf::from(part), PathBuf::from(state))
}

/// Moves the finished file `from` onto the first free name of `name`,
/// `name (1)`, `name (2)`... in `dir`, and returns where it went. Never
/// replaces anything: the move itself refuses a name that is taken (see
/// below), so a file another program saves under the name at the same moment,
/// or another caller racing for it, is never lost, and each racer lands on its
/// own name. A name is also passed over while a download's part or resume file
/// for it is there ([`part_paths`]). Every name passed over belongs to
/// something in `dir`, so the search ends after at most one more try than
/// `dir` has entries. Blocks on the file system.
///
/// The move is one step that fails when the name exists: `MoveFileExW` without
/// `MOVEFILE_REPLACE_EXISTING` on Windows, and `renameat2` with
/// `RENAME_NOREPLACE` on Linux, which local file systems take, FAT32 and exFAT
/// included. Where the kernel or the file system refuses that flag (some
/// network and FUSE file systems, or a sandbox that blocks the call), and on
/// other Unix systems, a hard link to the new name, which fails when the name
/// exists, then the old name removed. Where hard links are missing too (an SMB
/// mount without Unix extensions, some FUSE file systems), the name is checked
/// and then the file renamed, which leaves a moment in which a file saved under
/// the name is replaced. `from` and `dir` must be on the same volume.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] when [`is_safe_name`] refuses `name`: it is
/// then not one ordinary file in `dir` on every system, so it could not be
/// taken reliably. Otherwise the error moving the file, such as `from` missing
/// or a folder that cannot be written; `from` is then where it was.
pub fn move_to_free_name(from: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    check_name(name)?;
    let mut candidates = candidates(dir, name);
    loop {
        let candidate = candidates.next_name();
        if held_by_download(&candidate) {
            continue;
        }
        match storage::rename_no_replace(from, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Creates, empty, the part file of the first name of `name`, `name (1)`... in
/// `dir` that has no file, part file or resume file, and returns that name's
/// final path with the open part file. The part file is created only if it is
/// not there yet (`create_new`), so downloads racing for a name, in this
/// process or another, each get their own. Blocks on the file system.
pub(crate) fn create_free_part(dir: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    check_name(name)?;
    let mut candidates = candidates(dir, name);
    loop {
        let candidate = candidates.next_name();
        let (part, state) = part_paths(&candidate);
        if fs::symlink_metadata(&candidate).is_ok() || fs::symlink_metadata(&state).is_ok() {
            continue;
        }
        match OpenOptions::new().write(true).create_new(true).open(&part) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// `InvalidInput` unless [`is_safe_name`] takes `name`.
fn check_name(name: &str) -> io::Result<()> {
    if is_safe_name(name) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} is not a safe file name"),
        ))
    }
}

/// The longer of `a` and `b`.
const fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

/// Whether a download's part or resume file for `path` is there.
fn held_by_download(path: &Path) -> bool {
    let (part, state) = part_paths(path);
    fs::symlink_metadata(part).is_ok() || fs::symlink_metadata(state).is_ok()
}

/// The paths `name`, `name (1)`, `name (2)`... in a folder.
struct Candidates<'a> {
    dir: &'a Path,
    name: &'a str,
    next: u64,
}

fn candidates<'a>(dir: &'a Path, name: &'a str) -> Candidates<'a> {
    Candidates { dir, name, next: 0 }
}

impl Candidates<'_> {
    fn next_name(&mut self) -> PathBuf {
        let n = self.next;
        self.next += 1;
        if n == 0 {
            return self.dir.join(self.name);
        }
        let (stem, ext) = match split_ext(self.name) {
            (stem, ext) if ext.len() <= MAX_EXTENSION_BYTES => (stem, ext),
            _ => (self.name, ""),
        };
        // A name near the limit gives up the end of its stem to the number.
        let ending = format!(" ({n}){ext}");
        let mut cut = MAX_NAME_BYTES.saturating_sub(ending.len()).min(stem.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        self.dir.join(format!("{}{ending}", &stem[..cut]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_prefers_extended_filename() {
        let v = r#"attachment; filename="plain.zip"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"#;
        assert_eq!(from_content_disposition(v).as_deref(), Some("résumé.pdf"));
    }

    #[test]
    fn content_disposition_plain_with_semicolon_in_quotes() {
        let v = r#"attachment; filename="a;b.txt""#;
        assert_eq!(from_content_disposition(v).as_deref(), Some("a;b.txt"));
    }

    #[test]
    fn invisible_format_characters_cannot_disguise_a_name() {
        // "invoice\u{202E}4pm.exe" displays as "invoiceexe.mp4".
        assert_eq!(sanitize("invoice\u{202E}4pm.exe"), "invoice_4pm.exe");
        for c in [
            '\u{200E}',
            '\u{200F}',
            '\u{202A}',
            '\u{202B}',
            '\u{202C}',
            '\u{202D}',
            '\u{202E}',
            '\u{2066}',
            '\u{2067}',
            '\u{2068}',
            '\u{2069}',
            '\u{200B}',
            '\u{200D}',
            '\u{2060}',
            '\u{FEFF}',
            '\u{00AD}',
            '\u{E0041}',
        ] {
            assert_eq!(
                sanitize(&format!("a{c}b.txt")),
                "a_b.txt",
                "U+{:04X}",
                u32::from(c)
            );
        }
        // Ordinary letters from right-to-left scripts stay.
        assert_eq!(sanitize("שלום.pdf"), "שלום.pdf");
        assert_eq!(sanitize("مرحبا.txt"), "مرحبا.txt");
    }

    #[test]
    fn content_disposition_without_filename() {
        assert_eq!(from_content_disposition("inline"), None);
    }

    #[test]
    fn name_from_url_is_decoded() {
        let url = Url::parse("https://example.com/files/my%20file.iso?x=1").unwrap();
        assert_eq!(from_url(&url).as_deref(), Some("my file.iso"));
        let root = Url::parse("https://example.com/").unwrap();
        assert_eq!(from_url(&root), None);
    }

    #[test]
    fn sanitize_strips_unsafe_parts() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("a<b>:c?.txt"), "a_b__c_.txt");
        assert_eq!(sanitize("trailing. . "), "trailing");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("console.txt"), "console.txt");
        assert_eq!(sanitize(""), "download");
    }

    #[test]
    fn every_windows_device_name_is_avoided() {
        for (name, safe) in [
            ("CONIN$", "_CONIN$"),
            ("conout$.txt", "_conout$.txt"),
            ("COM\u{B9}.txt", "_COM\u{B9}.txt"),
            ("com\u{B2}", "_com\u{B2}"),
            ("LPT\u{B3}.log", "_LPT\u{B3}.log"),
            ("lpt9.tar.gz", "_lpt9.tar.gz"),
            ("NUL .txt", "_NUL .txt"),
        ] {
            assert_eq!(sanitize(name), safe, "{name}");
        }
        // Close, but ordinary names.
        for name in [
            "COM\u{2074}.txt",
            "COM10.txt",
            "LPT.txt",
            "CONIN.txt",
            "CONOUT$X.txt",
        ] {
            assert_eq!(sanitize(name), name, "{name}");
        }
    }

    #[test]
    fn sanitize_truncates_but_keeps_extension() {
        let long = format!("{}.mp4", "x".repeat(400));
        let out = sanitize(&long);
        assert!(out.len() <= SANITIZED_NAME_BYTES);
        assert_eq!(Path::new(&out).extension(), Some("mp4".as_ref()));
    }

    #[test]
    fn a_long_extension_cannot_make_the_name_too_long() {
        // Everything after the last dot of a hostile `Content-Disposition`.
        for name in [
            format!("a.{}", "x".repeat(300)),
            format!("{}.{}", "y".repeat(150), "z".repeat(150)),
            format!("é.{}", "€".repeat(100)),
        ] {
            let out = sanitize(&name);
            assert!(
                (1..=SANITIZED_NAME_BYTES).contains(&out.len()),
                "{} bytes",
                out.len()
            );
        }
        // An ordinary extension is still kept whole.
        let kept = sanitize(&format!("{}.tar.gz", "n".repeat(300)));
        assert_eq!(Path::new(&kept).extension(), Some("gz".as_ref()));
        assert!(kept.len() <= SANITIZED_NAME_BYTES);
    }

    /// A file of `contents` in `dir`, to be moved.
    fn finished(dir: &Path, leaf: &str, contents: &str) -> PathBuf {
        let path = dir.join(leaf);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn racing_moves_onto_one_name_each_land_on_their_own() {
        const RACERS: usize = 16;
        let dir = wirecatch_test_support::private_dir("move-race").unwrap();
        let start = std::sync::Barrier::new(RACERS);
        let moved: Vec<PathBuf> = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..RACERS)
                .map(|i| {
                    let (dir, start) = (&dir, &start);
                    scope.spawn(move || {
                        let from = finished(dir, &format!("from-{i}"), &i.to_string());
                        start.wait();
                        move_to_free_name(&from, dir, "f.zip").unwrap()
                    })
                })
                .collect();
            racers.into_iter().map(|r| r.join().unwrap()).collect()
        });
        let mut contents: Vec<usize> = moved
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap().parse().unwrap())
            .collect();
        contents.sort_unstable();
        assert_eq!(contents, (0..RACERS).collect::<Vec<_>>(), "{moved:?}");
        dir.remove().unwrap();
    }

    #[test]
    fn a_move_passes_over_files_and_downloads_in_progress() {
        let dir = wirecatch_test_support::private_dir("move-skip").unwrap();
        std::fs::write(dir.join("f.zip"), b"theirs").unwrap();
        std::fs::write(dir.join("f (1).zip.wcpart"), b"x").unwrap();
        std::fs::write(dir.join("f (2).zip.wcstate"), b"x").unwrap();
        let from = finished(&dir, "from", "mine");
        let moved = move_to_free_name(&from, &dir, "f.zip").unwrap();
        assert_eq!(moved, dir.join("f (3).zip"));
        assert_eq!(std::fs::read(&moved).unwrap(), b"mine");
        assert_eq!(std::fs::read(dir.join("f.zip")).unwrap(), b"theirs");
        assert!(!from.exists());
        dir.remove().unwrap();
    }

    #[test]
    fn a_failed_move_leaves_no_file_behind() {
        let dir = wirecatch_test_support::private_dir("move-failed").unwrap();
        let error = move_to_free_name(&dir.join("missing"), &dir, "f.zip").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        dir.remove().unwrap();
    }

    /// Only a name that is one ordinary file in the folder on every system is taken: `NUL` is a device, `f.zip:x` is a stream inside another file on Windows, and Windows drops a trailing dot or space.
    #[test]
    fn only_a_safe_name_is_taken() {
        let dir = wirecatch_test_support::private_dir("move-plain").unwrap();
        std::fs::write(dir.join("f.zip"), b"theirs").unwrap();
        let from = finished(&dir, "from", "mine");
        for name in [
            "",
            ".",
            "..",
            "../f.zip",
            "sub/f.zip",
            "/f.zip",
            r"sub\f.zip",
            "C:f.zip",
            "NUL",
            "CON.txt",
            "com1",
            "f.zip:x",
            "f.zip.",
            "f.zip ",
            "a\u{202E}b.txt",
        ] {
            let refused = move_to_free_name(&from, &dir, name).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::InvalidInput, "{name:?}");
        }
        assert_eq!(std::fs::read(&from).unwrap(), b"mine");
        assert_eq!(std::fs::read(dir.join("f.zip")).unwrap(), b"theirs");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        dir.remove().unwrap();
    }

    /// A name longer than `sanitize` leaves is still taken: core adds an extension and ` (1)` to names `sanitize` already cut, and a stream's container can change the extension after every byte arrived.
    #[test]
    fn a_long_name_that_sanitize_would_cut_is_taken() {
        let dir = wirecatch_test_support::private_dir("move-long").unwrap();
        let stem = sanitize(&"ж".repeat(150));
        let long = format!("{stem}.mkv");
        assert!(long.len() > SANITIZED_NAME_BYTES, "{} bytes", long.len());
        let numbered = format!("{stem} (1).mkv");
        let longest = format!("{}.mkv", "n".repeat(MAX_NAME_BYTES - 4));
        for name in [&long, &numbered, &longest] {
            let from = finished(&dir, "from", "mine");
            let moved = move_to_free_name(&from, &dir, name).unwrap();
            assert_eq!(moved, dir.join(name));
            assert_eq!(std::fs::read(&moved).unwrap(), b"mine");
            // Its part and resume files fit beside it too.
            let (part, state) = part_paths(&moved);
            for leftover in [part, state] {
                std::fs::write(&leftover, b"x").unwrap();
                std::fs::remove_file(&leftover).unwrap();
            }
        }
        let too_long = format!("{}.mkv", "n".repeat(MAX_NAME_BYTES - 3));
        let from = finished(&dir, "from", "mine");
        let refused = move_to_free_name(&from, &dir, &too_long).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        dir.remove().unwrap();
    }

    /// A taken name as long as the engine takes gives up the end of its stem, on a character boundary, for its number.
    #[test]
    fn a_taken_name_at_the_limit_is_numbered_within_it() {
        let dir = wirecatch_test_support::private_dir("move-long-taken").unwrap();
        let name = format!("{}n.mkv", "ж".repeat(121));
        assert_eq!(name.len(), MAX_NAME_BYTES);
        std::fs::write(dir.join(&name), b"theirs").unwrap();
        let from = finished(&dir, "from", "mine");
        let moved = move_to_free_name(&from, &dir, &name).unwrap();
        assert_eq!(moved, dir.join(format!("{} (1).mkv", "ж".repeat(119))));
        assert_eq!(std::fs::read(&moved).unwrap(), b"mine");
        assert_eq!(std::fs::read(dir.join(&name)).unwrap(), b"theirs");
        dir.remove().unwrap();
    }

    /// Every Windows device name, in any case, with or without an extension, and also when a cut to length is what leaves it.
    #[test]
    fn sanitize_never_returns_a_device_name() {
        let mut devices: Vec<String> = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
            .into_iter()
            .map(String::from)
            .collect();
        for prefix in ["COM", "LPT"] {
            for digit in ('0'..='9').chain(['\u{B9}', '\u{B2}', '\u{B3}']) {
                devices.push(format!("{prefix}{digit}"));
            }
        }
        for device in &devices {
            for name in [
                device.clone(),
                device.to_lowercase(),
                format!("{device}.txt"),
                format!("{}.tar.gz", device.to_lowercase()),
                format!("{device} .txt"),
                // The cut to length drops everything after the device name.
                format!("{device}{}x", " ".repeat(300)),
                format!("{device}{}x.txt", ".".repeat(300)),
            ] {
                let safe = sanitize(&name);
                assert!(!is_windows_reserved(&safe), "{name:?} gave {safe:?}");
                assert!(is_safe_name(&safe), "{name:?} gave {safe:?}");
                assert_eq!(sanitize(&safe), safe, "{name:?}");
            }
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1024))]

        /// Whatever a server sends, `sanitize` returns a name it keeps as it is, that is never a device, fits its budget and is one the engine takes.
        #[test]
        fn sanitize_returns_a_name_it_keeps(
            device in proptest::sample::select(vec!["", "CON", "nul", "Com1", "LPT\u{B9}", "conout$"]),
            middle in "[CONULPRTAXI1$¹. \u{2003}/\\\\:é中\u{202E}a_-]{0,4}",
            padding in proptest::sample::select(vec![' ', '.']),
            padding_len in 0usize..260,
            tail in "[a-zé. ]{0,4}",
        ) {
            let padding = padding.to_string().repeat(padding_len);
            let name = format!("{device}{middle}{padding}{tail}");
            let safe = sanitize(&name);
            proptest::prop_assert_eq!(&sanitize(&safe), &safe);
            proptest::prop_assert!(!is_windows_reserved(&safe), "{:?}", safe);
            proptest::prop_assert!((1..=SANITIZED_NAME_BYTES).contains(&safe.len()));
            proptest::prop_assert!(is_safe_name(&safe), "{:?}", safe);
        }
    }
}
