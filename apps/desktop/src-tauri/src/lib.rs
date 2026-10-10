//! The Wirecatch desktop shell.
//!
//! For now every command is a stub that answers "not connected yet". Wave 2 replaces the bodies
//! with calls into `wirecatch-core` and forwards each core `UiEvent` to the webview on
//! [`EVENT_NAME`]. The command names and argument names here are the contract the interface
//! uses (`apps/desktop/src/lib/backend/tauri.ts`); a test on the TypeScript side checks that
//! both lists agree.

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

/// Proposed addition to the command list: move a download to another folder. The capture
/// toast's Change button and the Choose folder fix need it.
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

/// Starts the desktop app. With `--background` no window opens; Wave 2 adds the tray icon and
/// keeps the core running there.
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
