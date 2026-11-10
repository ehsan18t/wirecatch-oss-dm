//! Opening a finished download, showing it in its folder, and picking a folder. A program asks
//! first (D43.1).

use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::dialog::Question;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Global, PathPromptOptions, Window};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api::{self, ProgramWarning, Reveal, ToOpen};

use crate::backend::{Backend, answer};

const CHOOSER_FAILED: &str = if cfg!(target_os = "linux") {
    "The file chooser could not open. Install xdg-desktop-portal with the portal for your desktop, then try again."
} else {
    "The file chooser could not open. Try again, and restart your computer if this keeps happening."
};

/// Whether a system file chooser is open in any window (D230).
#[derive(Default)]
struct ChooserOpen(Arc<AtomicBool>);

impl Global for ChooserOpen {}

/// The open file chooser's hold on [`ChooserOpen`], let go when dropped.
pub struct ChooserTurn(Arc<AtomicBool>);

impl Drop for ChooserTurn {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The turn to open a system file chooser; `None` while one is open. GPUI gives a chooser no owner
/// when the window that asked is not active, and an unowned one blocks nothing (D230).
pub fn chooser_turn(cx: &mut App) -> Option<ChooserTurn> {
    let open = cx.default_global::<ChooserOpen>().0.clone();
    (!open.swap(true, Ordering::AcqRel)).then(|| ChooserTurn(open))
}

/// Asks for a folder to save in; `None` when none is picked, while another file chooser is open,
/// or when its name is not valid text, which the window in front is told.
pub fn choose_folder(cx: &mut App) -> impl Future<Output = Option<String>> + use<> {
    let picked = chooser_turn(cx).map(|turn| {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Save here".into()),
        });
        (turn, paths)
    });
    let cx = cx.to_async();
    async move {
        let (turn, picked) = picked?;
        let answer = picked.await;
        drop(turn);
        let folder = chooser_answer(answer).and_then(|paths| {
            let Some(folder) = paths.and_then(|paths| paths.into_iter().next()) else {
                return Ok(None);
            };
            api::path_text(&folder).map(Some)
        });
        folder
            .map_err(|refused| {
                cx.update(|cx| {
                    if let Some(window) = crate::backend::window_in_front(cx) {
                        crate::closing::may_have_closed(window.update(cx, |_, window, cx| {
                            window.push_notification(refused.message, cx);
                        }));
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
    open_then(id, AfterHandoff::Keep, window, cx);
}

/// Opens the finished file and closes its download window after any program warning is accepted.
pub fn open_and_close(id: DownloadId, window: &mut Window, cx: &mut App) {
    open_then(id, AfterHandoff::Close, window, cx);
}

fn open_then(id: DownloadId, after: AfterHandoff, window: &mut Window, cx: &mut App) {
    let found = Backend::run(cx, move |manager| async move {
        api::file_to_open(&manager, id).await
    });
    window
        .spawn(cx, async move |cx| {
            let Some(opened) = answer(found.await) else {
                return;
            };
            crate::closing::may_have_closed(cx.update(|window, cx| match opened {
                Ok(ToOpen::File(path)) => {
                    handoff(after, window, cx, |cx| cx.open_with_system(&path));
                }
                Ok(ToOpen::Program { path, warning }) => {
                    ask_to_open_then(path, warning, after, window, cx);
                }
                Err(refused) => window.push_notification(refused.message, cx),
            }));
        })
        .detach();
}

#[derive(Clone, Copy)]
enum AfterHandoff {
    Keep,
    Close,
}

fn handoff(after: AfterHandoff, window: &mut Window, cx: &mut App, use_path: impl FnOnce(&App)) {
    use_path(cx);
    if matches!(after, AfterHandoff::Close) {
        crate::closing::close(window, cx);
    }
}

/// Asks in `window` before a program at `path` opens.
pub fn ask_to_open(path: PathBuf, warning: ProgramWarning, window: &mut Window, cx: &mut App) {
    ask_to_open_then(path, warning, AfterHandoff::Keep, window, cx);
}

fn ask_to_open_then(
    path: PathBuf,
    warning: ProgramWarning,
    after: AfterHandoff,
    window: &mut Window,
    cx: &mut App,
) {
    let question = Question {
        title: warning.title,
        body: warning.message,
        keep: "Cancel",
        go: "Open",
    };
    crate::ui::dialog::ask(question, window, cx, move |window, cx| {
        handoff(after, window, cx, |cx| cx.open_with_system(&path));
    });
}

/// Shows download `id`'s file selected in its folder, or opens the folder while the file is not
/// there yet.
pub fn show_in_folder(id: DownloadId, window: &mut Window, cx: &mut App) {
    show_location(id, FolderFocus::File, AfterHandoff::Keep, window, cx);
}

pub fn show_in_folder_and_close(id: DownloadId, window: &mut Window, cx: &mut App) {
    show_location(id, FolderFocus::File, AfterHandoff::Close, window, cx);
}

/// Opens download `id`'s folder without singling it out from other downloads there.
pub fn show_containing_folder(id: DownloadId, folder: PathBuf, window: &mut Window, cx: &mut App) {
    show_location(
        id,
        FolderFocus::Folder(folder),
        AfterHandoff::Keep,
        window,
        cx,
    );
}

enum FolderFocus {
    File,
    Folder(PathBuf),
}

impl FolderFocus {
    fn location(self, found: Reveal) -> Result<Reveal, ProtocolError> {
        let Self::Folder(expected) = self else {
            return Ok(found);
        };
        let folder = match &found {
            Reveal::File(path) => path.parent(),
            Reveal::Folder(path) => Some(path.as_path()),
        };
        if folder == Some(expected.as_path()) {
            Ok(Reveal::Folder(expected))
        } else {
            Err(api::failed(
                "A selected download moved to another folder. Check the selection and try again.",
            ))
        }
    }
}

fn show_location(
    id: DownloadId,
    focus: FolderFocus,
    after: AfterHandoff,
    window: &mut Window,
    cx: &mut App,
) {
    let found = Backend::run(cx, move |manager| async move {
        api::item_to_reveal(&manager, id).await
    });
    window
        .spawn(cx, async move |cx| {
            let Some(shown) = answer(found.await) else {
                return;
            };
            let location = shown.and_then(|found| focus.location(found));
            crate::closing::may_have_closed(cx.update(|window, cx| match location {
                Ok(location) => handoff(after, window, cx, |cx| match location {
                    Reveal::File(path) => cx.reveal_path(&path),
                    Reveal::Folder(folder) => cx.open_with_system(&folder),
                }),
                Err(refused) => window.push_notification(refused.message, cx),
            }));
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::{AppContext as _, Empty, TestAppContext, WindowOptions};

    use super::*;

    #[gpui_kit::test]
    fn a_completed_download_window_closes_after_file_handoff(cx: &mut TestAppContext) {
        let handed_off = Rc::new(Cell::new(false));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let handed_off = handed_off.clone();
                crate::closing::on_close(window, cx, move |_, _| handed_off.get());
                cx.new(|_| Empty)
            })
            .unwrap()
        });
        window
            .update(cx, |_, window, cx| {
                handoff(AfterHandoff::Close, window, cx, |_| handed_off.set(true));
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(cx.windows(), [] as [gpui_kit::AnyWindowHandle; 0]);
    }

    fn pick(cx: &TestAppContext, folder: &str) {
        cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from(folder)]));
    }

    #[gpui_kit::test]
    async fn a_second_folder_chooser_opens_only_once_the_first_closes(cx: &mut TestAppContext) {
        let first = cx.update(choose_folder);
        let second = cx.update(choose_folder);
        pick(cx, "first");
        assert!(
            !cx.did_prompt_for_paths(),
            "a second chooser opened over the first"
        );
        assert_eq!(first.await.as_deref(), Some("first"));
        assert_eq!(second.await, None);

        let third = cx.update(choose_folder);
        pick(cx, "third");
        assert_eq!(third.await.as_deref(), Some("third"));
    }

    type Answer = Result<Result<Option<u8>, &'static str>, ()>;

    #[test]
    fn revealing_a_shared_folder_never_follows_a_download_that_moved() {
        let expected = PathBuf::from("Downloads");
        let file = Reveal::File(expected.join("file.zip"));
        assert_eq!(
            FolderFocus::Folder(expected.clone())
                .location(file)
                .unwrap(),
            Reveal::Folder(expected.clone())
        );
        let moved = Reveal::File(PathBuf::from("Elsewhere").join("file.zip"));
        assert!(FolderFocus::Folder(expected).location(moved).is_err());
    }

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
