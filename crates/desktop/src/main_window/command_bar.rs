//! The bar over the list: one field that searches the downloads and takes pasted links, which it
//! offers to download, and the button that opens an empty Add dialog.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, Window,
};

use wirecatch_protocol::app::{AppSettingsPatch, ListLines};
use wirecatch_shell::api;

use super::list::GroupCommand;
use super::{MainWindow, View, sort};
use crate::backend::act;
use crate::links;
use crate::ui::tokens;

impl MainWindow {
    pub(super) fn command_bar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let typed = self.typed_links(cx).len();
        let first_run = self.is_first_run(cx);
        h_flex()
            .key_context(super::SEARCH_CONTEXT)
            .px_5()
            .pt_4()
            .pb_2()
            .gap_2()
            .child(
                Input::new(&self.search)
                    .flex_1()
                    .cleanable(true)
                    .prefix(
                        Icon::new(if typed > 0 {
                            IconName::Globe
                        } else {
                            IconName::Search
                        })
                        .with_size(tokens::ICON),
                    )
                    .when(typed > 0, |field| {
                        field.suffix(
                            Button::new("download-typed")
                                .xsmall()
                                .primary()
                                .label(match typed {
                                    1 => "Download".to_owned(),
                                    n => format!("Download {n} links"),
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.download_typed(window, cx);
                                })),
                        )
                    }),
            )
            .when_some(self.page_command(cx), |bar, (label, command)| {
                bar.child(
                    Button::new("page-command")
                        .ghost()
                        .label(label)
                        .on_click(cx.listener(move |_, _, window, cx| command.run(window, cx))),
                )
            })
            .child(self.sort_menu(cx))
            .child(self.view_menu(cx))
            .when(!first_run, |bar| {
                bar.child(
                    Button::new("new-download")
                        .primary()
                        .icon(IconName::Plus)
                        .tooltip("New download")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.new_download(&[], window, cx)),
                        ),
                )
            })
    }

    /// The button a status's own page offers for all its downloads, which a heading offers on All
    /// downloads.
    fn page_command(&self, cx: &App) -> Option<(&'static str, GroupCommand)> {
        match self.view {
            View::Group(group) => self.group_command(group, cx),
            View::All | View::Queue(_) => None,
        }
    }

    /// How the list is sorted: choosing the sort it has turns its direction around.
    fn sort_menu(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let chosen = self.sort;
        let main = cx.entity().downgrade();
        Button::new("sort")
            .ghost()
            .icon(Icon::new(Lucide::ArrowUpDown))
            .tooltip("Sort")
            .dropdown_menu(move |menu, _, _| {
                sort::Key::ALL.iter().fold(menu, |menu, key| {
                    let (key, main) = (*key, main.clone());
                    let label = if key == chosen.key && chosen.descending {
                        format!("{} (descending)", key.label())
                    } else {
                        key.label().to_owned()
                    };
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(key == chosen.key)
                            .on_click(move |_, _, cx| {
                                _ = main.update(cx, |main, cx| {
                                    main.sort = sort::Sort {
                                        key,
                                        descending: key == main.sort.key && !main.sort.descending,
                                    };
                                    cx.notify();
                                });
                            }),
                    )
                })
            })
    }

    /// Which lines each row shows under its name; the rows grow or shrink with them.
    fn view_menu(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let shown = self.list_lines(cx);
        let toggle = |label: &'static str, on: bool, changed: ListLines| {
            PopupMenuItem::new(label)
                .checked(on)
                .on_click(move |_, window, cx| {
                    let patch = AppSettingsPatch {
                        list_lines: Some(changed),
                        ..AppSettingsPatch::default()
                    };
                    act(window.window_handle(), cx, move |manager| {
                        api::change_settings(manager, patch)
                    });
                })
        };
        Button::new("view")
            .ghost()
            .icon(Icon::new(Lucide::LayoutList))
            .tooltip("View")
            .dropdown_menu(move |menu, _, _| {
                menu.label("Show on each row")
                    .item(toggle(
                        "Link",
                        shown.link,
                        ListLines {
                            link: !shown.link,
                            ..shown
                        },
                    ))
                    .item(toggle(
                        "Save folder",
                        shown.folder,
                        ListLines {
                            folder: !shown.folder,
                            ..shown
                        },
                    ))
            })
    }

    /// Nothing was ever downloaded: the list is loaded and empty.
    pub(super) fn is_first_run(&self, cx: &App) -> bool {
        let library = self.library.read(cx);
        matches!(library.loaded, crate::library::Loaded::Ready) && library.downloads.is_empty()
    }

    /// The links typed or pasted into the field.
    pub(super) fn typed_links(&self, cx: &App) -> Vec<String> {
        links::extract(&self.search.read(cx).value())
    }

    /// Opens the Add dialog on the field's links and empties the field.
    pub(super) fn download_typed(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let urls = self.typed_links(cx);
        if urls.is_empty() {
            return;
        }
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.new_download(&urls, window, cx);
    }
}
