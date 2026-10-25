//! What each Tauri command does, as plain functions on the [`Manager`].
//!
//! The `#[tauri::command]` wrappers in `commands.rs` only find the manager and call these, so
//! tests drive exactly the code the interface runs, without a webview. Every error is a
//! [`ProtocolError`]: a code the interface acts on and plain-language text, ready to show as is.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use wirecatch_core::{LinkFrom, Manager};
use wirecatch_protocol::app::{
    AddRequest, AddonId, AddonInfo, AddonRequestItem, AppSettings, AppSettingsPatch,
    CaptureConfirmation, DownloadStatus, DownloadSummary, ImportPreview, MediaChoice,
    MediaSelection, PendingCapture, Question, Queue, QueueSettings, SuffixListInfo,
};
use wirecatch_protocol::desktop::SAFE_EXTENSIONS;
use wirecatch_protocol::proxy::{ProxyChange, ProxySettings, ProxyTest, ProxyTestResult};
use wirecatch_protocol::wire::{ErrorCode, ProtocolError};
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};

/// What a command answers: a value, or an error with a code for the interface to act on and a
/// message worded for a person. It is the protocol's error, so the interface reads one shape
/// whether the core or the shell refused.
pub type CommandResult<T> = Result<T, ProtocolError>;

/// The core's answer, its error carrying the core's code and plain-language text.
fn plain<T>(result: wirecatch_core::Result<T>) -> CommandResult<T> {
    result.map_err(|err| err.to_protocol())
}

/// A request the shell itself turns down, worded for a person.
pub fn refused(message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::InvalidRequest,
        message: message.into(),
    }
}

/// Something the shell itself could not do, worded for a person.
pub fn failed(message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::Internal,
        message: message.into(),
    }
}

/// Every download, in queue order.
#[must_use]
pub fn list_downloads(m: &Manager) -> Vec<DownloadSummary> {
    m.list_downloads()
}

/// Adds a download from the app (paste, Add dialog, a link passed on the command line).
///
/// # Errors
///
/// The link is not http, https or FTP, the queue does not exist, the folder does not resolve,
/// the file name cannot be used, or the manager has shut down ([`Manager::add_download`]).
pub async fn add_download(m: &Manager, request: AddRequest) -> CommandResult<DownloadId> {
    plain(m.add_download(request).await)
}

/// Reads a video page added in the app for its qualities (D51).
///
/// # Errors
///
/// The link is not http or https, the queue does not exist, the folder does not resolve, too
/// many videos are waiting, or the manager has shut down ([`Manager::add_video_page`]).
pub fn add_video_page(m: &Manager, request: AddRequest) -> CommandResult<QuestionId> {
    plain(m.add_video_page(request))
}

/// Pauses one download, keeping what it has downloaded.
///
/// # Errors
///
/// There is no download `id`, or the manager has shut down ([`Manager::pause`]).
pub fn pause(m: &Manager, id: DownloadId) -> CommandResult<()> {
    plain(m.pause(id))
}

/// Lets a paused or failed download continue.
///
/// # Errors
///
/// The download has finished, there is no download `id`, or the manager has shut down
/// ([`Manager::resume`]).
pub fn resume(m: &Manager, id: DownloadId) -> CommandResult<()> {
    plain(m.resume(id))
}

/// Stops a download and discards its partial file. The row stays.
///
/// # Errors
///
/// The download has finished, there is no download `id`, or the manager has shut down
/// ([`Manager::cancel`]).
pub fn cancel(m: &Manager, id: DownloadId) -> CommandResult<()> {
    plain(m.cancel(id))
}

/// Removes downloads from the list, and their files when `delete_file` is set.
///
/// # Errors
///
/// The manager has shut down ([`Manager::remove`]). Files that cannot be deleted are logged,
/// not reported.
pub async fn remove(m: &Manager, ids: &[DownloadId], delete_file: bool) -> CommandResult<()> {
    plain(m.remove(ids, delete_file).await)
}

/// Starts a failed or cancelled download again.
///
/// # Errors
///
/// The download has finished, there is no download `id`, or the manager has shut down
/// ([`Manager::retry`]).
pub fn retry(m: &Manager, id: DownloadId) -> CommandResult<()> {
    plain(m.retry(id))
}

/// Swaps in a fresh link for an expired one, keeping the downloaded bytes. `from` says whose
/// link it is: the person's only from the main window (D43.5).
///
/// # Errors
///
/// The new link is not http, https or FTP, the download has finished, there is no download
/// `id`, or the manager has shut down ([`Manager::refresh_link`]).
pub fn refresh_link(m: &Manager, id: DownloadId, url: &str, from: LinkFrom) -> CommandResult<()> {
    plain(m.refresh_link(id, url, from))
}

/// Saves a download whose conversion failed as it was downloaded instead (D24).
///
/// # Errors
///
/// The download is not a failed conversion that offers this, there is no download `id`, or
/// the manager has shut down ([`Manager::keep_original`]).
pub fn keep_original(m: &Manager, id: DownloadId) -> CommandResult<()> {
    plain(m.keep_original(id))
}

/// Moves downloads to another queue.
///
/// # Errors
///
/// The queue or one of the downloads does not exist, in which case nothing moves, or the
/// manager has shut down ([`Manager::move_to_queue`]).
pub fn move_to_queue(m: &Manager, ids: &[DownloadId], queue: QueueId) -> CommandResult<()> {
    plain(m.move_to_queue(ids, queue))
}

/// Puts a queue's downloads in this order.
///
/// # Errors
///
/// The queue or a download does not exist, a download is in another queue or listed twice,
/// or the manager has shut down; nothing changes then ([`Manager::reorder`]).
pub fn reorder(m: &Manager, queue: QueueId, ids_in_order: &[DownloadId]) -> CommandResult<()> {
    plain(m.reorder(queue, ids_in_order))
}

/// Pauses everything.
///
/// # Errors
///
/// The manager has shut down ([`Manager::pause_all`]).
pub fn pause_all(m: &Manager) -> CommandResult<()> {
    plain(m.pause_all())
}

/// Resumes everything that was paused.
///
/// # Errors
///
/// The manager has shut down ([`Manager::resume_all`]).
pub fn resume_all(m: &Manager) -> CommandResult<()> {
    plain(m.resume_all())
}

/// Every queue.
#[must_use]
pub fn list_queues(m: &Manager) -> Vec<Queue> {
    m.list_queues()
}

pub fn create_queue(m: &Manager, settings: QueueSettings) -> CommandResult<Queue> {
    plain(m.create_queue(settings))
}

pub fn update_queue(m: &Manager, id: QueueId, settings: QueueSettings) -> CommandResult<()> {
    plain(m.update_queue(id, settings))
}

/// Deletes a queue; its downloads move to the default queue.
///
/// # Errors
///
/// It is the default queue, there is no queue `id`, or the manager has shut down
/// ([`Manager::delete_queue`]).
pub fn delete_queue(m: &Manager, id: QueueId) -> CommandResult<()> {
    plain(m.delete_queue(id))
}

/// Pauses or resumes a whole queue.
///
/// # Errors
///
/// There is no queue `id`, or the manager has shut down ([`Manager::set_queue_paused`]).
pub fn set_queue_paused(m: &Manager, id: QueueId, paused: bool) -> CommandResult<()> {
    plain(m.set_queue_paused(id, paused))
}

/// Runs a scheduled queue now, outside its hours.
///
/// # Errors
///
/// There is no queue `id`, or the manager has shut down ([`Manager::start_queue_now`]).
pub fn start_queue_now(m: &Manager, id: QueueId) -> CommandResult<()> {
    plain(m.start_queue_now(id))
}

/// Every setting.
#[must_use]
pub fn get_settings(m: &Manager) -> AppSettings {
    m.get_settings()
}

/// Changes the settings a patch holds; the others keep their current values.
///
/// # Errors
///
/// A value is out of range, a queue does not exist, a folder does not resolve, or the manager
/// has shut down; nothing changes then ([`Manager::change_settings`]).
pub fn change_settings(m: &Manager, patch: AppSettingsPatch) -> CommandResult<()> {
    plain(m.change_settings(patch))
}

/// Writes the settings profile to `path`.
///
/// # Errors
///
/// `path` cannot be written ([`Manager::export_settings`]).
pub async fn export_settings(m: &Manager, path: &str) -> CommandResult<()> {
    plain(m.export_settings(Path::new(path)).await)
}

/// What importing the settings profile at `path` would change, without
/// changing anything.
///
/// # Errors
///
/// The errors of [`import_settings`].
pub async fn preview_import(m: &Manager, path: &str) -> CommandResult<ImportPreview> {
    plain(m.preview_import(Path::new(path)).await)
}

/// Reads a settings profile from `path` and applies it.
///
/// # Errors
///
/// `path` cannot be read, is not a settings profile or was made by a newer Wirecatch, its
/// settings or queues are out of range, or the manager has shut down; nothing changes then
/// ([`Manager::import_settings`]).
pub async fn import_settings(m: &Manager, path: &str) -> CommandResult<()> {
    plain(m.import_settings(Path::new(path)).await)
}

/// Browser downloads waiting for the person to confirm them.
#[must_use]
pub fn pending_confirmations(m: &Manager) -> Vec<PendingCapture> {
    m.pending_confirmations()
}

/// Starts a captured download the person confirmed, or adds it paused for later; returns the
/// new download's id.
///
/// # Errors
///
/// The question was already answered or withdrawn; the link, name, folder or queue in the
/// answer cannot be used, in which case the question stays open; or the manager has shut down
/// ([`Manager::confirm_capture`]).
pub async fn confirm_capture(
    m: &Manager,
    id: QuestionId,
    confirmation: CaptureConfirmation,
) -> CommandResult<DownloadId> {
    plain(m.confirm_capture(id, confirmation).await)
}

/// Drops a captured download the person did not want.
///
/// # Errors
///
/// The question was already answered or withdrawn, or the manager has shut down
/// ([`Manager::reject_capture`]).
pub fn reject_capture(m: &Manager, id: QuestionId) -> CommandResult<()> {
    plain(m.reject_capture(id))
}

/// Moves a download, running or not, to another folder.
///
/// # Errors
///
/// `folder` does not resolve, cannot be created or written to, or the files cannot be moved
/// there; the download is an unfinished video or already being moved; there is no download
/// `id`; or the manager has shut down ([`Manager::set_download_folder`]).
pub async fn set_download_folder(m: &Manager, id: DownloadId, folder: &str) -> CommandResult<()> {
    plain(m.set_download_folder(id, folder).await)
}

fn find(m: &Manager, id: DownloadId) -> CommandResult<DownloadSummary> {
    plain(m.get_download(id))
}

/// The type of `name` as the system sees it: lower case, the last extension only (so
/// `report.pdf.exe` is `exe`). `None` when the name has no extension.
///
/// On Windows trailing dots and spaces are dropped first, because Windows drops them and opens
/// `video.mp4.` as an mp4. On Linux they are part of the name: `video.mp4.` has no extension
/// there, and the desktop may pick a program for it by its content instead (a `.desktop`
/// launcher, say), so it must ask, and trimming would wrongly call it an mp4.
#[must_use]
pub fn file_type(name: &str) -> Option<String> {
    let name = if cfg!(windows) {
        name.trim_end_matches(['.', ' '])
    } else {
        name
    };
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// Whether opening `path` might run code, so the person must confirm first: any type not on
/// [`SAFE_EXTENSIONS`], a file without an extension, and on Linux any file marked executable,
/// whatever its name.
#[must_use]
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

/// The site a download came from, for "only open it if you trust example.com": its host as a
/// browser reads the link, with the port when it is not the scheme's own.
fn site_of(url: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    match parsed
        .as_ref()
        .and_then(|link| Some((link.host_str()?, link.port())))
    {
        Some((host, Some(port))) => format!("{host}:{port}"),
        Some((host, None)) => host.to_owned(),
        None => "where it came from".to_owned(),
    }
}

/// What `open_file` opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToOpen {
    /// A file that cannot run code: it opens at once.
    File(PathBuf),
    /// A program: it opens only once the person agreed to `warning`, which the shell shows in a
    /// dialog of its own, so no page can answer it for them.
    Program {
        /// The program.
        path: PathBuf,
        /// What the person is asked.
        warning: ProgramWarning,
    },
}

/// The warning before a program opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramWarning {
    /// Names the file.
    pub title: String,
    /// Says what the file can do and where it came from.
    pub message: String,
}

impl ProgramWarning {
    /// The warning for the program at `path`, downloaded from `url`. It names the file and type
    /// on disk, which decided that it is a program.
    fn new(path: &Path, url: &str) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let what = match file_type(&name) {
            Some(ext) if SAFE_EXTENSIONS.contains(&ext.as_str()) => {
                format!("{name} is marked as a program, so it may run code.")
            }
            Some(ext) => format!("This is a .{ext} file, a type that can run code."),
            None => format!("{name} has no file type, so it may run code."),
        };
        Self {
            title: format!("Open \"{name}\"?"),
            message: format!(
                "{what} Only open it if you trust {}. Code can change your computer, and a file from an unknown site can contain anything. To look at it first, choose Cancel and use Show in folder.",
                site_of(url)
            ),
        }
    }
}

/// The downloads whose program warning the shell shows now. A page can call `open_file` without
/// a click and as often as it likes, so while one warning is up for a download, opening that
/// download again raises no other (D43.1).
#[derive(Debug, Default)]
pub struct WarningsShown(Mutex<HashSet<DownloadId>>);

impl WarningsShown {
    /// Marks the warning for download `id` as showing until the answer drops. `None` when it
    /// shows already.
    pub fn show(&self, id: DownloadId) -> Option<WarningShowing<'_>> {
        let newly = self.ids().insert(id);
        newly.then_some(WarningShowing { shown: self, id })
    }

    fn ids(&self) -> std::sync::MutexGuard<'_, HashSet<DownloadId>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A program warning that is showing; it is gone once this drops.
#[derive(Debug)]
pub struct WarningShowing<'a> {
    shown: &'a WarningsShown,
    id: DownloadId,
}

impl Drop for WarningShowing<'_> {
    fn drop(&mut self) {
        self.shown.ids().remove(&self.id);
    }
}

/// The finished file to open for `open_file`, and whether it is a program the person must
/// trust first.
///
/// # Errors
///
/// There is no download `id`, it has not finished, or its file is no longer where it was saved.
/// Each message says what to do instead.
pub fn file_to_open(m: &Manager, id: DownloadId) -> CommandResult<ToOpen> {
    let row = find(m, id)?;
    if row.status != DownloadStatus::Completed {
        return Err(refused(
            "The file is not finished yet. It opens once the download completes.",
        ));
    }
    let path = PathBuf::from(&row.path);
    if !path.is_file() {
        return Err(refused(format!(
            "{} is no longer at {}. It may have been moved or deleted. Download it again with Retry, or look for it in another folder.",
            row.file_name, row.path
        )));
    }
    Ok(if is_program(&path) {
        ToOpen::Program {
            warning: ProgramWarning::new(&path, &row.url),
            path,
        }
    } else {
        ToOpen::File(path)
    })
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
///
/// # Errors
///
/// There is no download `id`, or neither its file nor its folder exists yet.
pub fn item_to_reveal(m: &Manager, id: DownloadId) -> CommandResult<Reveal> {
    let row = find(m, id)?;
    let path = PathBuf::from(&row.path);
    if path.is_file() {
        return Ok(Reveal::File(path));
    }
    match path.parent() {
        Some(folder) if folder.is_dir() => Ok(Reveal::Folder(folder.to_path_buf())),
        Some(folder) => Err(refused(format!(
            "The folder {} does not exist yet. Wirecatch creates it when the download starts.",
            folder.display()
        ))),
        None => Err(refused(format!("{} has no folder to show.", row.path))),
    }
}

/// Downloads the chosen quality of a media choice. An option that needs the media add-on
/// fails while it is not installed; the core then asks for it with `AddonRequired` and keeps
/// the choice open.
///
/// # Errors
///
/// The choice was already made, dismissed or withdrawn, or is not ready yet; the selection
/// asks for something the choice did not offer; the option needs the media add-on; the
/// download's name, folder or queue cannot be used, in which case the choice stays open; or
/// the manager has shut down ([`Manager::choose_media`]).
pub async fn choose_media(
    m: &Manager,
    id: QuestionId,
    selection: MediaSelection,
) -> CommandResult<DownloadId> {
    plain(m.choose_media(id, selection).await)
}

/// The open question `id`, for its download window; `None` once it is answered, dismissed or
/// withdrawn by the browser.
#[must_use]
pub fn question(m: &Manager, id: QuestionId) -> Option<Question> {
    if let Some(capture) = m.pending_confirmations().into_iter().find(|c| c.id == id) {
        return Some(Question::Capture(capture));
    }
    if let Some(choice) = m.pending_media_choices().into_iter().find(|c| c.id == id) {
        return Some(Question::Choice(choice));
    }
    if let Some(page) = m.pending_addon_requests().into_iter().find(|p| p.id == id) {
        return Some(Question::Addon(page));
    }
    m.media_being_read()
        .into_iter()
        .find(|r| r.id == id)
        .map(Question::Reading)
}

/// One download, for its progress window.
///
/// # Errors
///
/// There is no download `id` ([`Manager::get_download`]).
pub fn get_download(m: &Manager, id: DownloadId) -> CommandResult<DownloadSummary> {
    find(m, id)
}

/// The newest download in the list that question `id` is about again, so its window can say
/// so without blocking anything.
#[must_use]
pub fn earlier_download(m: &Manager, id: QuestionId) -> Option<DownloadSummary> {
    m.earlier_download(id)
}

/// Drops a media choice the person did not want (or media still being read).
///
/// # Errors
///
/// There is no such choice, or the manager has shut down ([`Manager::dismiss_media_choice`]).
pub fn dismiss_media_choice(m: &Manager, id: QuestionId) -> CommandResult<()> {
    plain(m.dismiss_media_choice(id))
}

/// Media choices still waiting for an answer, so a new or reloaded window shows them again.
#[must_use]
pub fn pending_media_choices(m: &Manager) -> Vec<MediaChoice> {
    m.pending_media_choices()
}

/// Pages waiting for the media add-on, so a new or reloaded window offers the install again.
/// [`dismiss_media_choice`] with an item's id drops that page.
#[must_use]
pub fn pending_addon_requests(m: &Manager) -> Vec<AddonRequestItem> {
    m.pending_addon_requests()
}

/// Every add-on and its state. Update checks run in the core: once a day while
/// `auto_update_addons` is on.
#[must_use]
pub fn list_addons(m: &Manager) -> Vec<AddonInfo> {
    m.list_addons()
}

/// Installs an add-on in the background; progress and the result arrive as `AddonsChanged`.
///
/// # Errors
///
/// `id` names no add-on Wirecatch knows, or the manager has shut down
/// ([`Manager::install_addon`]). A failed install is not an error here: it arrives as
/// `AddonsChanged`.
pub async fn install_addon(m: &Manager, id: AddonId) -> CommandResult<()> {
    plain(m.install_addon(id).await)
}

/// Brings an add-on up to date in the background, like [`install_addon`].
///
/// # Errors
///
/// The errors of [`install_addon`].
pub async fn update_addon(m: &Manager, id: AddonId) -> CommandResult<()> {
    plain(m.update_addon(id).await)
}

/// Looks for a newer media add-on now, as the person asked, and answers with the add-ons as
/// they stand afterwards (up to date, an update offered, or one already installing quietly).
///
/// # Errors
///
/// The releases cannot be looked up, a newer release's checksum list is not signed by its
/// developers, or the manager has shut down ([`Manager::check_addon_updates`]).
pub async fn check_addon_updates(m: &Manager) -> CommandResult<Vec<AddonInfo>> {
    plain(m.check_addon_updates().await)?;
    Ok(m.list_addons())
}

/// The Public Suffix List in use and when a newer one was last looked for (D44). The weekly
/// check runs in the core while `update_suffix_list` is on.
#[must_use]
pub fn suffix_list(m: &Manager) -> SuffixListInfo {
    m.suffix_list()
}

/// Looks for a newer Public Suffix List now, as the person asked, and answers with the list in
/// use afterwards.
///
/// # Errors
///
/// The list could not be fetched, was refused or could not be saved, which keeps the list in
/// use, or the manager has shut down ([`Manager::check_suffix_list`]).
pub async fn check_suffix_list(m: &Manager) -> CommandResult<SuffixListInfo> {
    plain(m.check_suffix_list().await)
}

/// Saves a new proxy setting (D71), restarting running downloads when `change` asks.
///
/// # Errors
///
/// The proxy cannot work, or the manager has shut down; nothing changes then
/// ([`Manager::set_proxy`]).
pub fn set_proxy(m: &Manager, change: ProxyChange) -> CommandResult<()> {
    plain(m.set_proxy(change))
}

/// Tries a proxy setting before it is saved.
///
/// # Errors
///
/// The proxy cannot work ([`Manager::test_proxy`]).
pub async fn test_proxy(m: &Manager, test: ProxyTest) -> CommandResult<ProxyTestResult> {
    plain(m.test_proxy(test).await)
}

/// The computer's proxy as a manual setting, for Copy from system.
#[must_use]
pub fn system_proxy() -> Option<ProxySettings> {
    wirecatch_core::system_proxy()
}

/// Stops an install or update in progress. An update keeps the installed version working; a
/// first install removes what it put down.
///
/// # Errors
///
/// `id` names no add-on Wirecatch knows, the install stopped but some of its files could not
/// be deleted, or the manager has shut down ([`Manager::cancel_addon_operation`]).
pub async fn cancel_addon_operation(m: &Manager, id: AddonId) -> CommandResult<()> {
    plain(m.cancel_addon_operation(id).await)
}

/// Removes an add-on and its files, stopping an install in progress first.
///
/// # Errors
///
/// `id` names no add-on Wirecatch knows, its files could not all be deleted, a video is being
/// read or downloaded with it, or the manager has shut down ([`Manager::remove_addon`]).
pub async fn remove_addon(m: &Manager, id: AddonId) -> CommandResult<()> {
    plain(m.remove_addon(id).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_core::CoreError;

    #[test]
    fn one_warning_shows_per_download_until_it_is_answered() {
        let shown = WarningsShown::default();
        let first = shown.show(DownloadId(1));
        assert!(first.is_some());
        assert!(shown.show(DownloadId(1)).is_none());
        assert!(shown.show(DownloadId(2)).is_some());
        drop(first);
        assert!(shown.show(DownloadId(1)).is_some());
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
            "letter.rtf",
            "sheet.ods",
            "text.odt",
            "slides.odp",
            "data.csv",
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
        assert_eq!(file_type("report.PDF"), Some("pdf".into()));
        assert_eq!(file_type("report.pdf.exe"), Some("exe".into()));
        assert_eq!(file_type("noext"), None);
        assert_eq!(file_type("trailing."), None);
        assert!(is_program(Path::new("report.pdf.exe")));
        assert!(is_program(Path::new("setup.exe.")));
        assert!(is_program(Path::new("setup.exe ")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drops_trailing_dots_and_spaces() {
        assert_eq!(file_type("video.mp4."), Some("mp4".into()));
        assert_eq!(file_type("video.mp4 . "), Some("mp4".into()));
        assert!(!is_program(Path::new("video.mp4.")));
        assert!(!is_program(Path::new("video.mp4  ")));
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_trailing_dots_and_spaces_are_part_of_the_name() {
        assert_eq!(file_type("video.mp4."), None);
        assert_eq!(file_type("video.mp4 "), Some("mp4 ".into()));
        assert!(is_program(Path::new("video.mp4.")));
        assert!(is_program(Path::new("video.mp4 ")));
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
        let warning = ProgramWarning::new(&dir.path().join("movie.mp4"), "https://a.example/");
        assert!(
            warning
                .message
                .starts_with("movie.mp4 is marked as a program"),
            "{warning:?}"
        );
    }

    #[test]
    fn the_warning_names_the_file_on_disk_and_where_to_look_first() {
        let warning = ProgramWarning::new(
            &Path::new("downloads").join("Setup.EXE"),
            "https://dl.example.com/get",
        );
        assert_eq!(warning.title, "Open \"Setup.EXE\"?");
        assert!(
            warning
                .message
                .starts_with("This is a .exe file, a type that can run code.")
                && warning.message.contains("trust dl.example.com.")
                && warning.message.contains("use Show in folder."),
            "{warning:?}"
        );
    }

    /// The interface tells this error from others by its code (`isAddonRequired` in
    /// `src/lib/addons.ts`), never by its wording.
    #[test]
    fn the_add_on_required_error_keeps_its_code() {
        assert!(
            plain::<()>(Err(CoreError::AddonRequired))
                .is_err_and(|e| e.code == ErrorCode::AddonRequired)
        );
    }

    #[test]
    fn the_source_site_is_named_plainly() {
        assert_eq!(
            site_of("https://user:pw@dl.example.com:8080/a.exe?x=1"),
            "dl.example.com:8080"
        );
        assert_eq!(site_of("http://example.com"), "example.com");
    }

    /// The site named is the one the link was fetched from, as a browser reads the link, never a
    /// part of it a site chose to look like a host.
    #[test]
    fn the_source_site_is_the_host_the_link_reaches() {
        // A backslash ends the host in a web address, so the `@` after it is in the path.
        assert_eq!(
            site_of(r"https://dl.example.com\@trusted.example/a.exe"),
            "dl.example.com"
        );
        assert_eq!(
            site_of("https://dl.example.com?@trusted.example/a.exe"),
            "dl.example.com"
        );
        assert_eq!(site_of("https://DL.Example.COM/a.exe"), "dl.example.com");
        assert_eq!(
            site_of("https://dl.example.com:443/a.exe"),
            "dl.example.com"
        );
        assert_eq!(site_of("http://[::1]:8080/a.exe"), "[::1]:8080");
        for unreadable in ["", "not a link", "file:///C:/setup.exe", "https://"] {
            assert_eq!(site_of(unreadable), "where it came from", "{unreadable:?}");
        }
    }
}
