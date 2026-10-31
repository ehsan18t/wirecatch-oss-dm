//! The menu of what can be done with downloads: a right-click on a row, its "…" button, or the
//! Menu key and Shift+F10 on the row under the cursor. It acts on the selection when the row is
//! in it, else on the row alone. Everything here is also on the row, in the selection bar or in
//! the details (D3).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, DismissEvent, Entity, Focusable as _, Pixels, WeakEntity,
    Window, point, px,
};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, FixAction};
use wirecatch_protocol::{DownloadId, QueueId};
use wirecatch_shell::api;

use super::applies::{self, Applicable};
use super::{MainWindow, confirm, details, fixes};
use crate::backend::act;
use crate::library::Library;
use crate::{files, text};

impl MainWindow {
    /// What the menu of row `id` acts on: the selection when it holds the row, else the row.
    fn menu_rows(&self, id: DownloadId, cx: &App) -> Vec<DownloadSummary> {
        if self.selected.contains(&id) {
            return self.selected_rows(cx);
        }
        self.library
            .read(cx)
            .downloads
            .iter()
            .filter(|row| row.id == id)
            .cloned()
            .collect()
    }

    /// A right-click on a row outside the selection picks that row first.
    pub(super) fn right_click_row(&mut self, id: DownloadId, cx: &mut Context<'_, Self>) {
        if !self.selected.contains(&id) {
            self.pick(id);
            cx.notify();
        }
    }

    /// The Menu key or Shift+F10: the menu of the row under the cursor, at that row.
    pub(super) fn open_keyboard_menu(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(id) = self.cursor.filter(|id| self.selected.contains(id)) else {
            return;
        };
        let target = Target::of(self, id, cx);
        let menu = PopupMenu::build(window, cx, move |menu, _, _| fill(menu, target));
        let dismissed = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
            this.keyboard_menu = None;
            this.focus.focus(window, cx);
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.keyboard_menu = Some((menu, dismissed));
        cx.notify();
    }

    /// Where the keyboard menu shows: under the start of the cursor's row.
    pub(super) fn keyboard_menu_at(&self) -> gpui_kit::Point<Pixels> {
        self.cursor_bounds
            .get()
            .map_or_else(gpui_kit::Point::default, |bounds: Bounds<Pixels>| {
                point(
                    bounds.origin.x + px(56.),
                    bounds.origin.y + bounds.size.height,
                )
            })
    }
}

/// What the menu acts on, read once when it opens.
struct Target {
    rows: Vec<DownloadSummary>,
    ids: Vec<DownloadId>,
    /// The one download, when the menu acts on one.
    only: Option<DownloadSummary>,
    library: Entity<Library>,
    /// The queues the downloads can move to, with more than one queue.
    moves: Vec<(QueueId, String)>,
}

impl Target {
    fn of(main: &MainWindow, id: DownloadId, cx: &App) -> Self {
        let library = main.library.clone();
        let rows = main.menu_rows(id, cx);
        let refs: Vec<&DownloadSummary> = rows.iter().collect();
        let queues = &library.read(cx).queues;
        let moves = if queues.len() > 1 {
            queues
                .iter()
                .filter(|queue| applies::can_move_to(&refs, queue.id))
                .map(|queue| (queue.id, queue.settings.name.clone()))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            ids: rows.iter().map(|row| row.id).collect(),
            only: match rows.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            },
            rows,
            library,
            moves,
        }
    }

    /// The one download's fix, when it failed with one.
    fn fix(&self) -> Option<(DownloadId, FixAction)> {
        let row = self.only.as_ref()?;
        let fix = row.error.as_ref()?.fix?;
        (row.status == DownloadStatus::Failed).then_some((row.id, fix))
    }
}

/// The menu for row `id` of the main window `main`.
pub(super) fn build(
    menu: PopupMenu,
    main: &WeakEntity<MainWindow>,
    id: DownloadId,
    cx: &mut App,
) -> PopupMenu {
    let Some(main) = main.upgrade() else {
        return menu;
    };
    let target = Target::of(main.read(cx), id, cx);
    fill(menu, target)
}

/// The menu's items for `target`.
fn fill(menu: PopupMenu, target: Target) -> PopupMenu {
    let menu = actions(menu, &target);
    let menu = about_one(menu, &target);
    let menu = moves(menu, &target);
    ending(menu, target)
}

/// The fix, Open, Pause, Resume, Try again and the details.
fn actions(menu: PopupMenu, target: &Target) -> PopupMenu {
    let refs: Vec<&DownloadSummary> = target.rows.iter().collect();
    let can = Applicable::to(&refs);
    let fix = target.fix();
    let rows = &target.rows;
    let finished = target
        .only
        .as_ref()
        .filter(|row| row.status == DownloadStatus::Completed)
        .map(|row| row.id);
    menu.when_some(fix, |menu, (id, fix)| {
        menu.item(
            PopupMenuItem::new(text::fix_label(fix))
                .icon(Icon::new(IconName::RotateCw))
                .on_click(move |_, window, cx| fixes::run(id, fix, window, cx)),
        )
    })
    .when_some(finished, |menu, id| {
        menu.item(
            PopupMenuItem::new("Open")
                .icon(IconName::ExternalLink)
                .on_click(move |_, window, cx| files::open(id, window, cx)),
        )
    })
    .when(can.pause, |menu| {
        menu.item(each_item(
            "Pause",
            IconName::Pause,
            rows,
            applies::can_pause,
            api::pause,
        ))
    })
    .when(can.resume, |menu| {
        menu.item(each_item(
            "Resume",
            IconName::Play,
            rows,
            applies::can_resume,
            api::resume,
        ))
    })
    .when(
        can.retry && fix.map(|(_, fix)| fix) != Some(FixAction::Retry),
        |menu| {
            menu.item(each_item(
                "Try again",
                IconName::RotateCw,
                rows,
                applies::can_retry,
                api::retry,
            ))
        },
    )
    .when_some(target.only.as_ref().map(|row| row.id), |menu, id| {
        let library = target.library.clone();
        menu.item(
            PopupMenuItem::new("Show details")
                .icon(IconName::Info)
                .on_click(move |_, window, cx| details::open(library.clone(), id, window, cx)),
        )
    })
    .separator()
}

/// Show in folder, Copy link and Save to a different folder, for one download.
fn about_one(menu: PopupMenu, target: &Target) -> PopupMenu {
    let Some(row) = target.only.clone() else {
        return menu;
    };
    let (id, link) = (row.id, row.url.clone());
    menu.item(
        PopupMenuItem::new("Show in folder")
            .icon(IconName::FolderOpen)
            .on_click(move |_, window, cx| files::show_in_folder(id, window, cx)),
    )
    .item(
        PopupMenuItem::new("Copy link")
            .icon(IconName::Copy)
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
            }),
    )
    .when(
        applies::offers_new_link(&row)
            && row.error.as_ref().and_then(|e| e.fix) != Some(FixAction::RefreshLink),
        |menu| {
            menu.item(
                PopupMenuItem::new(text::fix_label(FixAction::RefreshLink))
                    .icon(IconName::Globe)
                    .on_click(move |_, window, cx| {
                        fixes::run(id, FixAction::RefreshLink, window, cx);
                    }),
            )
        },
    )
    .when(row.status != DownloadStatus::Completed, |menu| {
        menu.item(
            PopupMenuItem::new("Save to a different folder")
                .icon(IconName::Folder)
                .on_click(move |_, window, cx| fixes::choose_folder(id, false, window, cx)),
        )
    })
    .separator()
}

/// Moving to each queue some of the downloads are not in.
fn moves(menu: PopupMenu, target: &Target) -> PopupMenu {
    target
        .moves
        .iter()
        .fold(menu, |menu, (queue, name)| {
            let (queue, ids) = (*queue, target.ids.clone());
            menu.item(
                PopupMenuItem::new(format!("Move to {name}"))
                    .icon(Icon::new(Lucide::ArrowRightLeft))
                    .on_click(move |_, window, cx| {
                        let ids = ids.clone();
                        act(window.window_handle(), cx, move |manager| {
                            api::move_to_queue(manager, &ids, queue)
                        });
                    }),
            )
        })
        .separator()
}

/// Cancel and Remove, which ask first.
fn ending(menu: PopupMenu, target: Target) -> PopupMenu {
    let Target {
        rows, ids, library, ..
    } = target;
    let cancelled = (library.clone(), ids.clone());
    menu.when(rows.iter().any(applies::can_cancel), |menu| {
        menu.item(
            PopupMenuItem::new("Cancel")
                .icon(IconName::CircleX)
                .on_click(move |_, window, cx| {
                    confirm::cancel(&cancelled.0, &cancelled.1, window, cx);
                }),
        )
    })
    .item(
        PopupMenuItem::new("Remove")
            .icon(Icon::new(Lucide::Trash))
            .on_click(move |_, window, cx| confirm::remove(&library, &ids, window, cx)),
    )
}

/// An item that runs `command` on each of `rows` that `takes` it.
fn each_item(
    label: &'static str,
    icon: IconName,
    rows: &[DownloadSummary],
    takes: fn(&DownloadSummary) -> bool,
    command: super::selection::Command,
) -> PopupMenuItem {
    let rows = rows.to_vec();
    PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_, window, cx| {
            let rows: Vec<&DownloadSummary> = rows.iter().collect();
            super::selection::each(&rows, takes, command, window, cx);
        })
}

/// The menu as a child of the window, while the keyboard opened it.
pub(super) fn keyboard_menu(
    menu: &gpui_kit::Entity<PopupMenu>,
    at: gpui_kit::Point<Pixels>,
) -> impl gpui_kit::IntoElement {
    use gpui_kit::ParentElement as _;
    gpui_kit::deferred(gpui_kit::anchored().position(at).child(menu.clone())).with_priority(1)
}
