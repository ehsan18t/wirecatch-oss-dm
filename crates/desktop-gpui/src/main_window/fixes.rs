//! A failed download's fix (D3), which the row, its menu and the details all offer.

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Window};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::FixAction;
use wirecatch_protocol::desktop::Reveal;
use wirecatch_shell::api;

use super::bring_up;
use crate::backend::{act, command};
use crate::files;
use crate::library::Asked;

/// What a failed download's row offers for it: its own fix, else trying again.
pub fn offered(row: &wirecatch_protocol::app::DownloadSummary) -> Option<FixAction> {
    let fix = row.error.as_ref().and_then(|error| error.fix);
    fix.or(super::applies::can_retry(row).then_some(FixAction::Retry))
}

/// Does `fix` for download `id`.
pub fn run(id: DownloadId, fix: FixAction, window: &mut Window, cx: &mut App) {
    match fix {
        FixAction::Retry => act(window.window_handle(), cx, move |manager| {
            api::retry(manager, id)
        }),
        FixAction::ChooseFolder => choose_folder(id, true, window, cx),
        FixAction::FreeDiskSpace => files::show_in_folder(id, window, cx),
        FixAction::KeepOriginal => act(window.window_handle(), cx, move |manager| {
            api::keep_original(manager, id)
        }),
        FixAction::ProxySettings => {
            window.close_all_dialogs(cx);
            bring_up(&Asked::Reveal(Reveal::proxy_settings()), cx);
        }
        FixAction::RefreshLink | FixAction::ChangeQueue => {
            window.push_notification("This comes with a later version of this window.", cx);
        }
    }
}

/// Asks for a new folder for download `id` and saves it there, then tries it again when `retry`.
pub fn choose_folder(id: DownloadId, retry: bool, window: &mut Window, cx: &mut App) {
    let picked = files::choose_folder(cx);
    window
        .spawn(cx, async move |cx| {
            let Some(folder) = picked.await else {
                return;
            };
            _ = cx.update(|window, cx| {
                command(window.window_handle(), cx, move |manager| async move {
                    api::set_download_folder(&manager, id, &folder).await?;
                    if retry {
                        api::retry(&manager, id)
                    } else {
                        Ok(())
                    }
                });
            });
        })
        .detach();
}
