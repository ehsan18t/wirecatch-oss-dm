//! The title bar: the app's mark and name, the total speed, and the window's buttons (D23, D36).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use wirecatch_protocol::app::{DownloadStatus, ProcessingStep};

use super::{Group, MainWindow};
use crate::text;
use crate::ui::mark::mark;
use crate::ui::{Role, Typed as _, tokens};

/// The mark beside the app's name, as tall as its capitals and a little more.
const MARK_HEIGHT: f32 = 13.;

/// The readout keeps this width whatever it says, so the bar never shifts.
const READOUT_WIDTH: f32 = 140.;

impl MainWindow {
    pub(super) fn title_bar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        TitleBar::new()
            .on_close_window(|_, window, cx| crate::closing::close(window, cx))
            .child(
                h_flex()
                    .w_full()
                    .pr_2()
                    .gap_3()
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_2()
                            .child(
                                // A control in the bar takes its own clicks: the bar under it only drags.
                                div().occlude().child(
                                    Button::new("toggle-nav")
                                        .small()
                                        .ghost()
                                        .icon(IconName::PanelLeft)
                                        .tooltip("Show or hide the navigation")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.toggle_nav(window, cx);
                                        })),
                                ),
                            )
                            .child(mark(px(MARK_HEIGHT), theme.primary))
                            .child(
                                div()
                                    .text_role(Role::Strong, cx)
                                    .child(wirecatch_shell::host::CHANNEL.name()),
                            ),
                    )
                    .child(h_flex().flex_1().justify_end().child(self.readout(cx))),
            )
    }

    /// How fast everything moves right now; in its tooltip, what is going on. Hidden while
    /// nothing runs.
    fn readout(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let downloads = &self.library.read(cx).downloads;
        let active: Vec<_> = downloads
            .iter()
            .filter(|row| Group::of(row.status) == Group::Downloading)
            .collect();
        let waiting = downloads
            .iter()
            .filter(|row| row.status == DownloadStatus::Queued)
            .count();
        let joining = active
            .iter()
            .filter(|row| {
                row.status == DownloadStatus::Processing
                    || row
                        .processing
                        .as_ref()
                        .is_some_and(|p| p.step == ProcessingStep::Joining)
            })
            .count();
        let speed: u64 = active.iter().map(|row| row.speed).sum();
        let said = if speed > 0 {
            text::speed(speed)
        } else if active.is_empty() {
            String::new()
        } else if joining == active.len() {
            "Finishing videos".to_owned()
        } else {
            "Starting".to_owned()
        };
        let downloading = active.len() - joining;
        let summary: Vec<String> = [
            (downloading > 0 || joining == 0).then(|| format!("{downloading} downloading")),
            (joining > 0).then(|| format!("{joining} being merged or converted")),
            (waiting > 0).then(|| format!("{waiting} waiting")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let summary = if active.is_empty() && waiting == 0 {
            "Nothing running".to_owned()
        } else {
            summary.join(", ")
        };
        let theme = cx.theme();
        h_flex()
            .id("speed-readout")
            .w(px(READOUT_WIDTH))
            .justify_end()
            .gap_1p5()
            .text_role(Role::Meta, cx)
            .when(said.is_empty(), gpui_kit::Styled::invisible)
            .text_color(if speed > 0 {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .child(
                Icon::new(IconName::ArrowDown)
                    .with_size(tokens::ICON_SMALL)
                    .when(speed > 0, |icon| icon.text_color(theme.primary)),
            )
            .child(said)
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(format!(
                    "Total download speed. {summary}."
                ))
                .build(window, cx)
            })
    }
}
