//! Dragging in the main window: downloads within a queue to reorder it, or onto a queue to move
//! them there; queues to reorder them, the default queue staying first (D72.9); and Internet
//! shortcut files from outside, whose links open the Add dialog. GPUI takes only files from
//! other programs, so a link dragged from a browser cannot be dropped.

use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex};
use gpui_kit::{
    Context, ExternalPaths, IntoElement, ParentElement as _, Render, Styled as _, Window,
};
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use super::{MainWindow, View, order};
use crate::backend::act;
use crate::ui::{Role, Typed as _};

/// Downloads being dragged, with what the pointer shows.
#[derive(Clone)]
pub struct DraggedRows {
    pub ids: Vec<DownloadId>,
    label: String,
}

/// A queue being dragged, with its name for the pointer.
#[derive(Clone)]
pub struct DraggedQueue {
    pub id: QueueId,
    name: String,
}

impl DraggedQueue {
    pub fn new(id: QueueId, name: String) -> Self {
        Self { id, name }
    }
}

impl Render for DraggedRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        pill(self.label.clone(), cx)
    }
}

impl Render for DraggedQueue {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        pill(self.name.clone(), cx)
    }
}

/// What follows the pointer: the name of what is dragged.
fn pill<V>(label: String, cx: &Context<'_, V>) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .px_2()
        .py_1()
        .rounded(theme.radius)
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .shadow_md()
        .text_role(Role::Body, cx)
        .child(label)
}

impl MainWindow {
    /// What dragging row `id` carries: the selection when it holds the row, else the row.
    pub(super) fn dragged_rows(&self, id: DownloadId, name: &str) -> DraggedRows {
        if self.selected.contains(&id) && self.selected.len() > 1 {
            return DraggedRows {
                ids: self.selected.iter().copied().collect(),
                label: format!("{} downloads", self.selected.len()),
            };
        }
        DraggedRows {
            ids: vec![id],
            label: name.to_owned(),
        }
    }

    /// Rows dropped on row `target` of a queue's line land just before it.
    pub(super) fn drop_rows_before(
        &mut self,
        target: DownloadId,
        dragged: &DraggedRows,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let View::Queue(queue) = self.view else {
            return;
        };
        let downloads = &self.library.read(cx).downloads;
        let line: Vec<DownloadId> = self
            .queue_line(cx)
            .into_iter()
            .map(|index| downloads[index].id)
            .collect();
        if !line.contains(&target) {
            return;
        }
        if let Some(order) = order::placed(&line, &dragged.ids, Some(target)) {
            order::apply(&self.library, queue, order, window, cx);
        }
    }

    /// Rows dropped on a queue in the navigation move there.
    pub(super) fn drop_rows_on_queue(
        queue: QueueId,
        dragged: &DraggedRows,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) {
        let ids = dragged.ids.clone();
        act(window.window_handle(), cx, move |manager| {
            api::move_to_queue(manager, &ids, queue)
        });
    }

    /// A queue dropped on queue `target` lands just before it; nothing goes before the default.
    pub(super) fn drop_queue_before(
        &mut self,
        target: QueueId,
        dragged: &DraggedQueue,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let queues: Vec<QueueId> = self.library.read(cx).queues.iter().map(|q| q.id).collect();
        let Some(default) = queues.first().copied() else {
            return;
        };
        if dragged.id == default {
            return;
        }
        let before = if target == default {
            queues.get(1).copied()
        } else {
            Some(target)
        };
        if let Some(order) = order::placed(&queues, &[dragged.id], before) {
            act(window.window_handle(), cx, move |manager| {
                api::set_queue_order(manager, &order)
            });
        }
    }

    /// Files dropped from outside: the links of Internet shortcuts open the Add dialog.
    pub(super) fn drop_paths(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let links: Vec<String> = paths
            .paths()
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("url"))
            })
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|text| shortcut_link(&text))
            .collect();
        if links.is_empty() {
            window.push_notification(
                "Only links can be dropped here. Drop an Internet shortcut, or copy the link and press Ctrl+V.",
                cx,
            );
            return;
        }
        self.new_download(&links, window, cx);
    }
}

/// The link of an Internet shortcut file: its `URL=` line.
fn shortcut_link(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("URL="))
        .map(str::trim)
        .filter(|link| !link.is_empty())
        .map(str::to_owned)
}

/// The look of a place that takes a drop while something is dragged over it.
pub(super) fn drop_target(
    style: gpui_kit::StyleRefinement,
    cx: &gpui_kit::App,
) -> gpui_kit::StyleRefinement {
    style.bg(cx.theme().list_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_internet_shortcut_gives_its_link() {
        let file = "[InternetShortcut]\r\nURL=https://example.com/file.zip\r\nIDList=\r\n";
        assert_eq!(
            shortcut_link(file),
            Some("https://example.com/file.zip".to_owned())
        );
        assert_eq!(shortcut_link("[InternetShortcut]\nURL=\n"), None);
    }
}
