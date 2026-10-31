//! Adding downloads by hand: links pasted or typed, one per line, then the file name for a single
//! link, where they are saved, in which queue (Automatic, where the rules send each, unless one is
//! picked), and whether they start now. A link to a video page is read for its qualities (D51).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectItem, SelectState};
use gpui_kit::component::{Disableable as _, IndexPath, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, WeakEntity, Window, px,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AddRequest, Setting};
use wirecatch_protocol::rules::{LinkTrial, RuleMatch};
use wirecatch_shell::api;
use wirecatch_sites::video_sites::video_site_for;

use crate::backend::Backend;
use crate::library::Library;
use crate::ui::{Callout, Field, FolderRow};
use crate::{files, links, text};

/// Said when the box holds no link to send.
/// Where links go with no folder picked and no queue folder.
const FOLDER_BY_TYPE: &str = "By file type (from Settings)";

const NO_LINK: &str = "Paste at least one link that starts with https://, http:// or ftp://.";

/// One entry of the queue list: Automatic, or a queue.
#[derive(Clone)]
struct QueueChoice {
    title: SharedString,
    queue: Option<QueueId>,
}

impl SelectItem for QueueChoice {
    type Value = Option<QueueId>;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.queue
    }
}

/// The form inside the dialog.
pub struct AddForm {
    library: Entity<Library>,
    links: Entity<TextareaState>,
    file_name: Entity<InputState>,
    queue: Entity<SelectState<Vec<QueueChoice>>>,
    folder: Option<String>,
    /// Why the links cannot be sent as they are.
    invalid: Option<String>,
    /// What went wrong sending them: a title and the reason.
    problem: Option<(String, String)>,
    busy: bool,
    /// Where Automatic sends the links, once the rules were tried on them.
    routes: Vec<Route>,
    /// Bumped on each change of the links, so an older trial's answer is dropped.
    tried: u64,
    _subscriptions: Vec<Subscription>,
}

/// Where the rules send one link: its queue's name and the rule that sends it there.
struct Route {
    queue: String,
    rule: Option<String>,
}

/// Links tried against the rules at most, for the hint under Automatic.
const MAX_TRIED: usize = 20;

/// Opens the Add dialog in `window`, with `urls` already in it and `queue` picked; the form, while
/// the dialog is open.
pub fn open(
    library: Entity<Library>,
    urls: &[String],
    queue: Option<QueueId>,
    window: &mut Window,
    cx: &mut App,
) -> WeakEntity<AddForm> {
    let form = cx.new(|cx| AddForm::new(library, urls, queue, window, cx));
    let opened = form.downgrade();
    let links = form.read(cx).links.clone();
    let empty = urls.is_empty();
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let count = form.read(cx).link_count(cx);
        let send = form.clone();
        let paused = form.clone();
        let busy = form.read(cx).busy;
        let confirmed = form.clone();
        dialog
            .title(if count > 1 {
                format!("{count} new downloads")
            } else {
                "New download".to_owned()
            })
            .w(px(560.))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                confirmed.update(cx, |form, cx| form.submit(true, window, cx));
                false
            })
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("add-paused")
                            .label("Add paused")
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                paused.update(cx, |form, cx| form.submit(false, window, cx));
                            }),
                    )
                    .child(
                        Button::new("add-now")
                            .primary()
                            .label(if count > 1 {
                                format!("Download {count}")
                            } else {
                                "Download".to_owned()
                            })
                            .loading(busy)
                            .on_click(move |_, window, cx| {
                                send.update(cx, |form, cx| form.submit(true, window, cx));
                            }),
                    ),
            )
    });
    // With links in it, the dialog keeps the keyboard so Enter downloads them.
    if empty {
        crate::ui::dialog::focus_after_opening(&links, window, cx);
    }
    opened
}

impl AddForm {
    fn new(
        library: Entity<Library>,
        urls: &[String],
        queue: Option<QueueId>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let links = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("https://example.com/file.zip")
        });
        links.update(cx, |state, cx| state.set_value(urls.join("\n"), window, cx));
        let file_name = cx.new(|cx| InputState::new(window, cx));
        let mut choices = vec![QueueChoice {
            title: "Automatic".into(),
            queue: None,
        }];
        choices.extend(library.read(cx).queues.iter().map(|q| QueueChoice {
            title: q.settings.name.clone().into(),
            queue: Some(q.id),
        }));
        let picked = choices
            .iter()
            .position(|choice| choice.queue == queue)
            .unwrap_or(0);
        let queue = cx.new(|cx| {
            SelectState::new(choices, Some(IndexPath::default().row(picked)), window, cx)
        });
        let edited = cx.subscribe_in(
            &links,
            window,
            |form: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    form.invalid = None;
                    form.route(cx);
                    cx.notify();
                }
                InputEvent::PressEnter {
                    secondary: true, ..
                } => form.submit(true, window, cx),
                _ => {}
            },
        );
        let redraw = cx.observe(&queue, |_, _, cx| cx.notify());
        let mut form = Self {
            library,
            links,
            file_name,
            queue,
            folder: None,
            invalid: None,
            problem: None,
            busy: false,
            routes: Vec::new(),
            tried: 0,
            _subscriptions: vec![edited, redraw],
        };
        form.route(cx);
        form
    }

    /// Adds `urls` that are not in the box yet: links from a second launch while it is open.
    pub fn add_links(&mut self, urls: &[String], window: &mut Window, cx: &mut Context<'_, Self>) {
        let mut all = self.urls(cx);
        all.extend(
            urls.iter()
                .filter(|url| !all.contains(url))
                .cloned()
                .collect::<Vec<_>>(),
        );
        self.links
            .update(cx, |state, cx| state.set_value(all.join("\n"), window, cx));
        self.route(cx);
        cx.notify();
    }

    /// Tries the links against the queues' rules, for the hint under Automatic.
    fn route(&mut self, cx: &mut Context<'_, Self>) {
        self.tried += 1;
        let tried = self.tried;
        let urls: Vec<String> = self.urls(cx).into_iter().take(MAX_TRIED).collect();
        let matched = Backend::run(cx, move |manager| async move {
            urls.iter()
                .map(|url| {
                    api::try_link(
                        &manager,
                        &LinkTrial {
                            url: url.clone(),
                            draft: None,
                        },
                    )
                    .ok()
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |form, cx| {
            let Ok(Ok(matched)) = matched.await else {
                return;
            };
            _ = form.update(cx, |form, cx| {
                if form.tried != tried {
                    return;
                }
                form.routes = matched
                    .into_iter()
                    .map(|found| form.route_of(found.and_then(|m| m.matched), cx))
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    /// The queue a rule match names, or the default queue when none matched.
    fn route_of(&self, matched: Option<RuleMatch>, cx: &App) -> Route {
        let library = self.library.read(cx);
        let name = |id: Option<QueueId>| {
            id.and_then(|id| library.queue(id))
                .or_else(|| library.queues.first())
                .map(|queue| queue.settings.name.clone())
                .unwrap_or_default()
        };
        match matched {
            Some(found) => Route {
                queue: name(found.queue),
                rule: Some(text::rule_phrase(&found.rule)),
            },
            None => Route {
                queue: name(None),
                rule: None,
            },
        }
    }

    /// Under Automatic: where the links go.
    fn automatic_hint(&self) -> String {
        match self.routes.as_slice() {
            [] => "Each link goes to the queue whose rules it matches, or the default queue."
                .to_owned(),
            [only] => match &only.rule {
                Some(rule) => format!("Goes to {}: {rule}.", only.queue),
                None => format!("Goes to {}, the default queue.", only.queue),
            },
            routes => {
                let mut queues: Vec<&str> = Vec::new();
                for route in routes {
                    if !queues.contains(&route.queue.as_str()) {
                        queues.push(&route.queue);
                    }
                }
                match queues.as_slice() {
                    [one] => format!("All go to {one}."),
                    many => format!("They go to {} queues: {}.", many.len(), many.join(", ")),
                }
            }
        }
    }

    fn urls(&self, cx: &App) -> Vec<String> {
        links::extract(&self.links.read(cx).value())
    }

    fn link_count(&self, cx: &App) -> usize {
        self.urls(cx).len()
    }

    fn picked_queue(&self, cx: &App) -> Option<QueueId> {
        self.queue.read(cx).selected_value().copied().flatten()
    }

    fn choose_folder(cx: &mut Context<'_, Self>) {
        let picked = files::choose_folder(cx);
        cx.spawn(async move |form, cx| {
            let Some(folder) = picked.await else {
                return;
            };
            _ = form.update(cx, |form, cx| {
                form.folder = Some(folder);
                cx.notify();
            });
        })
        .detach();
    }

    /// Sends every link, started now or paused. Links that fail stay in the box with why; once
    /// every one is a download, the dialog closes.
    fn submit(&mut self, start: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.busy {
            return;
        }
        let urls = self.urls(cx);
        if urls.is_empty() {
            self.invalid = Some(NO_LINK.to_owned());
            cx.notify();
            return;
        }
        let name = self.file_name.read(cx).value().trim().to_owned();
        let file_name = (urls.len() == 1 && !name.is_empty()).then_some(name);
        let queue = self.picked_queue(cx);
        let folder = self.folder.clone();
        let video_sites = self
            .library
            .read(cx)
            .settings
            .as_ref()
            .map(|settings| settings.extension.video_sites.clone())
            .unwrap_or_default();
        let requests: Vec<AddRequest> = urls
            .iter()
            .map(|url| AddRequest {
                url: url.as_str().into(),
                queue,
                folder: folder.clone(),
                file_name: file_name.clone(),
                start,
            })
            .collect();
        self.busy = true;
        self.problem = None;
        cx.notify();
        let sent = Backend::run(cx, move |manager| async move {
            let mut failed = Vec::new();
            let mut added = 0_usize;
            for request in requests {
                let url = request.url.as_str().to_owned();
                let done = if video_site_for(&url, &video_sites).is_some() {
                    api::add_video_page(&manager, request).map(|_| ())
                } else {
                    api::add_download(&manager, request)
                        .await
                        .map(|_| added += 1)
                };
                if let Err(error) = done {
                    failed.push((url, error.message));
                }
            }
            (added, failed)
        });
        cx.spawn_in(window, async move |form, cx| {
            let (added, failed) = match sent.await {
                Ok(Ok(result)) => result,
                Ok(Err(unopened)) => (
                    0,
                    urls.iter()
                        .map(|url| (url.clone(), unopened.reason.message.clone()))
                        .collect(),
                ),
                Err(_) => return,
            };
            _ = form.update_in(cx, |form, window, cx| {
                form.sent(urls.len(), added, &failed, window, cx);
            });
        })
        .detach();
    }

    /// What became of the links sent: the dialog closes once all are downloads, and otherwise
    /// keeps the ones that failed with why.
    fn sent(
        &mut self,
        sent: usize,
        added: usize,
        failed: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let form = self;
        form.busy = false;
        if added > 0 {
            window.push_notification(
                if added == 1 {
                    "Download added.".to_owned()
                } else {
                    format!("{added} downloads added.")
                },
                cx,
            );
        }
        if failed.is_empty() {
            window.close_dialog(cx);
            return;
        }
        let left: Vec<_> = failed.iter().map(|(url, _)| url.clone()).collect();
        form.links
            .update(cx, |state, cx| state.set_value(left.join("\n"), window, cx));
        let Some((_, first)) = failed.first() else {
            return;
        };
        form.problem = Some(if sent == 1 {
            ("Could not add the link".to_owned(), first.clone())
        } else {
            (
                format!("Could not add {} of {sent} links", failed.len()),
                format!(
                    "{first} {} here to try again.",
                    if failed.len() == 1 {
                        "The link stays"
                    } else {
                        "The links stay"
                    }
                ),
            )
        });
        cx.notify();
    }
}

impl Render for AddForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let folder_row = self.folder_row(cx);
        let raw = self.links.read(cx).value();
        let urls = links::extract(&raw);
        let ignored = links::lines_without_links(&raw);
        let hint = match ignored {
            0 => "One link per line, or paste several at once.".to_owned(),
            1 => "One line has no link in it and is left out.".to_owned(),
            n => format!("{n} lines have no link in them and are left out."),
        };
        let queue_hint = if self.picked_queue(cx).is_none() {
            self.automatic_hint()
        } else {
            "For this add only.".to_owned()
        };
        v_flex()
            .gap_4()
            .text_sm()
            .pb_1()
            .when_some(self.problem.clone(), |form, (title, message)| {
                form.child(Callout::danger(message).title(title))
            })
            .child(
                Field::new("Links", Textarea::new(&self.links).aria_label("Links"))
                    .note(self.invalid.clone().unwrap_or(hint))
                    .wrong(self.invalid.is_some()),
            )
            .when(urls.len() <= 1, |form| {
                form.child(
                    Field::new("File name", Input::new(&self.file_name))
                        .note("Leave empty for the name the server gives."),
                )
            })
            .child(Field::new("Save to", folder_row))
            .child(Field::new("Queue", Select::new(&self.queue)).note(queue_hint))
    }
}

impl AddForm {
    /// Where the links are saved: the folder picked, or the one the app picks, the queue's own
    /// folder or the folder by file type (D94).
    fn folder_row(&self, cx: &mut Context<'_, Self>) -> FolderRow {
        let fallback = match self
            .picked_queue(cx)
            .and_then(|id| self.library.read(cx).queue(id))
            .map(|queue| &queue.settings.folder)
        {
            Some(Setting::Own(folder)) => text::folder(&folder.0),
            Some(Setting::Global) | None => FOLDER_BY_TYPE.to_owned(),
        };
        FolderRow::new(
            self.folder.clone(),
            fallback,
            cx.listener(|_, _, _, cx| Self::choose_folder(cx)),
            cx.listener(|form, _, _, cx| {
                form.folder = None;
                cx.notify();
            }),
        )
    }
}
