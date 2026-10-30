//! The navigation: every download, each status group, and each queue with its count, or its
//! speed while it downloads (D91).

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::{Group, MainWindow, View};
use crate::text;

pub(super) const WIDTH: f32 = 232.;

impl MainWindow {
    pub(super) fn sidebar(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let count = |view: View| {
            library
                .downloads
                .iter()
                .filter(|row| view.shows(row))
                .count()
        };
        let all = count(View::All);
        let counts = Group::ALL.map(|group| (group, count(View::Group(group))));
        let queues: Vec<_> = library
            .queues
            .iter()
            .map(|queue| {
                let rows = library.downloads.iter().filter(|row| row.queue == queue.id);
                let speed: u64 = rows.clone().map(|row| row.speed).sum();
                let figure = if speed > 0 {
                    text::speed(speed)
                } else {
                    rows.count().to_string()
                };
                (queue.id, queue.settings.name.clone(), figure)
            })
            .collect();
        let mut downloads =
            vec![self.nav_item("view-all", "All downloads", all.to_string(), View::All, cx)];
        for (group, shown) in counts {
            downloads.push(self.nav_item(
                ElementId::Name(SharedString::new(format!("view-{group:?}"))),
                group.label(),
                if shown == 0 {
                    String::new()
                } else {
                    shown.to_string()
                },
                View::Group(group),
                cx,
            ));
        }
        let queues: Vec<AnyElement> = queues
            .into_iter()
            .map(|(id, name, figure)| {
                self.nav_item(
                    ElementId::NamedInteger("queue".into(), id.0.unsigned_abs()),
                    name,
                    figure,
                    View::Queue(id),
                    cx,
                )
            })
            .collect();

        let theme = cx.theme();
        v_flex()
            .id("sidebar")
            .w(px(WIDTH))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .p_2()
            .gap_4()
            .child(v_flex().gap_px().children(downloads))
            .child(
                v_flex()
                    .gap_px()
                    .child(section_heading("Queues", cx))
                    .children(queues),
            )
    }

    fn nav_item(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        figure: impl Into<SharedString>,
        view: View,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let current = self.view == view;
        h_flex()
            .id(id)
            .h(px(28.))
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(current, |item| {
                item.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!current, |item| {
                item.text_color(theme.muted_foreground)
                    .hover(|item| item.bg(theme.sidebar_accent.opacity(0.6)))
            })
            .child(div().flex_1().truncate().child(label.into()))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(figure.into()),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)))
            .into_any_element()
    }
}

fn section_heading(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .px_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(label)
}
