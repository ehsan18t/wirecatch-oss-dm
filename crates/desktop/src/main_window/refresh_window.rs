//! The standalone window that watches for a fresh browser capture.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, TitleBar, h_flex, v_flex};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Task, WeakEntity, Window, WindowKind,
    WindowOptions, div, px, size,
};
use wirecatch_core::LinkFrom;
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::extension::MediaKind;
use wirecatch_shell::api;

use crate::backend::{Backend, said};
use crate::library::Library;
use crate::ui::{Field, PathText, Role, Typed as _};
use crate::{links, text};

const WIDTH: f32 = 470.;
const HEIGHT: f32 = 310.;
const TITLE: &str = "Get a new link";
const LINK_SAVED: &str =
    "New link saved. The download continues where it stopped if the server allows it.";

enum Mode {
    Opening,
    Watching { opened_page: bool },
    Paste,
    Saved,
}

struct RefreshWindow {
    library: Entity<Library>,
    id: DownloadId,
    link: Entity<InputState>,
    read_link: links::Kept,
    busy: bool,
    problem: Option<String>,
    mode: Mode,
    watching: Option<Task<()>>,
    stop: CancellationToken,
    _edited: Subscription,
}

struct OpenWatch {
    id: DownloadId,
    window: AnyWindowHandle,
    view: WeakEntity<RefreshWindow>,
}

impl Global for OpenWatch {}

impl Drop for RefreshWindow {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    let mut options = crate::popup_window_options(bounds);
    if cfg!(windows) {
        options.kind = WindowKind::PopUp;
    }
    options.is_minimizable = false;
    if let Some(titlebar) = &mut options.titlebar {
        titlebar.title = Some(TITLE.into());
    }
    options
}

/// Opens the watch in its own window, including from a progress window.
pub fn open(library: Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
    let earlier = cx
        .try_global::<OpenWatch>()
        .map(|watch| (watch.id, watch.window, watch.view.clone()));
    if let Some((open_id, earlier, view)) =
        earlier.filter(|(_, handle, _)| cx.windows().contains(handle))
    {
        if open_id == id
            && view
                .upgrade()
                .is_some_and(|view| view.read(cx).is_watching())
        {
            crate::closing::may_have_closed(
                earlier.update(cx, |_, window, _| window.activate_window()),
            );
            return;
        }
        crate::closing::may_have_closed(earlier.update(cx, |_, window, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |view, _| view.stop_watching());
            }
            window.remove_window();
        }));
    }
    let options = options(cx);
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| {
            let mut view = RefreshWindow::new(library, id, window, cx);
            view.watch(window, cx);
            view
        });
        let closing = view.downgrade();
        crate::closing::on_close(window, cx, move |_, cx| {
            if let Some(view) = closing.upgrade() {
                view.update(cx, |view, _| view.stop_watching());
            }
            true
        });
        view
    });
    match opened {
        Ok((handle, view)) => {
            cx.set_global(OpenWatch {
                id,
                window: handle,
                view: view.downgrade(),
            });
            crate::closing::may_have_closed(
                handle.update(cx, |_, window, _| window.activate_window()),
            );
        }
        Err(error) => {
            use gpui_kit::component::WindowExt as _;
            tracing::warn!("could not open the refresh window: {error}");
            window.push_notification("The refresh window could not open.", cx);
        }
    }
}

impl RefreshWindow {
    fn new(
        library: Entity<Library>,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let link = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let edited = cx.subscribe(&link, |form: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                form.problem = None;
                cx.notify();
            }
        });
        Self {
            library,
            id,
            link,
            read_link: links::Kept::default(),
            busy: false,
            problem: None,
            mode: Mode::Opening,
            watching: None,
            stop: CancellationToken::new(),
            _edited: edited,
        }
    }

    fn watch(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let id = self.id;
        let (saved, result) = tokio::sync::oneshot::channel();
        let stop = self.stop.clone();
        let reading = Backend::run(cx, move |manager| async move {
            api::watch_link(&manager, id, saved, stop)
        });
        self.watching = Some(cx.spawn_in(window, async move |form, cx| {
            let page = match said(reading.await) {
                Some(Ok(page)) => page,
                answer => {
                    crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                        if let Some(Err(problem)) = answer {
                            form.problem = Some(problem);
                        }
                        form.show_paste(window, cx);
                    }));
                    return;
                }
            };
            let opened = form.update_in(cx, |form, _, cx| {
                form.mode = Mode::Watching {
                    opened_page: page.is_some(),
                };
                if let Some(page) = page {
                    cx.open_url(page.as_str());
                }
                cx.notify();
            });
            if opened.is_err() {
                crate::closing::may_have_closed(opened);
                return;
            }
            let saved = result.await.is_ok();
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                if saved {
                    form.mode = Mode::Saved;
                    cx.notify();
                } else {
                    form.problem =
                        Some("The browser watch ended. You can paste a new link.".into());
                    form.show_paste(window, cx);
                }
            }));
        }));
    }

    fn paste(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.stop_watching();
        self.show_paste(window, cx);
    }

    fn show_paste(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.mode = Mode::Paste;
        crate::ui::dialog::focus_after_opening(&self.link, window, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window) {
        self.stop_watching();
        window.remove_window();
    }

    fn stop_watching(&mut self) {
        self.stop.cancel();
        self.watching = None;
    }

    fn is_watching(&self) -> bool {
        matches!(self.mode, Mode::Opening | Mode::Watching { .. })
    }

    fn buttons(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let watching = self.is_watching();
        h_flex()
            .mt_auto()
            .gap_2()
            .justify_end()
            .child(
                Button::new("refresh-stop")
                    .label(if watching { "Stop watching" } else { "Close" })
                    .on_click(cx.listener(|form, _, window, _| form.close(window))),
            )
            .children(watching.then(|| {
                Button::new("refresh-paste")
                    .label("Paste a link")
                    .on_click(cx.listener(|form, _, window, cx| form.paste(window, cx)))
            }))
            .children(matches!(self.mode, Mode::Paste).then(|| {
                Button::new("refresh-save")
                    .primary()
                    .label("Use this link")
                    .disabled(self.found(cx).is_none())
                    .loading(self.busy)
                    .on_click(cx.listener(|form, _, window, cx| form.save(window, cx)))
            }))
    }

    /// The link in the field, if it holds one.
    fn found(&self, cx: &App) -> Option<String> {
        let found = self.read_link.of(&self.link.read(cx).value());
        found.whole.clone().or_else(|| found.urls.first().cloned())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(url) = self
            .found(cx)
            .filter(|_| !self.busy && matches!(self.mode, Mode::Paste))
        else {
            return;
        };
        self.busy = true;
        cx.notify();
        let id = self.id;
        let saved = Backend::run(cx, move |manager| async move {
            api::refresh_link(&manager, id, &url, LinkFrom::Person)
        });
        cx.spawn_in(window, async move |form, cx| {
            let Some(answer) = said(saved.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update_in(cx, |form, _, cx| {
                form.busy = false;
                match answer {
                    Ok(()) => {
                        form.mode = Mode::Saved;
                    }
                    Err(message) => form.problem = Some(message),
                }
                cx.notify();
            }));
        })
        .detach();
    }
}

impl Render for RefreshWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let typed = !self.link.read(cx).value().trim().is_empty();
        let wrong = typed && self.found(cx).is_none();
        let row = self
            .library
            .read(cx)
            .downloads
            .iter()
            .find(|row| row.id == self.id)
            .cloned();
        let paste_note = if let Some(problem) = &self.problem {
            problem.clone()
        } else if wrong {
            "Paste the whole link, starting with https://.".to_owned()
        } else {
            let page_video = row
                .as_ref()
                .and_then(|row| row.media.as_ref())
                .is_some_and(|media| media.source == MediaKind::Page);
            let kept = if page_video {
                "A video read from a page starts over if the link is a different one."
            } else {
                "What is already downloaded is kept if the server can continue the download; otherwise it starts over."
            };
            format!(
                "Links from some sites stop working after a while. Start the download again on its page, copy the new link and paste it here. {kept}"
            )
        };
        let paste = matches!(self.mode, Mode::Paste);
        let content = v_flex()
            .flex_1()
            .gap_4()
            .p_4()
            .children(row.map(|row| {
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_role(Role::Strong, cx)
                            .child(PathText::new(row.file_name).tooltip("refresh-file-name")),
                    )
                    .child(
                        div()
                            .text_role(Role::Meta, cx)
                            .child(format!("from {}", text::host(&row.url))),
                    )
            }))
            .children(paste.then(|| {
                Field::new("New link", Input::new(&self.link))
                    .note(paste_note)
                    .wrong(wrong || self.problem.is_some())
            }))
            .children(match self.mode {
                Mode::Opening => Some("Starting the browser watch…"),
                Mode::Watching { opened_page: true } => Some("Start the same download again on the page that opened. For media, use the extension’s download button."),
                Mode::Watching { opened_page: false } => Some("Return to the original page in your browser and start the same download again. For media, use the extension’s download button."),
                Mode::Saved => Some(LINK_SAVED),
                Mode::Paste => None,
            }.map(|note| div().text_role(Role::Secondary, cx).child(note)))
            .child(self.buttons(cx));
        v_flex()
            .size_full()
            .child(
                TitleBar::new()
                    .on_close_window(|_, window, cx| crate::closing::close(window, cx))
                    .child(div().pl_3().text_role(Role::Strong, cx).child(TITLE)),
            )
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::WindowExt as _;

    use super::*;

    struct Parent;

    impl Render for Parent {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn get_a_new_link_opens_one_separate_window_and_can_start_again(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(Backend::unopened());
            if cfg!(windows) {
                assert_eq!(options(cx).kind, WindowKind::PopUp);
            }
            let library = cx.new(|_| Library::empty());
            let (parent, _) =
                gpui_kit::open_window(crate::window_options(), cx, |_, cx| cx.new(|_| Parent))
                    .unwrap();
            let id = DownloadId(1);
            parent
                .update(cx, |_, window, cx| {
                    open(library.clone(), id, window, cx);
                    assert!(!window.has_active_dialog(cx));
                    open(library.clone(), id, window, cx);
                })
                .unwrap();
            assert_eq!(cx.windows().len(), 2);
            assert_ne!(cx.global::<OpenWatch>().window, parent);
            let previous = cx.global::<OpenWatch>().window;
            let view = cx.global::<OpenWatch>().view.upgrade().unwrap();
            view.update(cx, |view, _| view.mode = Mode::Saved);
            parent
                .update(cx, |_, window, cx| open(library, id, window, cx))
                .unwrap();
            assert_ne!(cx.global::<OpenWatch>().window, previous);
        });
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 2);
    }
}
