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

/// The finished file to open for `open_file`.
pub fn file_to_open(m: &Manager, id: DownloadId) -> CommandResult<PathBuf> {
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

/// Drops a media choice the person did not want. Pending, like [`choose_media`].
pub fn dismiss_media_choice(_m: &Manager, _id: u64) -> CommandResult<()> {
    not_yet()
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
}
