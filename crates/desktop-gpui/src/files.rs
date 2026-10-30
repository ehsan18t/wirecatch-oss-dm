//! Opening a finished download, showing it in its folder, and picking a folder. A program asks
//! first (D43.1).

use std::future::Future;
use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, PathPromptOptions, Window};
use wirecatch_protocol::DownloadId;
use wirecatch_shell::api::{self, ProgramWarning, Reveal, ToOpen};

use crate::backend::Backend;

/// Asks for a folder to save in; `None` when none is picked.
pub fn choose_folder(cx: &mut App) -> impl Future<Output = Option<String>> + use<> {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Save here".into()),
    });
    async move {
        let folder = picked.await.ok()?.ok()??.into_iter().next()?;
        Some(folder.to_string_lossy().into_owned())
    }
}

/// Opens download `id`'s finished file; a program opens only once the person agrees in `window`.
pub fn open(id: DownloadId, window: &mut Window, cx: &mut App) {
    let found = Backend::run(
        cx,
        move |manager| async move { api::file_to_open(&manager, id) },
    );
    window
        .spawn(cx, async move |cx| {
            let opened = match found.await {
                Ok(Ok(opened)) => opened,
                Ok(Err(unopened)) => Err(unopened.reason),
                Err(_) => return,
            };
            _ = cx.update(|window, cx| match opened {
                Ok(ToOpen::File(path)) => cx.open_with_system(&path),
                Ok(ToOpen::Program { path, warning }) => ask_to_open(path, warning, window, cx),
                Err(refused) => window.push_notification(refused.message, cx),
            });
        })
        .detach();
}

fn ask_to_open(path: PathBuf, warning: ProgramWarning, window: &mut Window, cx: &mut App) {
    crate::ui::dialog::open_alert(window, cx, move |alert, _, _| {
        let path = path.clone();
        alert
            .title(warning.title.clone())
            .description(warning.message.clone())
            .confirm()
            .ok_text("Open")
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Cancel")
            .on_ok(move |_, _, cx| {
                cx.open_with_system(&path);
                true
            })
    });
}

/// Shows download `id`'s file selected in its folder, or opens the folder while the file is not
/// there yet.
pub fn show_in_folder(id: DownloadId, window: &mut Window, cx: &mut App) {
    let found = Backend::run(cx, move |manager| async move {
        api::item_to_reveal(&manager, id)
    });
    window
        .spawn(cx, async move |cx| {
            let shown = match found.await {
                Ok(Ok(shown)) => shown,
                Ok(Err(unopened)) => Err(unopened.reason),
                Err(_) => return,
            };
            _ = cx.update(|window, cx| match shown {
                Ok(Reveal::File(path)) => cx.reveal_path(&path),
                Ok(Reveal::Folder(folder)) => cx.open_with_system(&folder),
                Err(refused) => window.push_notification(refused.message, cx),
            });
        })
        .detach();
}
