//! The bar over the list: one field that searches the downloads and takes pasted links, which it
//! offers to download, and the button that opens an empty Add dialog.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Context, IntoElement, ParentElement as _, Styled as _, Window};

use super::MainWindow;
use crate::links;
use crate::ui::tokens;

impl MainWindow {
    pub(super) fn command_bar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let typed = self.typed_links(cx).len();
        h_flex()
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
            .child(
                Button::new("new-download")
                    .primary()
                    .icon(IconName::Plus)
                    .tooltip("New download")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_download(&[], window, cx)),
                    ),
            )
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
