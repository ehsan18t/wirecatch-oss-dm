//! The standalone window that watches for a fresh browser capture.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window,
    WindowKind, WindowOptions, div, px, size,
};
use wirecatch_core::{LinkFrom, LinkWatch};
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::DownloadId;
use wirecatch_shell::api;

use crate::backend::{Backend, said};
use crate::library::Library;
use crate::links;
use crate::ui::small_title_bar::close_only_title_bar;
use crate::ui::{PathText, Role, Typed as _};

const WIDTH: f32 = 400.;
const HEIGHT: f32 = 132.;
const TITLE: &str = "Get a new link";

enum Watch {
    Waiting { opened_page: bool },
    Over,
}

struct RefreshWindow {
    id: DownloadId,
    file_name: String,
    media: bool,
    link: Entity<InputState>,
    read_link: links::Kept,
    watch: Watch,
    pasting: bool,
    busy: bool,
    problem: Option<String>,
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
pub fn open(library: &Entity<Library>, id: DownloadId, window: &mut Window, cx: &mut App) {
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
        library: &Entity<Library>,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let row = library.read(cx).downloads.iter().find(|row| row.id == id);
        let (file_name, media) = match row {
            Some(row) => (row.file_name.clone(), row.media.is_some()),
            None => (String::new(), false),
        };
        let link = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let edited = cx.subscribe_in(
            &link,
            window,
            |form: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    form.problem = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => form.save(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        Self {
            id,
            file_name,
            media,
            link,
            read_link: links::Kept::default(),
            watch: Watch::Waiting { opened_page: true },
            pasting: false,
            busy: false,
            problem: None,
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
                Some(Ok(LinkWatch::Started(page))) => page,
                answer => {
                    crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                        form.watch_ended(answer.and_then(Result::err), window, cx);
                    }));
                    return;
                }
            };
            let opened = form.update_in(cx, |form, _, cx| {
                form.watch = Watch::Waiting {
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
                    window.remove_window();
                } else {
                    form.watch_ended(None, window, cx);
                }
            }));
        }));
    }

    /// Runs inside the watching task, which therefore stays in `watching`.
    fn watch_ended(
        &mut self,
        problem: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.watch = Watch::Over;
        if problem.is_some() {
            self.problem = problem;
        }
        self.show_paste(window, cx);
    }

    fn show_paste(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.pasting = true;
        crate::ui::dialog::focus_after_opening(&self.link, window, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window) {
        self.stop_watching();
        window.remove_window();
    }

    fn stop_watching(&mut self) {
        self.stop.cancel();
        self.watch = Watch::Over;
        self.watching = None;
    }

    fn is_watching(&self) -> bool {
        matches!(self.watch, Watch::Waiting { .. })
    }

    /// The link in the field, if it holds one.
    fn found(&self, cx: &App) -> Option<String> {
        let found = self.read_link.of(&self.link.read(cx).value());
        found.whole.clone().or_else(|| found.urls.first().cloned())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(url) = self.found(cx).filter(|_| self.pasting && !self.busy) else {
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
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                form.busy = false;
                match answer {
                    Ok(()) => form.close(window),
                    Err(message) => {
                        form.problem = Some(message);
                        cx.notify();
                    }
                }
            }));
        })
        .detach();
    }

    /// The line under the file name, and whether it says something is wrong.
    fn line(&self, cx: &App) -> (SharedString, bool) {
        if !self.pasting {
            let line = if self.media {
                "Use the download button on the video in your browser."
            } else if matches!(self.watch, Watch::Waiting { opened_page: false }) {
                "Start this download again in your browser."
            } else {
                "Waiting for the download to start in your browser."
            };
            return (line.into(), false);
        }
        if let Some(problem) = &self.problem {
            return (problem.clone().into(), true);
        }
        let typed = !self.link.read(cx).value().trim().is_empty();
        if typed && self.found(cx).is_none() {
            ("Paste the whole link, starting with https://.".into(), true)
        } else {
            (
                "Copy the new link on the download's page and paste it here.".into(),
                false,
            )
        }
    }

    fn controls(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let row = h_flex().mt_auto().gap_2();
        if !self.pasting {
            return row
                .justify_end()
                .child(
                    Button::new("refresh-paste")
                        .small()
                        .ghost()
                        .label("Paste a link")
                        .on_click(cx.listener(|form, _, window, cx| form.show_paste(window, cx))),
                )
                .child(
                    Button::new("refresh-cancel")
                        .small()
                        .label("Cancel")
                        .on_click(cx.listener(|form, _, window, _| form.close(window))),
                );
        }
        row.child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&self.link).small()),
        )
        .children(self.is_watching().then(|| {
            Button::new("refresh-back")
                .small()
                .ghost()
                .label("Back")
                .on_click(cx.listener(|form, _, _, cx| {
                    form.pasting = false;
                    cx.notify();
                }))
        }))
        .child(
            Button::new("refresh-save")
                .small()
                .primary()
                .label("Use link")
                .disabled(self.found(cx).is_none())
                .loading(self.busy)
                .on_click(cx.listener(|form, _, window, cx| form.save(window, cx))),
        )
    }
}

impl Render for RefreshWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let (background, danger) = (cx.theme().background, cx.theme().danger);
        let (line, wrong) = self.line(cx);
        let file = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(
                div()
                    .text_role(Role::Strong, cx)
                    .child(PathText::new(self.file_name.clone()).tooltip("refresh-file-name")),
            )
            .child(
                div()
                    .text_role(Role::Meta, cx)
                    .when(wrong, |text| text.text_color(danger))
                    .child(line),
            );
        v_flex()
            .size_full()
            .bg(background)
            .text_role(Role::Body, cx)
            .child(close_only_title_bar(
                div().text_role(Role::Strong, cx).child(TITLE),
                window,
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .p_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .when(!self.pasting, |row| row.child(Spinner::new().small()))
                            .child(file),
                    )
                    .child(self.controls(cx)),
            )
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
                    open(&library, id, window, cx);
                    assert!(!window.has_active_dialog(cx));
                    open(&library, id, window, cx);
                })
                .unwrap();
            assert_eq!(cx.windows().len(), 2);
            assert_ne!(cx.global::<OpenWatch>().window, parent);
            let previous = cx.global::<OpenWatch>().window;
            let view = cx.global::<OpenWatch>().view.upgrade().unwrap();
            view.update(cx, |view, _| view.watch = Watch::Over);
            parent
                .update(cx, |_, window, cx| open(&library, id, window, cx))
                .unwrap();
            assert_ne!(cx.global::<OpenWatch>().window, previous);
        });
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 2);
    }
}
