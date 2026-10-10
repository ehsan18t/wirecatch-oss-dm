//! The Wirecatch desktop shell.
//!
//! For now every command is a stub that answers "not connected yet". Wave 2 replaces the bodies
//! with calls into `wirecatch-core` and forwards each core `UiEvent` to the webview on
//! [`EVENT_NAME`]. The command names and argument names here are the contract the interface
//! uses (`apps/desktop/src/lib/backend/tauri.ts`); a test on the TypeScript side checks that
//! both lists agree.
//!
//! # Command contract
//!
//! Wave 2 implements exactly these commands with these parameter names (the webview sends the
//! camelCase form, for example `deleteFile` for `delete_file`; Tauri maps it). Every command
//! fails with a plain-language `String`.
//!
//! | Command | Parameters | Returns |
//! |---|---|---|
//! | `list_downloads` | none | `Vec<DownloadSummary>`, in queue order |
//! | `add_download` | `request: AddRequest` | `DownloadId` |
//! | `pause` | `id: DownloadId` | nothing |
//! | `resume` | `id: DownloadId` | nothing |
//! | `cancel` | `id: DownloadId` | nothing |
//! | `remove` | `ids: Vec<DownloadId>`, `delete_file: bool` | nothing |
//! | `retry` | `id: DownloadId` | nothing |
//! | `refresh_link` | `id: DownloadId`, `url: String` | nothing |
//! | `move_to_queue` | `ids: Vec<DownloadId>`, `queue: QueueId` | nothing |
//! | `reorder` | `queue: QueueId`, `ids_in_order: Vec<DownloadId>` | nothing |
//! | `pause_all` | none | nothing |
//! | `resume_all` | none | nothing |
//! | `list_queues` | none | `Vec<Queue>` |
//! | `create_queue` | `name: String` | `Queue` |
//! | `update_queue` | `queue: Queue` | nothing |
//! | `delete_queue` | `id: QueueId` | nothing |
//! | `set_queue_paused` | `id: QueueId`, `paused: bool` | nothing |
//! | `start_queue_now` | `id: QueueId` | nothing |
//! | `get_settings` | none | `AppSettings` |
//! | `update_settings` | `settings: AppSettings` | nothing |
//! | `export_settings` | `path: String` | nothing |
//! | `import_settings` | `path: String` | nothing |
//! | `pending_confirmations` | none | `Vec<PendingCapture>` |
//! | `confirm_capture` | `id: u64`, `confirmation: CaptureConfirmation` | nothing |
//! | `reject_capture` | `id: u64` | nothing |
//! | `open_file` | `id: DownloadId` | nothing |
//! | `open_folder` | `id: DownloadId` | nothing |
//! | `set_download_folder` | `id: DownloadId`, `folder: String` | nothing |
//!
//! # Starting in the background
//!
//! `--background` (passed by the bridge) only skips creating the window for now. There is no
//! tray icon and no single-instance guard yet, so a second launch starts a second process; Wave 2
//! adds both, together with hosting `wirecatch-core`.

use wirecatch_protocol::app::{
    AddRequest, AppSettings, CaptureConfirmation, DownloadSummary, PendingCapture, Queue,
};
use wirecatch_protocol::{DownloadId, QueueId};

/// The one Tauri event that carries every `UiEvent` to the webview.
pub const EVENT_NAME: &str = "wirecatch://event";

/// What every command answers until the core is wired in.
const NOT_CONNECTED: &str =
    "Wirecatch is not connected to its download engine yet. This build only shows the interface.";

type CommandResult<T> = Result<T, String>;

fn not_connected<T>() -> CommandResult<T> {
    Err(NOT_CONNECTED.to_owned())
}

#[tauri::command]
fn list_downloads() -> CommandResult<Vec<DownloadSummary>> {
    not_connected()
}

#[tauri::command]
fn add_download(request: AddRequest) -> CommandResult<DownloadId> {
    let _ = request;
    not_connected()
}

#[tauri::command]
fn pause(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn resume(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn cancel(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn remove(ids: Vec<DownloadId>, delete_file: bool) -> CommandResult<()> {
    let _ = (ids, delete_file);
    not_connected()
}

#[tauri::command]
fn retry(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn refresh_link(id: DownloadId, url: String) -> CommandResult<()> {
    let _ = (id, url);
    not_connected()
}

#[tauri::command]
fn move_to_queue(ids: Vec<DownloadId>, queue: QueueId) -> CommandResult<()> {
    let _ = (ids, queue);
    not_connected()
}

#[tauri::command]
fn reorder(queue: QueueId, ids_in_order: Vec<DownloadId>) -> CommandResult<()> {
    let _ = (queue, ids_in_order);
    not_connected()
}

#[tauri::command]
fn pause_all() -> CommandResult<()> {
    not_connected()
}

#[tauri::command]
fn resume_all() -> CommandResult<()> {
    not_connected()
}

#[tauri::command]
fn list_queues() -> CommandResult<Vec<Queue>> {
    not_connected()
}

#[tauri::command]
fn create_queue(name: String) -> CommandResult<Queue> {
    let _ = name;
    not_connected()
}

#[tauri::command]
fn update_queue(queue: Queue) -> CommandResult<()> {
    let _ = queue;
    not_connected()
}

#[tauri::command]
fn delete_queue(id: QueueId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn get_settings() -> CommandResult<AppSettings> {
    not_connected()
}

#[tauri::command]
fn update_settings(settings: AppSettings) -> CommandResult<()> {
    let _ = settings;
    not_connected()
}

#[tauri::command]
fn export_settings(path: String) -> CommandResult<()> {
    let _ = path;
    not_connected()
}

#[tauri::command]
fn import_settings(path: String) -> CommandResult<()> {
    let _ = path;
    not_connected()
}

#[tauri::command]
fn pending_confirmations() -> CommandResult<Vec<PendingCapture>> {
    not_connected()
}

#[tauri::command]
fn confirm_capture(id: u64, confirmation: CaptureConfirmation) -> CommandResult<()> {
    let _ = (id, confirmation);
    not_connected()
}

#[tauri::command]
fn reject_capture(id: u64) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn open_file(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn open_folder(id: DownloadId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn set_queue_paused(id: QueueId, paused: bool) -> CommandResult<()> {
    let _ = (id, paused);
    not_connected()
}

#[tauri::command]
fn start_queue_now(id: QueueId) -> CommandResult<()> {
    let _ = id;
    not_connected()
}

#[tauri::command]
fn set_download_folder(id: DownloadId, folder: String) -> CommandResult<()> {
    let _ = (id, folder);
    not_connected()
}

/// The flag the bridge passes when it starts the app for the browser: run with no window.
pub const BACKGROUND_FLAG: &str = "--background";

/// True when the command line asks to start without a window.
pub fn wants_background<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .any(|a| a.as_ref() == BACKGROUND_FLAG)
}

/// Starts the desktop app. With `--background` no window opens. There is no tray icon or
/// single-instance guard yet (Wave 2 adds them), so the process simply runs without a window.
pub fn run() {
    let background = wants_background(std::env::args());
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            if !background {
                // The window is declared in tauri.conf.json with `create: false`, so it is only
                // built here, when the app is started by a person rather than by the bridge.
                if let Some(config) = app.config().app.windows.first() {
                    tauri::WebviewWindowBuilder::from_config(app.handle(), config)?.build()?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_downloads,
            add_download,
            pause,
            resume,
            cancel,
            remove,
            retry,
            refresh_link,
            move_to_queue,
            reorder,
            pause_all,
            resume_all,
            list_queues,
            create_queue,
            update_queue,
            delete_queue,
            set_queue_paused,
            start_queue_now,
            get_settings,
            update_settings,
            export_settings,
            import_settings,
            pending_confirmations,
            confirm_capture,
            reject_capture,
            open_file,
            open_folder,
            set_download_folder,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("Wirecatch could not start its window: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_explain_they_are_not_connected() {
        let err = list_downloads().expect_err("stub must refuse");
        assert!(err.contains("not connected"));
        assert!(
            !err.contains('\u{2014}'),
            "no em dashes in user-facing text"
        );
    }

    #[test]
    fn background_flag_is_read_after_the_program_name() {
        assert!(wants_background(["wirecatch-app", "--background"]));
        assert!(!wants_background(["wirecatch-app"]));
        assert!(!wants_background(["--background"]));
        assert!(!wants_background(["wirecatch-app", "--backgrounds"]));
    }
}
