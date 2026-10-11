//! What each Tauri command does, as plain functions on the [`Manager`].
//!
//! The `#[tauri::command]` wrappers in `commands.rs` only find the manager and call these, so
//! tests drive exactly the code the interface runs, without a webview. Every error is the
//! core's plain-language text, ready to show as is.

use std::path::{Path, PathBuf};

use wirecatch_core::{CoreError, Manager};
use wirecatch_protocol::app::{
    AddRequest, AddonInfo, AppSettings, CaptureConfirmation, DownloadStatus, DownloadSummary,
    MediaSelection, PendingCapture, Queue,
};
use wirecatch_protocol::{DownloadId, QueueId};

/// What a command answers: a value, or an error worded for a person.
pub type CommandResult<T> = Result<T, String>;

/// The answer of the media and add-on commands until the core implements them (WP11).
pub const NEXT_UPDATE: &str = "This arrives in the next update of Wirecatch. Until then, downloads from video sites stay in the browser.";

fn plain(err: CoreError) -> String {
    err.to_string()
}

/// Every download, in queue order.
pub fn list_downloads(m: &Manager) -> Vec<DownloadSummary> {
    m.list_downloads()
}

/// Adds a download from the app (paste, Add dialog, a link passed on the command line).
pub async fn add_download(m: &Manager, request: AddRequest) -> CommandResult<DownloadId> {
    m.add_download(request).await.map_err(plain)
}

/// Pauses one download, keeping what it has downloaded.
pub fn pause(m: &Manager, id: DownloadId) -> CommandResult<()> {
    m.pause(id).map_err(plain)
}

/// Lets a paused or failed download continue.
pub fn resume(m: &Manager, id: DownloadId) -> CommandResult<()> {
    m.resume(id).map_err(plain)
}

/// Stops a download and discards its partial file. The row stays.
pub fn cancel(m: &Manager, id: DownloadId) -> CommandResult<()> {
    m.cancel(id).map_err(plain)
}

/// Removes downloads from the list, and their files when `delete_file` is set.
pub async fn remove(m: &Manager, ids: &[DownloadId], delete_file: bool) -> CommandResult<()> {
    m.remove(ids, delete_file).await.map_err(plain)
}

/// Starts a failed or cancelled download again.
pub fn retry(m: &Manager, id: DownloadId) -> CommandResult<()> {
    m.retry(id).map_err(plain)
}

/// Swaps in a fresh link for an expired one, keeping the downloaded bytes.
pub fn refresh_link(m: &Manager, id: DownloadId, url: &str) -> CommandResult<()> {
    m.refresh_link(id, url).map_err(plain)
}

/// Moves downloads to another queue.
pub fn move_to_queue(m: &Manager, ids: &[DownloadId], queue: QueueId) -> CommandResult<()> {
    m.move_to_queue(ids, queue).map_err(plain)
}

/// Puts a queue's downloads in this order.
pub fn reorder(m: &Manager, queue: QueueId, ids_in_order: &[DownloadId]) -> CommandResult<()> {
    m.reorder(queue, ids_in_order).map_err(plain)
}

/// Pauses everything.
pub fn pause_all(m: &Manager) -> CommandResult<()> {
    m.pause_all().map_err(plain)
}

/// Resumes everything that was paused.
pub fn resume_all(m: &Manager) -> CommandResult<()> {
    m.resume_all().map_err(plain)
}

/// Every queue.
pub fn list_queues(m: &Manager) -> Vec<Queue> {
    m.list_queues()
}

/// Makes a new queue.
pub fn create_queue(m: &Manager, name: &str) -> CommandResult<Queue> {
    m.create_queue(name).map_err(plain)
}

/// Saves a queue's name, limits and schedule.
pub fn update_queue(m: &Manager, queue: Queue) -> CommandResult<()> {
    m.update_queue(queue).map_err(plain)
}

/// Deletes a queue; its downloads move to the default queue.
pub fn delete_queue(m: &Manager, id: QueueId) -> CommandResult<()> {
    m.delete_queue(id).map_err(plain)
}

/// Pauses or resumes a whole queue.
pub fn set_queue_paused(m: &Manager, id: QueueId, paused: bool) -> CommandResult<()> {
    m.set_queue_paused(id, paused).map_err(plain)
}

/// Runs a scheduled queue now, outside its hours.
pub fn start_queue_now(m: &Manager, id: QueueId) -> CommandResult<()> {
    m.start_queue_now(id).map_err(plain)
}

/// Every setting.
pub fn get_settings(m: &Manager) -> AppSettings {
    m.get_settings()
}

/// Saves every setting at once.
pub fn update_settings(m: &Manager, settings: AppSettings) -> CommandResult<()> {
    m.update_settings(settings).map_err(plain)
}

/// Writes the settings profile to `path`.
pub async fn export_settings(m: &Manager, path: &str) -> CommandResult<()> {
    m.export_settings(Path::new(path)).await.map_err(plain)
}

/// Reads a settings profile from `path` and applies it.
pub async fn import_settings(m: &Manager, path: &str) -> CommandResult<()> {
    m.import_settings(Path::new(path)).await.map_err(plain)
}

/// Browser downloads waiting for the person to confirm them.
pub fn pending_confirmations(m: &Manager) -> Vec<PendingCapture> {
    m.pending_confirmations()
}

/// Starts a captured download the person confirmed.
pub async fn confirm_capture(
    m: &Manager,
    id: u64,
    confirmation: CaptureConfirmation,
) -> CommandResult<()> {
    m.confirm_capture(id, confirmation).await.map_err(plain)
}

/// Drops a captured download the person did not want.
pub fn reject_capture(m: &Manager, id: u64) -> CommandResult<()> {
    m.reject_capture(id).map_err(plain)
}

/// Moves a download, running or not, to another folder.
pub async fn set_download_folder(m: &Manager, id: DownloadId, folder: &str) -> CommandResult<()> {
    m.set_download_folder(id, folder).await.map_err(plain)
}

fn find(m: &Manager, id: DownloadId) -> CommandResult<DownloadSummary> {
    m.list_downloads()
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| plain(CoreError::DownloadNotFound(id)))
}

/// File types that open directly, without asking first: common video, audio, image, document
/// and archive types that do not run code when opened. Every other type, and a file with no
/// extension, could run code (programs, installers, scripts, shortcuts, disk images, Office
/// files with macros and many more), so opening it needs the person's confirmation. An
/// allowlist fails safe: a type nobody thought of asks first.
///
/// In order: video, audio, subtitles, images, documents (Office files with macros, such as
/// `docm`, are left out on purpose), archives.
pub const SAFE_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "3gp",
    "ogv", "mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aiff", "mka", "srt",
    "vtt", "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
    "pdf", "txt", "md", "rtf", "odt", "ods", "odp", "docx", "xlsx", "pptx", "csv", "epub", "zip",
    "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "zst",
];

/// The type of `name` as the system sees it: trailing dots and spaces dropped (Windows ignores
/// them), lower case, the last extension only (so `report.pdf.exe` is `exe`). `None` when the
/// name has no extension.
pub fn file_type(name: &str) -> Option<String> {
    let name = name.trim_end_matches(['.', ' ']);
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// Whether opening `path` might run code, so the person must confirm first: any type not on
/// [`SAFE_EXTENSIONS`], a file without an extension, and on Linux any file marked executable,
/// whatever its name.
pub fn is_program(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let safe = file_type(&name).is_some_and(|ext| SAFE_EXTENSIONS.contains(&ext.as_str()));
    !safe || is_marked_executable(path)
}

#[cfg(unix)]
fn is_marked_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_marked_executable(_path: &Path) -> bool {
    false
}

/// Whether the download `id` is a program (see [`is_program`]), for the interface to ask first.
pub fn download_is_program(m: &Manager, id: DownloadId) -> CommandResult<bool> {
    Ok(is_program(Path::new(&find(m, id)?.path)))
}

/// The site a download came from, for "only open it if you trust example.com".
fn site_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    if host.is_empty() {
        "where it came from".to_owned()
    } else {
        host.to_owned()
    }
}

/// The finished file to open for `open_file`. A program is only opened when `allow_program` is
/// set, after the person confirmed they trust where it came from.
pub fn file_to_open(m: &Manager, id: DownloadId, allow_program: bool) -> CommandResult<PathBuf> {
    let row = find(m, id)?;
    if row.status != DownloadStatus::Completed {
        return Err("The file is not finished yet. It opens once the download completes.".into());
    }
    let path = PathBuf::from(&row.path);
    if !path.is_file() {
        return Err(format!(
            "{} is no longer at {}. It may have been moved or deleted. Download it again with Retry, or look for it in another folder.",
            row.file_name, row.path
        ));
    }
    if is_program(&path) && !allow_program {
        return Err(format!(
            "{} is a type of file that can run code. Only open it if you trust {}. Use Show in folder to open it yourself.",
            row.file_name,
            site_of(&row.url)
        ));
    }
    Ok(path)
}

/// What `open_folder` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reveal {
    /// The file exists: show its folder with the file selected.
    File(PathBuf),
    /// Only the folder exists yet: open it.
    Folder(PathBuf),
}

/// The file to select, or the folder to open, for `open_folder`.
pub fn item_to_reveal(m: &Manager, id: DownloadId) -> CommandResult<Reveal> {
    let row = find(m, id)?;
    let path = PathBuf::from(&row.path);
    if path.is_file() {
        return Ok(Reveal::File(path));
    }
    match path.parent() {
        Some(folder) if folder.is_dir() => Ok(Reveal::Folder(folder.to_path_buf())),
        Some(folder) => Err(format!(
            "The folder {} does not exist yet. Wirecatch creates it when the download starts.",
            folder.display()
        )),
        None => Err(format!("{} has no folder to show.", row.path)),
    }
}

fn not_yet<T>() -> CommandResult<T> {
    Err(NEXT_UPDATE.to_owned())
}

/// Downloads the chosen quality of a media choice. Pending until the core has media support.
pub fn choose_media(_m: &Manager, _id: u64, _selection: MediaSelection) -> CommandResult<()> {
    not_yet()
}

/// Drops a media choice the person did not want. Until the core has media support there is
/// nothing to tell it; the desktop forgets the choice itself.
pub fn dismiss_media_choice(_m: &Manager, _id: u64) -> CommandResult<()> {
    Ok(())
}

/// Media choices still waiting for an answer, so a new or reloaded window shows them again.
///
/// Adapter: the core does not offer `pending_media_choices()` yet (WP11), so these are the ones
/// the desktop saw arrive and has not seen answered. Once the core has it, this returns the
/// core's list instead.
pub fn pending_media_choices(
    _m: &Manager,
    questions: &crate::questions::Questions,
) -> Vec<wirecatch_protocol::app::MediaChoice> {
    questions.media_choices()
}

/// Every add-on and its state. Pending until the core has the add-on manager.
pub fn list_addons(_m: &Manager) -> CommandResult<Vec<AddonInfo>> {
    not_yet()
}

/// Installs an add-on. Pending, like [`list_addons`].
pub fn install_addon(_m: &Manager, _id: &str) -> CommandResult<()> {
    not_yet()
}

/// Updates an add-on. Pending, like [`list_addons`].
pub fn update_addon(_m: &Manager, _id: &str) -> CommandResult<()> {
    not_yet()
}

/// Removes an add-on. Pending, like [`list_addons`].
pub fn remove_addon(_m: &Manager, _id: &str) -> CommandResult<()> {
    not_yet()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pending_answer_is_plain() {
        assert!(!NEXT_UPDATE.contains('\u{2014}'), "no em dashes");
        assert!(NEXT_UPDATE.starts_with("This arrives in the next update"));
    }

    #[test]
    fn known_safe_types_open_directly_in_any_case() {
        for name in [
            "movie.mp4",
            "MOVIE.MKV",
            "song.mp3",
            "photo.JPG",
            "report.pdf",
            "notes.txt",
            "sheet.xlsx",
            "book.epub",
            "archive.zip",
            "backup.tar.gz",
            "data.7z",
        ] {
            assert!(!is_program(Path::new(name)), "{name} should open directly");
        }
    }

    #[test]
    fn every_other_type_asks_first() {
        for name in [
            "setup.exe",
            "SETUP.EXE",
            "Installer.Msi",
            "run.bat",
            "x.ps1",
            "x.js",
            "x.hta",
            "x.lnk",
            "x.url",
            "console.msc",
            "app.appref-ms",
            "x.settingcontent-ms",
            "x.website",
            "x.xbap",
            "x.wsc",
            "x.sct",
            "disk.iso",
            "disk.img",
            "disk.vhd",
            "disk.vhdx",
            "x.diagcab",
            "x.library-ms",
            "x.search-ms",
            "x.xll",
            "x.terminal",
            "x.workflow",
            "macros.docm",
            "macros.xlsm",
            "macros.pptm",
            "page.html",
            "drawing.svg",
            "Wirecatch.AppImage",
            "install.sh",
            "x.deb",
            "something.unknowntype",
            "README",
            "tool",
        ] {
            assert!(is_program(Path::new(name)), "{name} should need trust");
        }
    }

    #[test]
    fn the_type_is_read_the_way_the_system_reads_it() {
        assert_eq!(file_type("video.mp4."), Some("mp4".into()));
        assert_eq!(file_type("video.mp4 . "), Some("mp4".into()));
        assert_eq!(file_type("report.PDF"), Some("pdf".into()));
        assert_eq!(file_type("report.pdf.exe"), Some("exe".into()));
        assert_eq!(file_type("noext"), None);
        assert_eq!(file_type("trailing."), None);
        assert!(!is_program(Path::new("video.mp4.")));
        assert!(!is_program(Path::new("video.mp4  ")));
        assert!(is_program(Path::new("report.pdf.exe")));
        assert!(is_program(Path::new("setup.exe.")));
        assert!(is_program(Path::new("setup.exe ")));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_marked_executable_is_a_program_whatever_its_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        for name in ["tool", "movie.mp4"] {
            let file = dir.path().join(name);
            std::fs::write(&file, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(is_program(&file), name == "tool", "{name} not executable");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_program(&file), "{name} marked executable");
        }
    }

    #[test]
    fn the_source_site_is_named_plainly() {
        assert_eq!(
            site_of("https://user:pw@dl.example.com:8080/a.exe?x=1"),
            "dl.example.com:8080"
        );
        assert_eq!(site_of("http://example.com"), "example.com");
    }
}
