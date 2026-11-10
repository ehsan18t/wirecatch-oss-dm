//! Copy and folder actions for a held selection.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{IconName, Sizable as _};
use gpui_kit::{
    App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Window, div,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadSummary;

use super::MainWindow;
use crate::files;
use crate::main_window::{applies, details};
use crate::rows::Rows;

/// Folder availability changes separately from the list's grouping and the selected IDs.
#[derive(Default)]
pub(super) struct FolderCache {
    revision: Option<u64>,
    found: Option<Rc<SelectionFolder>>,
}

impl FolderCache {
    fn get(&mut self, downloads: &Rows, selected: &[DownloadId]) -> Option<Rc<SelectionFolder>> {
        let revision = downloads.folder_revision();
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            self.found = SelectionFolder::of(selected.iter().filter_map(|id| downloads.find(*id)))
                .map(Rc::new);
        }
        self.found.clone()
    }
}

impl MainWindow {
    pub(super) fn selected_folder(&self, cx: &App) -> Option<Rc<SelectionFolder>> {
        let facts = self.selection_facts(cx);
        facts
            .folder
            .borrow_mut()
            .get(&self.library.read(cx).downloads, &facts.ids)
    }
}

/// The selected downloads with files on disk, when they all share a folder.
pub(super) struct SelectionFolder {
    first: DownloadId,
    path: PathBuf,
    count: usize,
}

impl SelectionFolder {
    fn of<'a>(rows: impl Iterator<Item = &'a DownloadSummary>) -> Option<Self> {
        let mut with_files = rows.filter(|row| applies::has_file(row));
        let first = with_files.next()?;
        let folder = Path::new(&first.path).parent()?;
        let count = with_files.try_fold(1, |count, row| {
            (Path::new(&row.path).parent() == Some(folder)).then_some(count + 1)
        })?;
        Some(Self {
            first: first.id,
            path: folder.to_path_buf(),
            count,
        })
    }
}

pub(super) fn folder_button(
    folder: &SelectionFolder,
    selected: usize,
    cx: &mut Context<'_, MainWindow>,
) -> impl IntoElement {
    let label = if folder.count == selected {
        "Show in folder".to_owned()
    } else {
        format!("Show {} in folder", folder.count)
    };
    bar_icon(
        "sel-show-folder",
        label,
        IconName::FolderOpen,
        cx.listener(|this, _, window, cx| {
            let Some(folder) = this.selected_folder(cx) else {
                return;
            };
            if folder.count == 1 {
                files::show_in_folder(folder.first, window, cx);
            } else {
                files::show_containing_folder(folder.first, folder.path.clone(), window, cx);
            }
        }),
    )
}

pub(super) fn copy_button(count: usize, cx: &mut Context<'_, MainWindow>) -> impl IntoElement {
    bar_icon(
        "sel-copy-links",
        details::copy_link_label(count),
        IconName::Copy,
        cx.listener(|this, _, window, cx| {
            let links: Vec<_> = this
                .chosen(&this.laid(cx), cx)
                .into_iter()
                .map(|row| row.url.clone())
                .collect();
            details::copy_links(&links, window, cx);
        }),
    )
}

fn bar_icon(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: IconName,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div().debug_selector(move || id.to_owned()).child(
        Button::new(id)
            .small()
            .ghost()
            .icon(icon)
            .accessibility_label(label.clone())
            .tooltip(label)
            .on_click(click),
    )
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AppContext as _, Modifiers, TestAppContext, VisualTestContext, WindowOptions, px, size,
    };
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{DownloadStatus, Queue, QueueSettings, QueueStatus};
    use wirecatch_test_support::downloads::download;

    use super::*;

    fn selection_window(
        downloads: Vec<DownloadSummary>,
        cx: &mut TestAppContext,
    ) -> (gpui_kit::Entity<MainWindow>, &mut VisualTestContext) {
        let (window, main) = cx
            .update(|cx| {
                gpui_kit::component::init(cx);
                let library = cx.new(|_| {
                    let mut library = crate::library::Library::empty();
                    library.downloads.replace(downloads);
                    library.queues = ["Main", "Later"]
                        .into_iter()
                        .zip([QueueId(1), QueueId(2)])
                        .map(|(name, id)| Queue {
                            id,
                            is_default: id == QueueId(1),
                            settings: QueueSettings::new(name),
                            status: QueueStatus::default(),
                        })
                        .collect();
                    library
                });
                crate::app_update::start(&library, cx);
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| MainWindow::new(library, window, cx))
                })
            })
            .unwrap();
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        cx.simulate_resize(size(px(720.), px(600.)));
        (main, cx)
    }

    #[gpui_kit::test]
    fn the_bar_copies_only_ticked_links_in_list_order(cx: &mut TestAppContext) {
        let rows = [download(3), download(1), download(2)];
        let copied = format!("{}\n{}", rows[0].url, rows[2].url);
        let (main, cx) = selection_window(rows.into(), cx);
        main.update(cx, |main, cx| {
            main.toggle_rows(&[DownloadId(2), DownloadId(3)], cx);
        });
        cx.run_until_parked();
        let button = cx
            .debug_bounds("sel-copy-links")
            .expect("the selection bar offers Copy links");
        cx.simulate_click(button.center(), Modifiers::none());
        let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(clipboard.as_deref(), Some(copied.as_str()));
    }

    #[gpui_kit::test]
    fn the_bar_offers_one_folder_and_updates_when_a_file_moves(cx: &mut TestAppContext) {
        let first = DownloadSummary {
            status: DownloadStatus::Completed,
            path: "Downloads/file-1.zip".into(),
            ..download(1)
        };
        let second = DownloadSummary {
            status: DownloadStatus::Downloading,
            path: "Downloads/file-2.zip".into(),
            ..download(2)
        };
        let (main, cx) = selection_window(vec![first, second.clone()], cx);
        main.update(cx, |main, cx| main.toggle_rows(&[DownloadId(2)], cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("sel-show-folder").is_none());

        main.update(cx, |main, cx| {
            main.library.update(cx, |library, cx| {
                library.downloads.put(DownloadSummary {
                    done: 1,
                    ..second.clone()
                });
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("sel-show-folder").is_some());

        main.update(cx, |main, cx| main.toggle_rows(&[DownloadId(1)], cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("sel-show-folder").is_some());

        main.update(cx, |main, cx| {
            main.library.update(cx, |library, cx| {
                library.downloads.put(DownloadSummary {
                    done: 1,
                    path: "Elsewhere/file-2.zip".into(),
                    ..second
                });
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("sel-show-folder").is_none());
    }

    #[gpui_kit::test]
    fn the_full_selection_bar_fits_the_narrow_window(cx: &mut TestAppContext) {
        let rows = [
            DownloadStatus::Completed,
            DownloadStatus::Queued,
            DownloadStatus::Paused,
            DownloadStatus::Failed,
        ]
        .into_iter()
        .zip(1..)
        .map(|(status, id)| DownloadSummary {
            status,
            path: format!("Downloads/file-{id}.zip"),
            ..download(id)
        })
        .collect();
        let (main, cx) = selection_window(rows, cx);
        main.update(cx, MainWindow::select_all);
        cx.run_until_parked();
        for action in ["sel-show-folder", "sel-copy-links", "sel-clear"] {
            let button = cx
                .debug_bounds(action)
                .expect("the selection action is drawn");
            assert!(
                button.right() <= px(720.),
                "{action} runs out of the window"
            );
        }
    }
}
