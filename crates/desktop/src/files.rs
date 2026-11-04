//! Opening a finished download, showing it in its folder, and picking a folder. A program asks
//! first (D43.1).

use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, PathPromptOptions, Window};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api::{self, ProgramWarning, Reveal, ToOpen};

use crate::backend::Backend;

const CHOOSER_FAILED: &str = "The file chooser could not open. On Linux, install xdg-desktop-portal with the portal for your desktop, then try again.";

/// Asks for a folder to save in; `None` when none is picked, or when its name is not valid text,
/// which the window in front is told.
pub fn choose_folder(cx: &mut App) -> impl Future<Output = Option<String>> + use<> {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Save here".into()),
    });
    let cx = cx.to_async();
    async move {
        let folder = chooser_answer(picked.await).and_then(|paths| {
            let Some(folder) = paths.and_then(|paths| paths.into_iter().next()) else {
                return Ok(None);
            };
            api::path_text(&folder).map(Some)
        });
        folder
            .map_err(|refused| {
                cx.update(|cx| {
                    if let Some(window) = crate::backend::window_in_front(cx) {
                        _ = window.update(cx, |_, window, cx| {
                            window.push_notification(refused.message, cx);
                        });
                    }
                });
            })
            .ok()
            .flatten()
    }
}

/// What a system file chooser answered: `None` when closed without a choice, and a refusal to show
/// when it could not open, as on a Linux desktop with no file chooser portal.
pub fn chooser_answer<T, E: Display, Gone>(
    answer: Result<Result<Option<T>, E>, Gone>,
) -> Result<Option<T>, ProtocolError> {
    match answer {
        Ok(Ok(chosen)) => Ok(chosen),
        Err(_) => Ok(None),
        Ok(Err(e)) => {
            tracing::error!("the system file chooser could not open: {e}");
            Err(api::failed(CHOOSER_FAILED))
        }
    }
}

/// Opens download `id`'s finished file; a program opens only once the person agrees in `window`.
pub fn open(id: DownloadId, window: &mut Window, cx: &mut App) {
    let found = Backend::run(cx, move |manager| async move {
        api::file_to_open(&manager, id).await
    });
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

/// Asks in `window` before a program at `path` opens.
pub fn ask_to_open(path: PathBuf, warning: ProgramWarning, window: &mut Window, cx: &mut App) {
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
        api::item_to_reveal(&manager, id).await
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

#[cfg(test)]
mod tests {
    use super::*;

    type Answer = Result<Result<Option<u8>, &'static str>, ()>;

    #[test]
    fn a_chooser_that_could_not_open_is_not_taken_for_a_cancel() {
        let failed: Answer = Ok(Err("no portal"));
        assert_eq!(chooser_answer(failed).unwrap_err().message, CHOOSER_FAILED);
    }

    #[test]
    fn a_chooser_closed_without_a_choice_says_nothing() {
        let closed: Answer = Ok(Ok(None));
        let gone: Answer = Err(());
        let chosen: Answer = Ok(Ok(Some(1)));
        assert_eq!(chooser_answer(closed).ok(), Some(None));
        assert_eq!(chooser_answer(gone).ok(), Some(None));
        assert_eq!(chooser_answer(chosen).ok(), Some(Some(1)));
    }
}
