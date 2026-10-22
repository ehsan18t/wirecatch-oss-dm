//! Opening a finished download and showing it in its folder, for the windows' buttons and the
//! notifications' alike, so a program asks first whichever opens it (D43.1, D52).

use tauri::{AppHandle, Manager as _, Runtime, WebviewWindow};
use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt as _;
use wirecatch_core::Manager;
use wirecatch_protocol::DownloadId;

use crate::api::{self, CommandResult, ProgramWarning, Reveal, ToOpen, WarningsShown};

/// Opens download `id`'s finished file. A program opens only once the person agreed to the
/// shell's warning, shown over `over` when a window asked, or on its own for a notification.
/// While a warning is up for the download, opening it again does nothing.
///
/// # Errors
///
/// The download is not finished, its file is gone, or the system could not open it; each
/// message says what to do instead.
pub async fn open_download<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Manager,
    id: DownloadId,
    over: Option<&WebviewWindow<R>>,
) -> CommandResult<()> {
    let warnings = app.try_state::<WarningsShown>();
    let _showing = match &warnings {
        Some(warnings) => match warnings.show(id) {
            Some(showing) => Some(showing),
            None => return Ok(()),
        },
        None => None,
    };
    let path = match api::file_to_open(manager, id)? {
        ToOpen::File(path) => path,
        ToOpen::Program { path, warning } => {
            if !agrees_to_open(app, warning, over).await {
                return Ok(());
            }
            path
        }
    };
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| {
            api::failed(format!(
                "Could not open {} ({e}). Try opening it from its folder.",
                path.display()
            ))
        })
}

/// Shows `warning` in the system's own dialog, over `over` when given, and says whether the
/// person chose Open. The dialog belongs to the shell, so a script in a page cannot answer it.
async fn agrees_to_open<R: Runtime>(
    app: &AppHandle<R>,
    warning: ProgramWarning,
    over: Option<&WebviewWindow<R>>,
) -> bool {
    let (answer, answered) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .message(warning.message)
        .title(warning.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open".to_owned(),
            "Cancel".to_owned(),
        ));
    if let Some(window) = over {
        dialog = dialog.parent(window);
    }
    dialog.show(move |open| {
        if answer.send(open).is_err() {
            tracing::debug!("the warning was answered after its command stopped waiting");
        }
    });
    answered.await.unwrap_or(false)
}

/// Shows download `id`'s file selected in its folder, or opens the folder while the file is
/// not there yet.
///
/// # Errors
///
/// Neither the file nor its folder exists, or the system could not show them.
pub fn show_in_folder<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Manager,
    id: DownloadId,
) -> CommandResult<()> {
    let opener = app.opener();
    match api::item_to_reveal(manager, id)? {
        Reveal::File(path) => opener.reveal_item_in_dir(&path).map_err(|e| {
            api::failed(format!(
                "Could not show {} in its folder ({e}). Open the folder yourself: {}",
                path.display(),
                path.parent().unwrap_or(&path).display()
            ))
        }),
        Reveal::Folder(folder) => opener
            .open_path(folder.to_string_lossy(), None::<&str>)
            .map_err(|e| {
                api::failed(format!(
                    "Could not open the folder {} ({e}). Open it yourself.",
                    folder.display()
                ))
            }),
    }
}
