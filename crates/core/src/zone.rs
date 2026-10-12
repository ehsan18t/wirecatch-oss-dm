//! The Mark of the Web: on Windows, a finished download gets a `Zone.Identifier`
//! alternate data stream saying it came from the internet, as browsers do. The
//! extension takes downloads away from the browser, so without it SmartScreen,
//! Office Protected View and the "this file came from another computer" check
//! would never see files downloaded with Wirecatch.
//!
//! Marking is best effort: a drive without alternate data streams (FAT, exFAT,
//! many network shares) cannot carry it, which is logged and otherwise ignored.

use std::path::Path;

#[cfg(any(windows, test))]
use wirecatch_engine::{Url, url::without_credentials};

/// Longest URL written into the mark; longer ones are cut.
// Only Windows writes the mark; the text is built there and in tests on every platform.
#[cfg(any(windows, test))]
const MAX_URL_LEN: usize = 2048;

/// The text of the `Zone.Identifier` stream for a file downloaded from
/// `host_url`, found through `referrer` if known. Zone 3 is the internet zone.
/// URLs lose any user name, password and fragment, and the file's address its
/// query too (which often carries a token); anything that is not a URL is left
/// out.
// Only Windows writes the mark; the text is built there and in tests on every platform.
#[cfg(any(windows, test))]
pub(crate) fn zone_identifier(host_url: &str, referrer: Option<&str>) -> String {
    let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(referrer) = referrer.and_then(clean_url) {
        text.push_str("ReferrerUrl=");
        text.push_str(&referrer);
        text.push_str("\r\n");
    }
    if let Some(host) = clean_url(host_url).map(|u| match u.split_once('?') {
        Some((without, _)) => without.to_string(),
        None => u,
    }) {
        text.push_str("HostUrl=");
        text.push_str(&host);
        text.push_str("\r\n");
    }
    text
}

/// A URL fit for the mark: no credentials or fragment, at most [`MAX_URL_LEN`]
/// characters, and certainly no line breaks.
// Only Windows writes the mark; the text is built there and in tests on every platform.
#[cfg(any(windows, test))]
fn clean_url(raw: &str) -> Option<String> {
    let mut url = without_credentials(&Url::parse(raw.trim()).ok()?);
    url.set_fragment(None);
    let mut text: String = url.as_str().chars().filter(|c| !c.is_control()).collect();
    if text.len() > MAX_URL_LEN {
        let mut end = MAX_URL_LEN;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

/// Marks the file at `path` as downloaded from the internet. Blocks on the disk.
/// Does nothing outside Windows.
pub(crate) fn mark(path: &Path, host_url: &str, referrer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        if let Err(e) = std::fs::write(&stream, zone_identifier(host_url, referrer)) {
            tracing::info!(path = %path.display(), error = %e, "could not mark the file as downloaded from the internet");
        }
    }
    #[cfg(not(windows))]
    let _ = (path, host_url, referrer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_and_fragments_never_reach_the_mark() {
        let text = zone_identifier(
            "https://user:secret@files.example/a.zip?x=1#part",
            Some("https://site.example/page#top"),
        );
        assert_eq!(
            text,
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://site.example/page\r\nHostUrl=https://files.example/a.zip\r\n"
        );
    }

    #[test]
    fn junk_and_overlong_urls_are_handled() {
        assert_eq!(
            zone_identifier("not a url", None),
            "[ZoneTransfer]\r\nZoneId=3\r\n"
        );
        let long = format!("https://example.com/{}", "a".repeat(5000));
        let text = zone_identifier(&long, None);
        let host = text.lines().find(|l| l.starts_with("HostUrl=")).unwrap();
        assert_eq!(host.len(), "HostUrl=".len() + MAX_URL_LEN);
    }

    #[cfg(windows)]
    #[test]
    fn the_mark_is_written_as_an_alternate_data_stream() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("setup.exe");
        std::fs::write(&file, b"MZ").unwrap();
        mark(&file, "https://example.com/setup.exe", None);
        let stream = format!("{}:Zone.Identifier", file.display());
        let text = std::fs::read_to_string(stream).expect("no Zone.Identifier stream");
        assert_eq!(
            text,
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/setup.exe\r\n"
        );
        // The file itself is untouched.
        assert_eq!(std::fs::read(&file).unwrap(), b"MZ");
    }
}
