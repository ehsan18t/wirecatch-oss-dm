//! What the list says when it has nothing to show, with the way on: the manager could not open,
//! nothing was ever added, the search found nothing, the queue is empty, or the view is.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement as _, Styled as _, div, px};
use wirecatch_protocol::QueueId;

use super::{Group, MainWindow, View};
use crate::add_dialog;
use crate::library::{Library, Loaded};
use crate::ui::{Role, Typed as _};

/// The widest the note under an empty list grows.
const NOTE_WIDTH: f32 = 440.;

struct Empty {
    icon: IconName,
    title: String,
    note: String,
    actions: Vec<AnyElement>,
}

impl MainWindow {
    pub(super) fn empty(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let needle = self.search.read(cx).value().trim().to_owned();
        let library = self.library.read(cx);
        let empty = if let Loaded::Failed(reason) = &library.loaded {
            unreachable_engine(reason.message.clone(), cx)
        } else if library.downloads.is_empty() {
            first_run(cx)
        } else if !needle.is_empty() {
            self.no_match(&needle, cx)
        } else if let View::Queue(queue) = self.view {
            self.empty_queue(queue, cx)
        } else {
            self.empty_view(cx)
        };
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .px_6()
            .child(
                Icon::new(empty.icon)
                    .large()
                    .text_color(theme.muted_foreground),
            )
            .child(div().text_role(Role::Title, cx).child(empty.title))
            .child(
                div()
                    .max_w(px(NOTE_WIDTH))
                    .text_center()
                    .text_role(Role::Secondary, cx)
                    .child(empty.note),
            )
            .child(h_flex().pt_2().gap_2().children(empty.actions))
    }

    fn no_match(&self, needle: &str, cx: &mut Context<'_, Self>) -> Empty {
        if !self.typed_links(cx).is_empty() {
            return not_downloaded(cx);
        }
        let mut actions = vec![
            Button::new("clear-search")
                .label("Clear the search")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.search
                        .update(cx, |search, cx| search.set_value("", window, cx));
                    cx.notify();
                }))
                .into_any_element(),
        ];
        if self.view != View::All {
            actions.push(
                Button::new("search-all")
                    .ghost()
                    .label("Search all downloads")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let needle = this.search.read(cx).value();
                        this.show(View::All, window, cx);
                        this.search
                            .update(cx, |search, cx| search.set_value(needle, window, cx));
                    }))
                    .into_any_element(),
            );
        }
        Empty {
            icon: IconName::Search,
            title: format!("Nothing matches “{needle}”"),
            note: "The search looks at file names and links in this view.".to_owned(),
            actions,
        }
    }

    fn empty_queue(&self, queue: QueueId, cx: &mut Context<'_, Self>) -> Empty {
        let name = self.title(cx);
        let other = self
            .library
            .read(cx)
            .queues
            .iter()
            .find(|q| q.id != queue)
            .map_or_else(|| "another queue".to_owned(), |q| q.settings.name.clone());
        Empty {
            icon: IconName::Inbox,
            title: format!("{name} is empty"),
            note: format!(
                "Add a download straight into this queue, or drag downloads onto it in the sidebar from {other} or any other list."
            ),
            actions: vec![
                Button::new("add-to-queue")
                    .primary()
                    .icon(IconName::Plus)
                    .label(format!("Add to {name}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        add_dialog::open(this.library.clone(), &[], Some(queue), window, cx);
                    }))
                    .into_any_element(),
            ],
        }
    }

    fn empty_view(&self, cx: &mut Context<'_, Self>) -> Empty {
        let (icon, title) = match self.view {
            View::Group(Group::Attention) => (
                IconName::CircleCheck,
                "Nothing needs your attention".to_owned(),
            ),
            View::Group(group) => (
                IconName::Inbox,
                format!("Nothing {}", group.label().to_lowercase()),
            ),
            View::All | View::Queue(_) => (IconName::Inbox, "No downloads here".to_owned()),
        };
        Empty {
            icon,
            title,
            note: "Downloads appear here as soon as they match this view.".to_owned(),
            actions: vec![
                Button::new("show-all")
                    .label("Show all downloads")
                    .on_click(cx.listener(|this, _, window, cx| this.show(View::All, window, cx)))
                    .into_any_element(),
            ],
        }
    }
}

fn unreachable_engine(reason: String, cx: &mut Context<'_, MainWindow>) -> Empty {
    let app = wirecatch_shell::host::CHANNEL.name();
    Empty {
        icon: IconName::CircleX,
        title: format!("{app} cannot reach its download engine"),
        note: reason,
        actions: vec![
            Button::new("reload")
                .primary()
                .label("Try again")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.library.update(cx, |_, cx| Library::load(cx));
                }))
                .into_any_element(),
        ],
    }
}

/// The field holds links none of the downloads has.
fn not_downloaded(cx: &mut Context<'_, MainWindow>) -> Empty {
    Empty {
        icon: IconName::Globe,
        title: "Not in your downloads yet".to_owned(),
        note: "Press Enter to download it, or keep typing to search.".to_owned(),
        actions: vec![
            Button::new("download-typed-empty")
                .primary()
                .label("Download")
                .on_click(cx.listener(|this, _, window, cx| this.download_typed(window, cx)))
                .into_any_element(),
        ],
    }
}

fn first_run(cx: &mut Context<'_, MainWindow>) -> Empty {
    let app = wirecatch_shell::host::CHANNEL.name();
    Empty {
        icon: IconName::ArrowDown,
        title: "Copy a link, then press Ctrl+V here".to_owned(),
        note: format!(
            "{app} downloads it right away, using several connections at once. Downloads you start in your browser show up here too once the {} extension is installed.",
            wirecatch_product::PRODUCT_NAME
        ),
        actions: vec![
            Button::new("first-download")
                .primary()
                .icon(IconName::Plus)
                .label("New download")
                .on_click(cx.listener(|this, _, window, cx| this.new_download(&[], window, cx)))
                .into_any_element(),
            Button::new("first-tour")
                .ghost()
                .label(format!("See everything {app} can do"))
                .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)))
                .into_any_element(),
        ],
    }
}
