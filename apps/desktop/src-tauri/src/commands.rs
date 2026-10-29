//! The Tauri commands, one for each command of the contract in `wirecatch_protocol::desktop`.
//! Each takes the contract's `Args` as its one `args` parameter, answers with its `Output`, and
//! finds the manager and calls the matching function in [`crate::api`].

use tauri::{AppHandle, Manager as _, Runtime, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt as _;
use wirecatch_core::LinkFrom;
use wirecatch_protocol::app::FileExistsChoice;
use wirecatch_protocol::desktop::commands as contract;
use wirecatch_protocol::desktop::{
    AddDownloadArgs, AddVideoPageArgs, AnswerFileExistsArgs, CancelAddonOperationArgs, CancelArgs,
    ChangeSettingsArgs, ChooseMediaArgs, ConfirmCaptureArgs, ControlWindowArgs,
    CreateProxyServerArgs, CreateQueueArgs, DeleteProxyServerArgs, DeleteQueueArgs,
    DismissMediaChoiceArgs, EarlierDownloadArgs, ExportSettingsArgs, FitDownloadWindowArgs,
    GetDownloadArgs, GetQuestionArgs, ImportSettingsArgs, InstallAddonArgs, KeepOriginalArgs,
    MoveToQueueArgs, OpenFileArgs, OpenFolderArgs, PauseArgs, PreviewImportArgs, RefreshLinkArgs,
    RejectCaptureArgs, RemoveAddonArgs, RemoveArgs, ReorderArgs, ReportUnreadableEventArgs,
    ResumeArgs, RetryArgs, SetDownloadFolderArgs, SetMaximizeButtonArgs, SetQueueOrderArgs,
    SetQueuePausedArgs, ShowDownloadDetailsArgs, ShowProgressWindowArgs, StartQueueNowArgs,
    SystemAppearance, TestProxyServerArgs, TryLinkArgs, UpdateAddonArgs, UpdateProxyServerArgs,
    UpdateQueueArgs,
};

use crate::api::{self, CommandResult};
use crate::download_windows::{self, Shows};
use crate::files;
use crate::host::Core;
use crate::window::LaunchLinks;

type CoreState<'a> = State<'a, Core>;

/// Refuses a download window acting on anything but the question or download it shows (D43).
fn its_own<R: Runtime>(window: &WebviewWindow<R>, target: Shows) -> CommandResult<()> {
    if download_windows::may_act_on(window.app_handle(), window.label(), target) {
        Ok(())
    } else {
        Err(api::refused(download_windows::NOT_ITS_OWN))
    }
}

#[tauri::command]
pub async fn list_downloads(
    core: CoreState<'_>,
) -> CommandResult<contract::list_downloads::Output> {
    Ok(api::list_downloads(core.manager().await?))
}

#[tauri::command]
pub async fn add_download(
    core: CoreState<'_>,
    args: contract::add_download::Args,
) -> CommandResult<contract::add_download::Output> {
    let AddDownloadArgs { request } = args;
    api::add_download(core.manager().await?, request).await
}

#[tauri::command]
pub async fn add_video_page(
    core: CoreState<'_>,
    args: contract::add_video_page::Args,
) -> CommandResult<contract::add_video_page::Output> {
    let AddVideoPageArgs { request } = args;
    api::add_video_page(core.manager().await?, request)
}

#[tauri::command]
pub async fn pause<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::pause::Args,
) -> CommandResult<contract::pause::Output> {
    let PauseArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    api::pause(core.manager().await?, id)
}

#[tauri::command]
pub async fn resume<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::resume::Args,
) -> CommandResult<contract::resume::Output> {
    let ResumeArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    api::resume(core.manager().await?, id)
}

#[tauri::command]
pub async fn cancel(
    core: CoreState<'_>,
    args: contract::cancel::Args,
) -> CommandResult<contract::cancel::Output> {
    let CancelArgs { id } = args;
    api::cancel(core.manager().await?, id)
}

#[tauri::command]
pub async fn remove<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::remove::Args,
) -> CommandResult<contract::remove::Output> {
    let RemoveArgs { ids, delete_file } = args;
    for id in &ids {
        its_own(&window, Shows::Download(*id))?;
    }
    api::remove(core.manager().await?, &ids, delete_file).await
}

#[tauri::command]
pub async fn retry<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::retry::Args,
) -> CommandResult<contract::retry::Output> {
    let RetryArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    api::retry(core.manager().await?, id)
}

#[tauri::command]
pub async fn refresh_link<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::refresh_link::Args,
) -> CommandResult<contract::refresh_link::Output> {
    let RefreshLinkArgs { id, url } = args;
    its_own(&window, Shows::Download(id))?;
    // A download window also shows what websites chose, so a script there could have written
    // the link: only the main window's is the person's (D43.5).
    let from = if window.label() == crate::window::MAIN_WINDOW {
        LinkFrom::Person
    } else {
        LinkFrom::SharedPage
    };
    api::refresh_link(core.manager().await?, id, url.as_str(), from)
}

#[tauri::command]
pub async fn keep_original<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::keep_original::Args,
) -> CommandResult<contract::keep_original::Output> {
    let KeepOriginalArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    api::keep_original(core.manager().await?, id)
}

#[tauri::command]
pub async fn move_to_queue(
    core: CoreState<'_>,
    args: contract::move_to_queue::Args,
) -> CommandResult<contract::move_to_queue::Output> {
    let MoveToQueueArgs { ids, queue } = args;
    api::move_to_queue(core.manager().await?, &ids, queue)
}

#[tauri::command]
pub async fn reorder(
    core: CoreState<'_>,
    args: contract::reorder::Args,
) -> CommandResult<contract::reorder::Output> {
    let ReorderArgs {
        queue,
        ids_in_order,
    } = args;
    api::reorder(core.manager().await?, queue, &ids_in_order)
}

#[tauri::command]
pub async fn pause_all(core: CoreState<'_>) -> CommandResult<contract::pause_all::Output> {
    api::pause_all(core.manager().await?)
}

#[tauri::command]
pub async fn resume_all(core: CoreState<'_>) -> CommandResult<contract::resume_all::Output> {
    api::resume_all(core.manager().await?)
}

#[tauri::command]
pub async fn list_queues(core: CoreState<'_>) -> CommandResult<contract::list_queues::Output> {
    Ok(api::list_queues(core.manager().await?))
}

#[tauri::command]
pub async fn create_queue(
    core: CoreState<'_>,
    args: contract::create_queue::Args,
) -> CommandResult<contract::create_queue::Output> {
    let CreateQueueArgs { settings } = args;
    api::create_queue(core.manager().await?, settings)
}

#[tauri::command]
pub async fn update_queue(
    core: CoreState<'_>,
    args: contract::update_queue::Args,
) -> CommandResult<contract::update_queue::Output> {
    let UpdateQueueArgs { id, settings } = args;
    api::update_queue(core.manager().await?, id, settings)
}

#[tauri::command]
pub async fn delete_queue(
    core: CoreState<'_>,
    args: contract::delete_queue::Args,
) -> CommandResult<contract::delete_queue::Output> {
    let DeleteQueueArgs { id } = args;
    api::delete_queue(core.manager().await?, id)
}

#[tauri::command]
pub async fn set_queue_paused(
    core: CoreState<'_>,
    args: contract::set_queue_paused::Args,
) -> CommandResult<contract::set_queue_paused::Output> {
    let SetQueuePausedArgs { id, paused } = args;
    api::set_queue_paused(core.manager().await?, id, paused)
}

#[tauri::command]
pub async fn start_queue_now(
    core: CoreState<'_>,
    args: contract::start_queue_now::Args,
) -> CommandResult<contract::start_queue_now::Output> {
    let StartQueueNowArgs { id } = args;
    api::start_queue_now(core.manager().await?, id)
}

#[tauri::command]
pub async fn get_settings(core: CoreState<'_>) -> CommandResult<contract::get_settings::Output> {
    Ok(api::get_settings(core.manager().await?))
}

#[tauri::command]
pub async fn change_settings<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::change_settings::Args,
) -> CommandResult<contract::change_settings::Output> {
    let ChangeSettingsArgs { patch } = args;
    let label = window.label();
    if download_windows::is_download_window(label)
        && !download_windows::may_change(&patch)
        && !crate::finished_window::may_change(label, &patch)
    {
        return Err(api::refused(download_windows::NOT_ITS_SETTING));
    }
    api::change_settings(core.manager().await?, patch)
}

#[tauri::command]
pub async fn export_settings(
    core: CoreState<'_>,
    args: contract::export_settings::Args,
) -> CommandResult<contract::export_settings::Output> {
    let ExportSettingsArgs { path } = args;
    api::export_settings(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn preview_import(
    core: CoreState<'_>,
    args: contract::preview_import::Args,
) -> CommandResult<contract::preview_import::Output> {
    let PreviewImportArgs { path } = args;
    api::preview_import(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn import_settings(
    core: CoreState<'_>,
    args: contract::import_settings::Args,
) -> CommandResult<contract::import_settings::Output> {
    let ImportSettingsArgs { path } = args;
    api::import_settings(core.manager().await?, &path).await
}

#[tauri::command]
pub async fn pending_confirmations(
    core: CoreState<'_>,
) -> CommandResult<contract::pending_confirmations::Output> {
    Ok(api::pending_confirmations(core.manager().await?))
}

#[tauri::command]
pub async fn confirm_capture<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::confirm_capture::Args,
) -> CommandResult<contract::confirm_capture::Output> {
    let ConfirmCaptureArgs { id, confirmation } = args;
    its_own(&window, Shows::Question(id))?;
    let download = api::confirm_capture(core.manager().await?, id, confirmation).await?;
    // Answered in its own window: that window shows the download from now on.
    download_windows::became_download(window.app_handle(), window.label(), download);
    Ok(download)
}

#[tauri::command]
pub async fn reject_capture<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::reject_capture::Args,
) -> CommandResult<contract::reject_capture::Output> {
    let RejectCaptureArgs { id } = args;
    its_own(&window, Shows::Question(id))?;
    api::reject_capture(core.manager().await?, id)
}

#[tauri::command]
pub async fn open_file<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::open_file::Args,
) -> CommandResult<contract::open_file::Output> {
    let OpenFileArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    files::open_download(
        window.app_handle(),
        core.manager().await?,
        id,
        Some(&window),
    )
    .await
}

#[tauri::command]
pub async fn open_folder<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::open_folder::Args,
) -> CommandResult<contract::open_folder::Output> {
    let OpenFolderArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    files::show_in_folder(window.app_handle(), core.manager().await?, id)
}

#[tauri::command]
pub async fn set_download_folder<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::set_download_folder::Args,
) -> CommandResult<contract::set_download_folder::Output> {
    let SetDownloadFolderArgs { id, folder } = args;
    its_own(&window, Shows::Download(id))?;
    api::set_download_folder(core.manager().await?, id, &folder).await
}

/// Async like every other command, so it runs off the main thread; Tauri then needs a
/// `Result`, though taking the links never fails.
#[tauri::command]
pub async fn take_launch_links(
    links: State<'_, LaunchLinks>,
) -> CommandResult<contract::take_launch_links::Output> {
    Ok(links.take())
}

#[tauri::command]
pub async fn choose_media<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::choose_media::Args,
) -> CommandResult<contract::choose_media::Output> {
    let ChooseMediaArgs { id, selection } = args;
    its_own(&window, Shows::Question(id))?;
    let download = api::choose_media(core.manager().await?, id, selection).await?;
    download_windows::became_download(window.app_handle(), window.label(), download);
    Ok(download)
}

#[tauri::command]
pub async fn get_question<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::get_question::Args,
) -> CommandResult<contract::get_question::Output> {
    let GetQuestionArgs { id } = args;
    its_own(&window, Shows::Question(id))?;
    Ok(api::question(core.manager().await?, id))
}

#[tauri::command]
pub async fn get_download<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::get_download::Args,
) -> CommandResult<contract::get_download::Output> {
    let GetDownloadArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    api::get_download(core.manager().await?, id)
}

#[tauri::command]
pub async fn earlier_download<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::earlier_download::Args,
) -> CommandResult<contract::earlier_download::Output> {
    let EarlierDownloadArgs { id } = args;
    its_own(&window, Shows::Question(id))?;
    Ok(api::earlier_download(core.manager().await?, id))
}

#[tauri::command]
pub async fn show_question_windows<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
) -> CommandResult<contract::show_question_windows::Output> {
    download_windows::show_all(&app, core.manager().await?);
    Ok(())
}

#[tauri::command]
pub async fn show_progress_window<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
    args: contract::show_progress_window::Args,
) -> CommandResult<contract::show_progress_window::Output> {
    let ShowProgressWindowArgs { id } = args;
    // Only for a download that exists, so a stale id opens nothing.
    api::get_download(core.manager().await?, id)?;
    download_windows::show_download(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn close_download_window<R: Runtime>(
    window: WebviewWindow<R>,
) -> CommandResult<contract::close_download_window::Output> {
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
pub async fn main_window_ready<R: Runtime>(
    window: WebviewWindow<R>,
) -> CommandResult<contract::main_window_ready::Output> {
    crate::appearance::match_stored_theme(&window);
    crate::window::reveal_main(&window);
    Ok(())
}

/// The longest event kind a window's report is logged with; a window sends the `type` of an event it could not read, which may be anything.
const MAX_REPORTED_KIND_CHARS: usize = 64;

#[tauri::command]
pub async fn report_unreadable_event<R: Runtime>(
    window: WebviewWindow<R>,
    args: contract::report_unreadable_event::Args,
) -> CommandResult<contract::report_unreadable_event::Output> {
    let ReportUnreadableEventArgs { kind } = args;
    let kind: Option<String> =
        kind.map(|kind| kind.chars().take(MAX_REPORTED_KIND_CHARS).collect());
    tracing::warn!(
        window = window.label(),
        kind = kind.as_deref().unwrap_or("(none)"),
        "a window could not read an event from the app and loads everything again"
    );
    Ok(())
}

#[tauri::command]
pub async fn fit_download_window<R: Runtime>(
    window: WebviewWindow<R>,
    args: contract::fit_download_window::Args,
) -> CommandResult<contract::fit_download_window::Output> {
    let FitDownloadWindowArgs { height } = args;
    if !download_windows::is_download_window(window.label()) {
        return Err(api::refused(
            "Only a download window fits itself to its content.",
        ));
    }
    if window.label() == crate::finished_window::LABEL {
        crate::finished_window::fit(&window, height);
    } else {
        download_windows::fit_to_content(&window, height);
    }
    Ok(())
}

#[tauri::command]
pub async fn dismiss_media_choice<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::dismiss_media_choice::Args,
) -> CommandResult<contract::dismiss_media_choice::Output> {
    let DismissMediaChoiceArgs { id } = args;
    its_own(&window, Shows::Question(id))?;
    api::dismiss_media_choice(core.manager().await?, id)
}

#[tauri::command]
pub async fn answer_file_exists<R: Runtime>(
    window: WebviewWindow<R>,
    core: CoreState<'_>,
    args: contract::answer_file_exists::Args,
) -> CommandResult<contract::answer_file_exists::Output> {
    let AnswerFileExistsArgs { id, answer } = args;
    its_own(&window, Shows::Question(id))?;
    let download = api::answer_file_exists(core.manager().await?, id, answer)?;
    if answer.choice != FileExistsChoice::Cancel {
        download_windows::became_download(window.app_handle(), window.label(), download);
    }
    Ok(download)
}

#[tauri::command]
pub async fn pending_file_exists(
    core: CoreState<'_>,
) -> CommandResult<contract::pending_file_exists::Output> {
    Ok(api::pending_file_exists(core.manager().await?))
}

#[tauri::command]
pub async fn pending_media_choices(
    core: CoreState<'_>,
) -> CommandResult<contract::pending_media_choices::Output> {
    Ok(api::pending_media_choices(core.manager().await?))
}

#[tauri::command]
pub async fn pending_addon_requests(
    core: CoreState<'_>,
) -> CommandResult<contract::pending_addon_requests::Output> {
    Ok(api::pending_addon_requests(core.manager().await?))
}

#[tauri::command]
pub async fn list_addons(core: CoreState<'_>) -> CommandResult<contract::list_addons::Output> {
    Ok(api::list_addons(core.manager().await?))
}

#[tauri::command]
pub async fn install_addon(
    core: CoreState<'_>,
    args: contract::install_addon::Args,
) -> CommandResult<contract::install_addon::Output> {
    let InstallAddonArgs { id } = args;
    api::install_addon(core.manager().await?, id).await
}

#[tauri::command]
pub async fn update_addon(
    core: CoreState<'_>,
    args: contract::update_addon::Args,
) -> CommandResult<contract::update_addon::Output> {
    let UpdateAddonArgs { id } = args;
    api::update_addon(core.manager().await?, id).await
}

#[tauri::command]
pub async fn remove_addon(
    core: CoreState<'_>,
    args: contract::remove_addon::Args,
) -> CommandResult<contract::remove_addon::Output> {
    let RemoveAddonArgs { id } = args;
    api::remove_addon(core.manager().await?, id).await
}

#[tauri::command]
pub async fn cancel_addon_operation(
    core: CoreState<'_>,
    args: contract::cancel_addon_operation::Args,
) -> CommandResult<contract::cancel_addon_operation::Output> {
    let CancelAddonOperationArgs { id } = args;
    api::cancel_addon_operation(core.manager().await?, id).await
}

#[tauri::command]
pub async fn check_addon_updates(
    core: CoreState<'_>,
) -> CommandResult<contract::check_addon_updates::Output> {
    api::check_addon_updates(core.manager().await?).await
}

#[tauri::command]
pub async fn suffix_list(core: CoreState<'_>) -> CommandResult<contract::suffix_list::Output> {
    Ok(api::suffix_list(core.manager().await?))
}

#[tauri::command]
pub async fn check_suffix_list(
    core: CoreState<'_>,
) -> CommandResult<contract::check_suffix_list::Output> {
    api::check_suffix_list(core.manager().await?).await
}

#[tauri::command]
pub async fn set_queue_order(
    core: CoreState<'_>,
    args: contract::set_queue_order::Args,
) -> CommandResult<contract::set_queue_order::Output> {
    let SetQueueOrderArgs { order } = args;
    api::set_queue_order(core.manager().await?, &order)
}

#[tauri::command]
pub async fn try_link(
    core: CoreState<'_>,
    args: contract::try_link::Args,
) -> CommandResult<contract::try_link::Output> {
    let TryLinkArgs { trial } = args;
    api::try_link(core.manager().await?, &trial)
}

#[tauri::command]
pub async fn list_proxy_servers(
    core: CoreState<'_>,
) -> CommandResult<contract::list_proxy_servers::Output> {
    Ok(api::list_proxy_servers(core.manager().await?))
}

#[tauri::command]
pub async fn create_proxy_server(
    core: CoreState<'_>,
    args: contract::create_proxy_server::Args,
) -> CommandResult<contract::create_proxy_server::Output> {
    let CreateProxyServerArgs { draft } = args;
    api::create_proxy_server(core.manager().await?, draft)
}

#[tauri::command]
pub async fn update_proxy_server(
    core: CoreState<'_>,
    args: contract::update_proxy_server::Args,
) -> CommandResult<contract::update_proxy_server::Output> {
    let UpdateProxyServerArgs { id, draft } = args;
    api::update_proxy_server(core.manager().await?, id, draft)
}

#[tauri::command]
pub async fn delete_proxy_server(
    core: CoreState<'_>,
    args: contract::delete_proxy_server::Args,
) -> CommandResult<contract::delete_proxy_server::Output> {
    let DeleteProxyServerArgs { id, replacement } = args;
    api::delete_proxy_server(core.manager().await?, id, replacement)
}

#[tauri::command]
pub async fn test_proxy_server(
    core: CoreState<'_>,
    args: contract::test_proxy_server::Args,
) -> CommandResult<contract::test_proxy_server::Output> {
    let TestProxyServerArgs { id, draft } = args;
    api::test_proxy_server(core.manager().await?, id, draft).await
}

#[tauri::command]
pub async fn copy_system_proxy(
    core: CoreState<'_>,
) -> CommandResult<contract::copy_system_proxy::Output> {
    api::copy_system_proxy(core.manager().await?).await
}

#[tauri::command]
pub async fn show_proxy_settings<R: Runtime>(
    app: AppHandle<R>,
) -> CommandResult<contract::show_proxy_settings::Output> {
    crate::window::reveal(&app, &wirecatch_protocol::desktop::Reveal::proxy_settings());
    Ok(())
}

#[tauri::command]
pub async fn show_download_details<R: Runtime>(
    window: WebviewWindow<R>,
    args: contract::show_download_details::Args,
) -> CommandResult<contract::show_download_details::Output> {
    let ShowDownloadDetailsArgs { id } = args;
    its_own(&window, Shows::Download(id))?;
    crate::window::reveal(
        window.app_handle(),
        &wirecatch_protocol::desktop::Reveal::details(id),
    );
    Ok(())
}

#[tauri::command]
pub async fn control_window<R: Runtime>(
    window: WebviewWindow<R>,
    args: contract::control_window::Args,
) -> CommandResult<contract::control_window::Output> {
    let ControlWindowArgs { action } = args;
    crate::frame::control(&window, action).map_err(|e| {
        api::failed(format!(
            "The window did not respond ({e}). Use its taskbar button instead."
        ))
    })
}

#[tauri::command]
pub async fn window_frame<R: Runtime>(
    window: WebviewWindow<R>,
) -> CommandResult<contract::window_frame::Output> {
    Ok(crate::frame::frame_of(&window))
}

#[tauri::command]
pub async fn set_maximize_button<R: Runtime>(
    window: WebviewWindow<R>,
    args: contract::set_maximize_button::Args,
) -> CommandResult<contract::set_maximize_button::Output> {
    let SetMaximizeButtonArgs { area } = args;
    crate::frame::set_maximize_button(&window, area);
    Ok(())
}

#[tauri::command]
pub async fn system_appearance<R: Runtime>(
    app: AppHandle<R>,
) -> CommandResult<contract::system_appearance::Output> {
    Ok(SystemAppearance {
        accent: crate::appearance::last_system_accent(&app),
    })
}

#[tauri::command]
pub async fn startup_notices(
    core: CoreState<'_>,
) -> CommandResult<contract::startup_notices::Output> {
    Ok(core.manager().await?.startup_notices().to_vec())
}

#[tauri::command]
pub async fn show_set_aside_list<R: Runtime>(
    app: AppHandle<R>,
    core: CoreState<'_>,
) -> CommandResult<contract::show_set_aside_list::Output> {
    let manager = core.manager().await?;
    let Some(path) = manager.set_aside_list() else {
        return Err(api::failed("No download list was set aside at this start."));
    };
    app.opener().reveal_item_in_dir(path).map_err(|e| {
        api::failed(format!(
            "Could not show the old download list in its folder ({e}). It is kept at {}.",
            path.display()
        ))
    })
}

#[tauri::command]
pub async fn open_log_folder<R: Runtime>(
    app: AppHandle<R>,
) -> CommandResult<contract::open_log_folder::Output> {
    let Some(folder) = crate::host::log_folder(std::env::var_os(crate::host::DATA_DIR_VAR)) else {
        return Err(api::failed(
            "This system names no folder for Wirecatch's logs.",
        ));
    };
    // Logging makes the folder only once something is logged.
    std::fs::create_dir_all(&folder)
        .and_then(|()| {
            app.opener()
                .open_path(folder.to_string_lossy(), None::<&str>)
                .map_err(std::io::Error::other)
        })
        .map_err(|e| {
            api::failed(format!(
                "Could not open the log folder ({e}). It is {}.",
                folder.display()
            ))
        })
}

#[tauri::command]
pub async fn take_reveal(
    selection: State<'_, crate::window::PendingSelection>,
) -> CommandResult<contract::take_reveal::Output> {
    Ok(selection.take())
}

#[tauri::command]
pub async fn finished_list<R: Runtime>(
    window: WebviewWindow<R>,
    finished: State<'_, crate::finished_window::FinishedWindow>,
) -> CommandResult<contract::finished_list::Output> {
    if window.label() != crate::finished_window::LABEL {
        return Err(api::refused(
            "Only the download complete window lists what finished.",
        ));
    }
    Ok(finished.list())
}

/// Registers the commands named, for the app's invoke handler, and lists their names in
/// [`REGISTERED`] for the test that holds them to the contract.
macro_rules! handler {
    ($($command:ident),+ $(,)?) => {
        /// The names of the commands [`handler`] registers.
        #[cfg(test)]
        pub const REGISTERED: &[&str] = &[$(stringify!($command)),+];

        /// Every command above, for the app's invoke handler.
        pub fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($command),+]
        }
    };
}

handler![
    list_downloads,
    add_download,
    add_video_page,
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
    open_folder,
    set_download_folder,
    take_launch_links,
    choose_media,
    dismiss_media_choice,
    pending_media_choices,
    answer_file_exists,
    pending_file_exists,
    pending_addon_requests,
    list_addons,
    install_addon,
    update_addon,
    remove_addon,
    cancel_addon_operation,
    check_addon_updates,
    suffix_list,
    check_suffix_list,
    set_queue_order,
    try_link,
    list_proxy_servers,
    create_proxy_server,
    update_proxy_server,
    delete_proxy_server,
    test_proxy_server,
    copy_system_proxy,
    show_proxy_settings,
    show_download_details,
    get_question,
    get_download,
    earlier_download,
    show_question_windows,
    show_progress_window,
    close_download_window,
    fit_download_window,
    main_window_ready,
    control_window,
    window_frame,
    set_maximize_button,
    system_appearance,
    startup_notices,
    show_set_aside_list,
    open_log_folder,
    take_reveal,
    finished_list,
    report_unreadable_event,
];

#[cfg(test)]
mod tests {
    use super::REGISTERED;
    use wirecatch_protocol::desktop::COMMAND_NAMES;

    /// The interface calls exactly the commands in the contract, so the shell registers exactly
    /// those: one missing fails at runtime only, and one extra is a command nothing declares.
    #[test]
    fn the_shell_registers_exactly_the_contract_commands() {
        let mut registered = REGISTERED.to_vec();
        registered.sort_unstable();
        let mut contract: Vec<&str> = COMMAND_NAMES.iter().map(|name| name.0).collect();
        contract.sort_unstable();
        assert_eq!(registered, contract);
    }
}
