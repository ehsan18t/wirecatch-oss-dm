//! The Tauri commands. Each finds the manager and calls the matching function in [`crate::api`];
//! see the contract table in the crate documentation.

use tauri::{AppHandle, Manager as _, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt as _;
use wirecatch_protocol::app::{
    AddRequest, AddonInfo, AppSettings, AppSettingsPatch, CaptureConfirmation, DownloadSummary,
    ImportPreview, MediaChoice, MediaSelection, PendingCapture, Queue,
};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::api::{self, CommandResult, Reveal};
use crate::download_windows;
use crate::host::Core;
use crate::questions::Question;
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
pub async fn keep_original(core: CoreState<'_>, id: DownloadId) -> CommandResult<()> {
    api::keep_original(core.manager().await?, id)
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
pub async fn change_settings(core: CoreState<'_>, patch: AppSettingsPatch) -> CommandResult<()> {
    api::change_settings(core.manager().await?, patch)
}

#[tauri::command]
pub async fn export_settings(core: CoreState<'_>, path: String) -> CommandResult<()> {
    api::export_settings(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn preview_import(core: CoreState<'_>, path: String) -> CommandResult<ImportPreview> {
    api::preview_import(core.manager().await?, &path).await
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
    window: WebviewWindow,
    core: CoreState<'_>,
    id: u64,
    confirmation: CaptureConfirmation,
) -> CommandResult<DownloadId> {
    let download = api::confirm_capture(core.manager().await?, id, confirmation).await?;
    // Answered in its own window: that window shows the download from now on.
    download_windows::became_download(window.app_handle(), window.label(), download);
    Ok(download)
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
            api::failed(format!(
                "Could not open {} ({e}). Try opening it from its folder.",
                path.display()
            ))
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

#[tauri::command]
pub async fn set_download_folder(
    core: CoreState<'_>,
    id: DownloadId,
    folder: String,
) -> CommandResult<()> {
    api::set_download_folder(core.manager().await?, id, &folder).await
}

/// Async like every other command, so it runs off the main thread; Tauri then needs a
/// `Result`, though taking the links never fails.
#[tauri::command]
pub async fn take_launch_links(links: State<'_, LaunchLinks>) -> CommandResult<Vec<String>> {
    Ok(links.take())
}

#[tauri::command]
pub async fn choose_media(
    window: WebviewWindow,
    core: CoreState<'_>,
    id: u64,
    selection: MediaSelection,
) -> CommandResult<DownloadId> {
    let download = api::choose_media(core.manager().await?, id, selection).await?;
    download_windows::became_download(window.app_handle(), window.label(), download);
    Ok(download)
}

#[tauri::command]
pub async fn get_question(core: CoreState<'_>, id: u64) -> CommandResult<Option<Question>> {
    Ok(api::question(core.manager().await?, id))
}

#[tauri::command]
pub async fn get_download(core: CoreState<'_>, id: DownloadId) -> CommandResult<DownloadSummary> {
    api::get_download(core.manager().await?, id)
}

#[tauri::command]
pub async fn earlier_download(
    core: CoreState<'_>,
    id: u64,
) -> CommandResult<Option<DownloadSummary>> {
    Ok(api::earlier_download(core.manager().await?, id))
}

#[tauri::command]
pub async fn show_question_windows(app: AppHandle, core: CoreState<'_>) -> CommandResult<()> {
    download_windows::show_all(&app, core.manager().await?);
    Ok(())
}

#[tauri::command]
pub async fn show_progress_window(
    app: AppHandle,
    core: CoreState<'_>,
    id: DownloadId,
) -> CommandResult<()> {
    // Only for a download that exists, so a stale id opens nothing.
    api::get_download(core.manager().await?, id)?;
    download_windows::show_download(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn close_download_window(window: WebviewWindow) -> CommandResult<()> {
    if !download_windows::is_download_window(window.label()) {
        return Err(api::refused("Only a download window closes this way."));
    }
    window.close().map_err(|e| {
        api::failed(format!(
            "Could not close the window ({e}). Close it with its X button."
        ))
    })
}

#[tauri::command]
pub async fn main_window_ready(window: WebviewWindow) -> CommandResult<()> {
    crate::window::reveal_main(&window);
    Ok(())
}

#[tauri::command]
pub async fn fit_download_window(window: WebviewWindow, height: f64) -> CommandResult<()> {
    if !download_windows::is_download_window(window.label()) {
        return Err(api::refused(
            "Only a download window fits itself to its content.",
        ));
    }
    download_windows::fit_to_content(&window, height);
    Ok(())
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

#[tauri::command]
pub async fn cancel_addon_operation(core: CoreState<'_>, id: String) -> CommandResult<()> {
    api::cancel_addon_operation(core.manager().await?, &id).await
}

#[tauri::command]
pub async fn check_addon_updates(core: CoreState<'_>) -> CommandResult<Vec<AddonInfo>> {
    api::check_addon_updates(core.manager().await?).await
}

/// Every command above, for the app's invoke handler. The contract table in the crate
/// documentation lists them with their parameters.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        list_downloads,
        add_download,
        pause,
        resume,
        cancel,
        remove,
        retry,
        refresh_link,
        keep_original,
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
        change_settings,
        export_settings,
        preview_import,
        import_settings,
        pending_confirmations,
        confirm_capture,
        reject_capture,
        open_file,
        is_program,
        open_folder,
        set_download_folder,
        take_launch_links,
        choose_media,
        dismiss_media_choice,
        pending_media_choices,
        pending_addon_requests,
        list_addons,
        install_addon,
        update_addon,
        remove_addon,
        cancel_addon_operation,
        check_addon_updates,
        get_question,
        get_download,
        earlier_download,
        show_question_windows,
        show_progress_window,
        close_download_window,
        fit_download_window,
        main_window_ready,
    ]
}
