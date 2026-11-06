//! Speed: a limit on all downloads together, how many run at once across the queues, and how
//! many connections each opens, each naming the queues with a value of their own; then the
//! queues, which are managed in the sidebar (D256).

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
};
use gpui_kit::{App, Entity, IntoElement as _, WeakEntity};
use wirecatch_engine::numeric::{approx_f64, f64_to_u64};
use wirecatch_engine::units::MIB;
use wirecatch_protocol::app::{
    AppSettingsPatch, MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD, Queue, Update,
};

use super::super::{MainWindow, Screen};
use super::{change, current};
use crate::closing::may_have_closed;
use crate::library::Library;

/// What the total limit starts at when switched on.
const FIRST_TOTAL_LIMIT: u64 = 5 * MIB;

pub(super) fn page(
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    cx: &App,
) -> SettingPage {
    SettingPage::new(super::Page::Speed.title())
        .resettable(false)
        .groups([limits(library, cx), queues(library, main, cx)])
}

/// The queues by name, and where they are managed: the sidebar.
fn queues(library: &Entity<Library>, main: &WeakEntity<MainWindow>, cx: &App) -> SettingGroup {
    let main = main.clone();
    let show = SettingField::render(move |_, _, _| {
        let main = main.clone();
        Button::new("show-queues")
            .small()
            .outline()
            .label("Show queues")
            .on_click(move |_, window, cx| {
                may_have_closed(main.update(cx, |main, cx| {
                    main.open_screen(Screen::Downloads, window, cx);
                }));
            })
            .into_any_element()
    });
    SettingGroup::new().item(
        SettingItem::new("Queues", show)
            .description(queues_line(&library.read(cx).queues))
            .keywords(["queue", "new queue", "schedule", "rules", "order"]),
    )
}

/// "Main, Videos. Create, edit and order queues in the sidebar."
fn queues_line(queues: &[Queue]) -> String {
    let names: Vec<&str> = queues
        .iter()
        .map(|queue| queue.settings.name.as_str())
        .collect();
    let listed = names.join(", ");
    let stop = if listed.ends_with(['.', '!', '?']) {
        ""
    } else {
        "."
    };
    format!("{listed}{stop} Create, edit and order queues in the sidebar.")
}

fn limits(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let own = |read: fn(&Queue) -> bool| own_note(&library.read(cx).queues, read);
    let limited = current(library, cx, |s| s.global_speed_limit.is_some(), false);
    let group = SettingGroup::new().title("Limits").item(
        SettingItem::new("Global speed limit", total_limit_switch(library))
            .description(with_note(
                "Keeps browsing and calls smooth while downloads run.",
                own(|q| q.settings.speed_limit.own().is_some()),
            ))
            .keywords(["bandwidth", "throttle", "cap", "limit", "rate"]),
    );
    let group = if limited {
        group.item(
            SettingItem::new("Limit, in MB/s", total_limit(library))
                .description("Shared by every download across the queues.")
                .keywords(["bandwidth", "throttle"]),
        )
    } else {
        group
    };
    group
        .item(
            SettingItem::new(
                "Downloads at the same time",
                whole_number(
                    library,
                    MAX_CONCURRENT_DOWNLOADS,
                    |s| s.max_concurrent_downloads,
                    |value| AppSettingsPatch {
                        max_concurrent_downloads: Some(value),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description(with_note(
                "Across all queues. Each queue can set a lower limit.",
                own(|q| q.settings.max_concurrent.own().is_some()),
            ))
            .keywords(["concurrent", "simultaneous", "parallel"]),
        )
        .item(
            SettingItem::new(
                "Connections per download",
                whole_number(
                    library,
                    u32::from(MAX_CONNECTIONS_PER_DOWNLOAD),
                    |s| u32::from(s.connections_per_download),
                    |value| AppSettingsPatch {
                        connections_per_download: u8::try_from(value).ok(),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description(with_note(
                "More connections can be faster on servers that allow it.",
                own(|q| q.settings.connections.own().is_some()),
            ))
            .keywords(["threads", "segments", "parts", "parallel"]),
        )
}

fn total_limit_switch(library: &Entity<Library>) -> SettingField<bool> {
    let reading = library.clone();
    SettingField::switch(
        move |cx| current(&reading, cx, |s| s.global_speed_limit.is_some(), false),
        |on, cx| {
            change(
                AppSettingsPatch {
                    global_speed_limit: Update::Set(on.then_some(FIRST_TOTAL_LIMIT)),
                    ..AppSettingsPatch::default()
                },
                cx,
            );
        },
    )
}

fn total_limit(library: &Entity<Library>) -> SettingField<f64> {
    let reading = library.clone();
    SettingField::number_input(
        NumberFieldOptions {
            min: 0.1,
            max: 100_000.,
            step: 0.5,
        },
        move |cx| {
            current(&reading, cx, |s| s.global_speed_limit, None).map_or(0., |bytes| {
                (approx_f64(bytes) / approx_f64(MIB) * 10.).round() / 10.
            })
        },
        |typed, cx| {
            let bytes = f64_to_u64((typed * approx_f64(MIB)).round()).filter(|bytes| *bytes > 0);
            if let Some(bytes) = bytes {
                change(
                    AppSettingsPatch {
                        global_speed_limit: Update::Set(Some(bytes)),
                        ..AppSettingsPatch::default()
                    },
                    cx,
                );
            }
        },
    )
}

/// A whole number from 1 to `most`.
fn whole_number(
    library: &Entity<Library>,
    most: u32,
    read: fn(&wirecatch_protocol::app::AppSettings) -> u32,
    patch: fn(u32) -> AppSettingsPatch,
) -> SettingField<f64> {
    let reading = library.clone();
    SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: f64::from(most),
            step: 1.,
        },
        move |cx| f64::from(current(&reading, cx, read, 1)),
        move |typed, cx| {
            let whole = f64_to_u64(typed.round()).and_then(|n| u32::try_from(n).ok());
            if let Some(whole) = whole.filter(|n| (1..=most).contains(n)) {
                change(patch(whole), cx);
            }
        },
    )
}

/// The queues with a value of their own: `1 queue uses its own value: Videos.`
fn own_note(queues: &[Queue], own: fn(&Queue) -> bool) -> Option<String> {
    let names: Vec<&str> = queues
        .iter()
        .filter(|queue| own(queue))
        .map(|queue| queue.settings.name.as_str())
        .collect();
    match names.len() {
        0 => None,
        1 => Some(format!("1 queue uses its own value: {}.", names[0])),
        n => Some(format!(
            "{n} queues use their own value: {}.",
            names.join(", ")
        )),
    }
}

fn with_note(help: &str, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{help} {note}"),
        None => help.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{QueueSettings, QueueStatus};

    use super::*;

    #[test]
    fn the_queues_row_names_every_queue_in_sidebar_order() {
        let queue = |id, name| Queue {
            id: QueueId(id),
            is_default: id == 1,
            settings: QueueSettings::new(name),
            status: QueueStatus::default(),
        };
        assert_eq!(
            queues_line(&[queue(1, "Main"), queue(2, "Videos")]),
            "Main, Videos. Create, edit and order queues in the sidebar."
        );
        assert_eq!(
            queues_line(&[queue(1, "Main"), queue(2, "Done by 6 a.m.")]),
            "Main, Done by 6 a.m. Create, edit and order queues in the sidebar."
        );
    }
}
