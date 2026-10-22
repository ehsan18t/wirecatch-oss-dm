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
///
/// A file no longer at `path` is left alone: a part file can be renamed into
/// place between deciding to mark it and marking it, and writing the stream by
/// name would then create an empty file under the old name.
#[cfg(windows)]
pub(crate) fn mark(path: &Path, host_url: &str, referrer: Option<&str>) {
    let marked = open_to_mark(path).and_then(|file| write_mark(&file, host_url, referrer));
    match marked {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "the file to mark was moved or removed first");
        }
        Err(e) => {
            tracing::info!(path = %path.display(), error = %e, "could not mark the file as downloaded from the internet");
        }
    }
}

/// Opens the file at `path` to mark it, never creating it. Asks only to read
/// its attributes, so it opens beside the engine's own handles, and shares
/// everything, so the engine can still rename it.
#[cfg(windows)]
fn open_to_mark(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;

    std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .open(path)
}

/// Writes the mark into `file`'s `Zone.Identifier` stream. The stream is
/// opened through the file's handle rather than its name, so the mark lands on
/// this file whatever it has been renamed to since it was opened.
#[cfg(windows)]
fn write_mark(file: &std::fs::File, host_url: &str, referrer: Option<&str>) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr::null;

    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_NON_DIRECTORY_FILE, FILE_OVERWRITE_IF, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        GENERIC_WRITE, HANDLE, RtlNtStatusToDosError, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    use windows_sys::Win32::System::Kernel::OBJ_CASE_INSENSITIVE;

    let mut units: Vec<u16> = ":Zone.Identifier".encode_utf16().collect();
    let name_len = u16::try_from(units.len() * 2).map_err(std::io::Error::other)?;
    let name = UNICODE_STRING {
        Length: name_len,
        MaximumLength: name_len,
        Buffer: units.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).map_err(std::io::Error::other)?,
        RootDirectory: file.as_raw_handle(),
        ObjectName: &raw const name,
        // As `CreateFile` asks, so a stream is found whatever its case.
        Attributes: OBJ_CASE_INSENSITIVE.cast_unsigned(),
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: an all-zero IO_STATUS_BLOCK is valid; the call fills it in.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is to a live local; `name`, the `units` it points to
    // and `file` outlive the call.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            GENERIC_WRITE | SYNCHRONIZE,
            &raw const attributes,
            &raw mut status_block,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            // Not shared for writing: a second mark of the same file at once
            // (the ticker's late one and the finished file's) fails and is
            // logged, rather than mixing its text into this one.
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            FILE_OVERWRITE_IF,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: a plain conversion of a status code.
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(std::io::Error::from_raw_os_error(
            i32::try_from(code).map_err(std::io::Error::other)?,
        ));
    }
    // SAFETY: the call succeeded, so `handle` is an open handle owned by nothing else.
    let mut stream = std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(handle) });
    stream.write_all(zone_identifier(host_url, referrer).as_bytes())
}

/// Does nothing: only Windows marks files downloaded from the internet.
#[cfg(not(windows))]
pub(crate) fn mark(_path: &Path, _host_url: &str, _referrer: Option<&str>) {}

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

    /// A part file renamed into place before the mark reaches it must not be
    /// brought back as an empty file under its old name.
    #[cfg(windows)]
    #[test]
    fn marking_a_file_that_is_gone_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("setup.exe.wcpart");
        mark(&part, "https://example.com/setup.exe", None);
        assert!(!part.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// The mark goes to the file that was opened, under whatever name it has
    /// by the time the mark is written.
    #[cfg(windows)]
    #[test]
    fn the_mark_follows_a_file_renamed_while_it_is_marked() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("setup.exe.wcpart");
        let done = dir.path().join("setup.exe");
        std::fs::write(&part, b"MZ").unwrap();
        let file = open_to_mark(&part).unwrap();
        std::fs::rename(&part, &done).unwrap();
        write_mark(&file, "https://example.com/setup.exe", None).unwrap();
        assert!(!part.exists());
        let stream = format!("{}:Zone.Identifier", done.display());
        let text = std::fs::read_to_string(stream).expect("no Zone.Identifier stream");
        assert!(
            text.contains("HostUrl=https://example.com/setup.exe"),
            "{text}"
        );
    }
}
