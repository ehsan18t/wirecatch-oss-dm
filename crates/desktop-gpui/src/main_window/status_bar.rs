//! The status bar: what the whole app is doing, at a glance (D23).

use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use wirecatch_protocol::app::DownloadStatus;

use super::{Group, MainWindow};
use crate::library::Loaded;
use crate::text;

impl MainWindow {
    pub(super) fn status_bar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let said = match &library.loaded {
            Loaded::Loading => "Opening the download list".to_owned(),
            Loaded::Failed(reason) => reason.message.clone(),
            Loaded::Ready => {
                let running: Vec<_> = library
                    .downloads
                    .iter()
                    .filter(|row| Group::of(row.status) == Group::Downloading)
                    .collect();
                let waiting = library
                    .downloads
                    .iter()
                    .filter(|row| row.status == DownloadStatus::Queued)
                    .count();
                let speed: u64 = running.iter().map(|row| row.speed).sum();
                match (running.len(), waiting) {
                    (0, 0) => "Nothing downloading".to_owned(),
                    (0, waiting) => format!("Nothing downloading · {waiting} waiting"),
                    (running, waiting) => format!(
                        "{running} downloading · {} total{}",
                        text::speed(speed),
                        if waiting > 0 {
                            format!(" · {waiting} waiting")
                        } else {
                            String::new()
                        }
                    ),
                }
            }
        };
        let theme = cx.theme();
        h_flex()
            .h(px(26.))
            .px_3()
            .flex_shrink_0()
            .bg(theme.status_bar)
            .border_t_1()
            .border_color(theme.status_bar_border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().truncate().child(said))
    }
}
