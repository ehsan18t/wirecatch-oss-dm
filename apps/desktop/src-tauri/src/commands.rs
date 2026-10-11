//! The Tauri commands. Each finds the manager and calls the matching function in [`crate::api`];
//! see the contract table in the crate documentation.

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt as _;
use wirecatch_protocol::app::{
    AddRequest, AddonInfo, AppSettings, CaptureConfirmation, DownloadSummary, MediaChoice,
    MediaSelection, PendingCapture, Queue,
};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::api::{self, CommandResult, Reveal};
use crate::host::Core;
use wirecatch_core::AddonRequestItem;

use crate::window::LaunchLinks;

type CoreState<'a> = State<'a, Core>;

#[tauri::command]
pub async fn list_downloads(core: CoreState<'_>) -> CommandResult<Vec<DownloadSummary>> {
    Ok(api::list_downloads(core.manager().await?))
}

#[tauri::command]
pub async fn add_download(core: CoreState<'_>, request: AddRequest) -> CommandResult<DownloadId> {
    api::add_download(core.manager().await?, request).await
}

#[tauri::command]
pub async fn pause(core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    api::pause(core.manager().await?, id)
}

#[tauri::command]
pub async fn resume(core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    api::resume(core.manager().await?, id)
}

#[tauri::command]
pub async fn cancel(core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    api::cancel(core.manager().await?, id)
}

#[tauri::command]
pub async fn remove(
    core: CoreState<'_>,
    ids: Vec<DownloadId>,
    delete_file: bool,
) -> CommandResult<()> {
    api::remove(core.manager().await?, &ids, delete_file).await
}

#[tauri::command]
pub async fn retry(core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    api::retry(core.manager().await?, id)
}

#[tauri::command]
pub async fn refresh_link(core: CoreState<'_>, id: DownloadId, url: String) -> CommandResult<()> {
    api::refresh_link(core.manager().await?, id, &url)
}

#[tauri::command]
pub async fn move_to_queue(
    core: CoreState<'_>,
    ids: Vec<DownloadId>,
    queue: QueueId,
) -> CommandResult<()> {
    api::move_to_queue(core.manager().await?, &ids, queue)
}

#[tauri::command]
pub async fn reorder(
    core: CoreState<'_>,
    queue: QueueId,
    ids_in_order: Vec<DownloadId>,
) -> CommandResult<()> {
    api::reorder(core.manager().await?, queue, &ids_in_order)
}

#[tauri::command]
pub async fn pause_all(core: CoreState<'_>) -> CommandResult<()> {
    api::pause_all(core.manager().await?)
}

#[tauri::command]
pub async fn resume_all(core: CoreState<'_>) -> CommandResult<()> {
    api::resume_all(core.manager().await?)
}

#[tauri::command]
pub async fn list_queues(core: CoreState<'_>) -> CommandResult<Vec<Queue>> {
    Ok(api::list_queues(core.manager().await?))
}

#[tauri::command]
pub async fn create_queue(core: CoreState<'_>, name: String) -> CommandResult<Queue> {
    api::create_queue(core.manager().await?, &name)
}

#[tauri::command]
pub async fn update_queue(core: CoreState<'_>, queue: Queue) -> CommandResult<()> {
    api::update_queue(core.manager().await?, queue)
}

#[tauri::command]
pub async fn delete_queue(core: CoreState<'_>, id: QueueId) -> CommandResult<()> {
    api::delete_queue(core.manager().await?, id)
}

#[tauri::command]
pub async fn set_queue_paused(core: CoreState<'_>, id: QueueId, paused: bool) -> CommandResult<()> {
    api::set_queue_paused(core.manager().await?, id, paused)
}

#[tauri::command]
pub async fn start_queue_now(core: CoreState<'_>, id: QueueId) -> CommandResult<()> {
    api::start_queue_now(core.manager().await?, id)
}

#[tauri::command]
pub async fn get_settings(core: CoreState<'_>) -> CommandResult<AppSettings> {
    Ok(api::get_settings(core.manager().await?))
}

#[tauri::command]
pub async fn update_settings(core: CoreState<'_>, settings: AppSettings) -> CommandResult<()> {
    api::update_settings(core.manager().await?, settings)
}

#[tauri::command]
pub async fn export_settings(core: CoreState<'_>, path: String) -> CommandResult<()> {
    api::export_settings(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn import_settings(core: CoreState<'_>, path: String) -> CommandResult<()> {
    api::import_settings(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn pending_confirmations(core: CoreState<'_>) -> CommandResult<Vec<PendingCapture>> {
    Ok(api::pending_confirmations(core.manager().await?))
}

#[tauri::command]
pub async fn confirm_capture(
    core: CoreState<'_>,
    id: u64,
    confirmation: CaptureConfirmation,
) -> CommandResult<()> {
    api::confirm_capture(core.manager().await?, id, confirmation).await
}

#[tauri::command]
pub async fn reject_capture(core: CoreState<'_>, id: u64) -> CommandResult<()> {
    api::reject_capture(core.manager().await?, id)
}

#[tauri::command]
pub async fn open_file(
    app: AppHandle,
    core: CoreState<'_>,
    id: DownloadId,
    allow_program: Option<bool>,
) -> CommandResult<()> {
    let path = api::file_to_open(core.manager().await?, id, allow_program.unwrap_or(false))?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| {
            format!(
                "Could not open {} ({e}). Try opening it from its folder.",
                path.display()
            )
        })
}

#[tauri::command]
pub async fn is_program(core: CoreState<'_>, id: DownloadId) -> CommandResult<bool> {
    api::download_is_program(core.manager().await?, id)
}

#[tauri::command]
pub async fn open_folder(app: AppHandle, core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    let opener = app.opener();
    match api::item_to_reveal(core.manager().await?, id)? {
        Reveal::File(path) => opener.reveal_item_in_dir(&path).map_err(|e| {
            format!(
                "Could not show {} in its folder ({e}). Open the folder yourself: {}",
                path.display(),
                path.parent().unwrap_or(&path).display()
            )
        }),
        Reveal::Folder(folder) => opener
            .open_path(folder.to_string_lossy(), None::<&str>)
            .map_err(|e| {
                format!(
                    "Could not open the folder {} ({e}). Open it yourself.",
                    folder.display()
                )
            }),
    }
}

#[tauri::command]
pub async fn set_download_folder(
    core: CoreState<'_>,
    id: DownloadId,
    folder: String,
) -> CommandResult<()> {
    api::set_download_folder(core.manager().await?, id, &folder).await
}

#[tauri::command]
pub fn take_launch_links(links: State<'_, LaunchLinks>) -> Vec<String> {
    links.take()
}

#[tauri::command]
pub async fn choose_media(
    core: CoreState<'_>,
    id: u64,
    selection: MediaSelection,
) -> CommandResult<()> {
    api::choose_media(core.manager().await?, id, selection).await
}

#[tauri::command]
pub async fn dismiss_media_choice(core: CoreState<'_>, id: u64) -> CommandResult<()> {
    api::dismiss_media_choice(core.manager().await?, id)
}

#[tauri::command]
pub async fn pending_media_choices(core: CoreState<'_>) -> CommandResult<Vec<MediaChoice>> {
    Ok(api::pending_media_choices(core.manager().await?))
}

#[tauri::command]
pub async fn pending_addon_requests(core: CoreState<'_>) -> CommandResult<Vec<AddonRequestItem>> {
    Ok(api::pending_addon_requests(core.manager().await?))
}

#[tauri::command]
pub async fn list_addons(core: CoreState<'_>) -> CommandResult<Vec<AddonInfo>> {
    Ok(api::list_addons(core.manager().await?))
}

#[tauri::command]
pub async fn install_addon(core: CoreState<'_>, id: String) -> CommandResult<()> {
    api::install_addon(core.manager().await?, &id).await
}

#[tauri::command]
pub async fn update_addon(core: CoreState<'_>, id: String) -> CommandResult<()> {
    api::update_addon(core.manager().await?, &id).await
}

#[tauri::command]
pub async fn remove_addon(core: CoreState<'_>, id: String) -> CommandResult<()> {
    api::remove_addon(core.manager().await?, &id).await
}
