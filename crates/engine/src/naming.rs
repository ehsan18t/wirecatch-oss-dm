//! Picking a safe, non-clashing file name for a download.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

use crate::storage::{PART_EXT, STATE_EXT};

const MAX_NAME_BYTES: usize = 200;
const FALLBACK_NAME: &str = "download";

/// Extracts the file name from a `Content-Disposition` header value.
/// `filename*` (RFC 6266 / 5987) wins over plain `filename`.
pub fn from_content_disposition(value: &str) -> Option<String> {
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
pub fn from_url(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    (!decoded.trim().is_empty()).then(|| decoded.into_owned())
}

/// Makes a name safe on both Windows and Linux.
pub fn sanitize(name: &str) -> String {
    // Drop any directory part a server may have sent.
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out: String = base
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
    let trimmed_len = out.trim_end_matches(['.', ' ']).len();
    out.truncate(trimmed_len);
    let out = out.trim_start().to_string();

    let mut out = if out.is_empty() || out == "." || out == ".." {
        FALLBACK_NAME.to_string()
    } else {
        out
    };

    if is_windows_reserved(&out) {
        out.insert(0, '_');
    }
    truncate_keeping_extension(&out, MAX_NAME_BYTES)
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

fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

fn truncate_keeping_extension(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    let room = max.saturating_sub(ext.len());
    let mut cut = room.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &stem[..cut], ext)
}

/// Splits `archive.tar.gz` into (`archive.tar`, `.gz`). Dotfiles keep their name as the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Paths of the in-progress data file and its resume state for a final path.
pub fn part_paths(final_path: &Path) -> (PathBuf, PathBuf) {
    let mut part = final_path.as_os_str().to_owned();
    part.push(PART_EXT);
    let mut state = final_path.as_os_str().to_owned();
    state.push(STATE_EXT);
    (PathBuf::from(part), PathBuf::from(state))
}

/// First of `name`, `name (1)`, `name (2)`... whose final, part and state files are all absent.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = split_ext(name);
    (0..)
        .map(|n| {
            if n == 0 {
                dir.join(name)
            } else {
                dir.join(format!("{stem} ({n}){ext}"))
            }
        })
        .find(|candidate| {
            let (part, state) = part_paths(candidate);
            !candidate.exists() && !part.exists() && !state.exists()
        })
        .expect("an unbounded range always yields a free name")
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
    fn sanitize_truncates_but_keeps_extension() {
        let long = format!("{}.mp4", "x".repeat(400));
        let out = sanitize(&long);
        assert!(out.len() <= MAX_NAME_BYTES);
        assert!(out.ends_with(".mp4"));
    }

    #[test]
    fn unique_path_skips_existing_files() {
        let dir = std::env::temp_dir().join(format!("wc-naming-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.zip"), b"x").unwrap();
        std::fs::write(dir.join("f (1).zip.wcpart"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "f.zip"), dir.join("f (2).zip"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
