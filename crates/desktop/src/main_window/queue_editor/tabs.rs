//! The queue page drawn: its header with the tabs, and each tab's fields.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, NumberInput};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::time_field::TimeField;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, Window, div, px, relative,
};
use wirecatch_protocol::app::{QueueNotify, Setting};

use super::{FOLDER_BY_TYPE, Own, QueueForm, Section, Stage, Unit, WEEK};
use crate::text;
use crate::ui::switch_row::panel;
use crate::ui::{Field, FolderRow, Role, SwitchRow, Typed as _, choice};

/// The widest the page's fields run, so a wide window does not stretch them.
const CONTENT_WIDTH: f32 = 720.;
/// The dot on a tab whose subject has something switched on.
const DOT: f32 = 6.;
const NUMBER_WIDTH: f32 = 132.;
/// The most of the header the way back takes, as a queue's name does in the queue's header.
const BACK_MAX_SHARE: f32 = 0.4;
const SCHEDULE_LABEL_WIDTH: f32 = 72.;

impl Render for QueueForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let content = match self.section {
            Section::General => self.general(cx),
            Section::Rules => self.rules.clone().into_any_element(),
            Section::Limits => panel(cx).children(self.limits(cx)).into_any_element(),
            Section::Schedule => panel(cx).child(self.schedule_row(cx)).into_any_element(),
            Section::Network => panel(cx).child(self.proxy.clone()).into_any_element(),
            Section::Notifications => panel(cx).child(self.notify_row(cx)).into_any_element(),
        };
        v_flex()
            .size_full()
            .min_h_0()
            .text_sm()
            .child(self.header(cx))
            .child(
                div()
                    .id("queue-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_5()
                    .py_5()
                    .child(div().max_w(px(CONTENT_WIDTH)).child(content)),
            )
    }
}

impl QueueForm {
    /// The way back, the page's title and Save on one line, as a queue's own header has them,
    /// with the tabs under it.
    fn header(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (danger, border) = (theme.danger, theme.border);
        let saving = self.stage == Stage::Saving;
        v_flex()
            .px_5()
            .pt_3()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .max_w(relative(BACK_MAX_SHARE))
                            .truncate()
                            .child(
                                Button::new("queue-page-back")
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(Lucide::ArrowLeft))
                                    .label(self.back.clone())
                                    .tooltip(self.back.clone())
                                    .on_click(
                                        cx.listener(|form, _, window, cx| form.go_back(window, cx)),
                                    ),
                            ),
                    )
                    .child(div().flex_shrink_0().text_role(Role::PageTitle, cx).child(
                        if self.original.is_some() {
                            "Edit queue"
                        } else {
                            "New queue"
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_role(Role::Secondary, cx)
                            .text_color(danger)
                            .children(self.problem.clone()),
                    )
                    .child(
                        Button::new("queue-page-save")
                            .small()
                            .primary()
                            .label(if self.original.is_some() {
                                "Save"
                            } else {
                                "Create queue"
                            })
                            .loading(saving)
                            .disabled(saving)
                            .on_click(cx.listener(|form, _, window, cx| form.save(window, cx))),
                    ),
            )
            .child(self.tab_bar(cx))
    }

    /// The tabs, a dot on each whose subject has something switched on.
    fn tab_bar(&self, cx: &mut Context<'_, Self>) -> TabBar {
        let tabs = Section::ALL.map(|section| {
            let tab = Tab::new().label(section.label());
            if self.is_switched_on(section, cx) {
                tab.suffix(
                    div()
                        .ml_1p5()
                        .size(px(DOT))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
            } else {
                tab
            }
        });
        TabBar::new("queue-tabs")
            .underline()
            .selected_index(self.section.index())
            .on_click(cx.listener(|form, index: &usize, _, cx| {
                if let Some(section) = Section::ALL.get(*index) {
                    form.section = *section;
                    cx.notify();
                }
            }))
            .children(tabs)
    }

    /// Whether a tab's subject has something switched on, which its dot shows.
    fn is_switched_on(&self, section: Section, cx: &App) -> bool {
        match section {
            Section::Rules => self.rules.read(cx).is_on(),
            Section::Limits => [Own::Concurrent, Own::Speed, Own::Connections]
                .iter()
                .any(|own| self.is_own(*own)),
            Section::Schedule => self.is_own(Own::Schedule),
            Section::Network => matches!(self.proxy.read(cx).proxy(), Setting::Own(_)),
            Section::General | Section::Notifications => false,
        }
    }

    fn general(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let folder = FolderRow::new(
            self.folder.as_deref().map(text::folder),
            FOLDER_BY_TYPE,
            cx.listener(|_, _, _, cx| Self::choose_folder(cx)),
            cx.listener(|form, _, _, cx| {
                form.folder = None;
                cx.notify();
            }),
        );
        v_flex()
            .gap_5()
            .child(Field::new("Name", Input::new(&self.name)))
            .child(
                Field::new("Description", Input::new(&self.description))
                    .note("Optional. Shown under the queue's name."),
            )
            .child(
                Field::new("Save to", folder).note(if self.folder.is_some() {
                    "Every download of this queue is saved here, whatever its file type."
                } else {
                    "Pick a folder to keep this queue's downloads together."
                }),
            )
            .children(self.delete_row(cx))
            .into_any_element()
    }

    /// Delete queue, for a queue made by the person: the default one stays.
    fn delete_row(&self, cx: &mut Context<'_, Self>) -> Option<Div> {
        self.original.as_ref().filter(|queue| !queue.is_default)?;
        let danger = cx.theme().danger;
        Some(
            panel(cx).child(
                h_flex()
                    .px_4()
                    .py_3()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_role(Role::Strong, cx).child("Delete queue"))
                            .child(div().text_role(Role::Meta, cx).child(
                                "Its downloads move to the default queue, or leave the list with it.",
                            )),
                    )
                    .child(
                        Button::new("ask-delete")
                            .small()
                            .outline()
                            .text_color(danger)
                            .label("Delete queue")
                            .on_click(cx.listener(|form, _, window, cx| form.delete(window, cx))),
                    ),
            ),
        )
    }

    fn limits(&self, cx: &mut Context<'_, Self>) -> Vec<AnyElement> {
        let app = self.library.read(cx).settings.clone();
        let global = |value: Option<String>| value.map(|value| format!("Global: {value}"));
        let unit = choice::one_of(
            "limit-unit",
            &["KB/s", "MB/s"],
            usize::from(self.unit == Unit::Mb),
            cx.listener(|form, picked: &usize, window, cx| {
                let unit = if *picked == 1 { Unit::Mb } else { Unit::Kb };
                form.set_unit(unit, window, cx);
            }),
            cx,
        );
        vec![
            SwitchRow::new(
                "concurrent-on",
                "Downloads at the same time",
                self.is_own(Own::Concurrent),
            )
            .help("Up to this many of its downloads run at once; the global limit still applies.")
            .off(global(
                app.as_ref().map(|s| s.max_concurrent_downloads.to_string()),
            ))
            .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                form.switch(Own::Concurrent, *on, cx);
            }))
            .fields(narrow(NumberInput::new(&self.concurrent)))
            .into_any_element(),
            SwitchRow::new("limit-on", "Speed limit", self.is_own(Own::Speed))
                .help("Shared by all its downloads; the global limit still applies.")
                .off(global(app.as_ref().map(|s| {
                    s.global_speed_limit
                        .map_or_else(|| "no limit".to_owned(), text::speed)
                })))
                .ruled(true)
                .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                    form.switch(Own::Speed, *on, cx);
                }))
                .fields(
                    h_flex()
                        .gap_2()
                        .child(narrow(NumberInput::new(&self.limit)))
                        .child(unit),
                )
                .into_any_element(),
            SwitchRow::new(
                "connections-on",
                "Connections per download",
                self.is_own(Own::Connections),
            )
            .help("Each of its downloads uses up to this many connections.")
            .off(global(
                app.as_ref().map(|s| s.connections_per_download.to_string()),
            ))
            .ruled(true)
            .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                form.switch(Own::Connections, *on, cx);
            }))
            .fields(narrow(NumberInput::new(&self.connections)))
            .into_any_element(),
        ]
    }

    fn schedule_row(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let stops = choice::one_of(
            "stops",
            &["When the queue is empty", "At"],
            usize::from(!self.until_empty),
            cx.listener(|form, picked: &usize, _, cx| {
                form.until_empty = *picked == 0;
                cx.notify();
            }),
            cx,
        );
        let pressed: Vec<(&'static str, bool)> = WEEK
            .iter()
            .map(|(day, label)| (*label, self.days.contains(day)))
            .collect();
        let days = choice::any_of(
            "days",
            &pressed,
            cx.listener(|form, picked: &[usize], _, cx| {
                form.days = picked
                    .iter()
                    .filter_map(|i| WEEK.get(*i))
                    .map(|(d, _)| *d)
                    .collect();
                form.problem = None;
                cx.notify();
            }),
            cx,
        );
        let line = |label: &'static str, value: AnyElement| {
            h_flex()
                .gap_3()
                .child(
                    div()
                        .w(px(SCHEDULE_LABEL_WIDTH))
                        .flex_shrink_0()
                        .text_role(Role::Secondary, cx)
                        .child(label),
                )
                .child(value)
        };
        SwitchRow::new("schedule-on", "Run on a schedule", self.is_own(Own::Schedule))
            .help("Its downloads start only in these hours, on the days picked. Start now runs it outside them.")
            .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                form.switch(Own::Schedule, *on, cx);
            }))
            .fields(
                v_flex()
                    .gap_2p5()
                    .child(line("Starts at", TimeField::new(&self.start).into_any_element()))
                    .child(line(
                        "Stops",
                        h_flex()
                            .gap_2()
                            .child(stops)
                            .when(!self.until_empty, |row| row.child(TimeField::new(&self.stop)))
                            .into_any_element(),
                    ))
                    .child(line("Days", days.into_any_element())),
            )
            .into_any_element()
    }

    fn notify_row(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        h_flex()
            .px_4()
            .py_3()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_role(Role::Strong, cx).child("Tell me when"))
                    .child(div().text_role(Role::Meta, cx).child(
                        "A notice for each finished download, or one when the whole queue is done.",
                    )),
            )
            .child(choice::one_of(
                "notify",
                &["Each finishes", "All are done"],
                usize::from(self.notify == QueueNotify::WhenDone),
                cx.listener(|form, picked: &usize, _, cx| {
                    form.notify = if *picked == 1 {
                        QueueNotify::WhenDone
                    } else {
                        QueueNotify::EachDownload
                    };
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }
}

fn narrow(field: impl IntoElement) -> Div {
    div().w(px(NUMBER_WIDTH)).child(field)
}
