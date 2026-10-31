//! Speed and queues: a limit on all downloads together, how many run at once across the queues,
//! and how many connections each opens, each naming the queues with a value of their own; then
//! every queue with when it runs, its own limits and its rules, and Test a link on the saved
//! rules.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, ElementId, Entity, IntoElement as _, ParentElement as _, Styled as _,
    WeakEntity, Window, div,
};
use wirecatch_engine::numeric::{approx_f64, f64_to_u64};
use wirecatch_engine::units::MIB;
use wirecatch_protocol::app::{
    AppSettingsPatch, MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD, Queue, QueueNotify,
    Update,
};
use wirecatch_protocol::rules::{AddDownloads, Rule};

use super::super::MainWindow;
use super::super::link_trial::LinkTrial;
use super::{change, current};
use crate::library::Library;
use crate::text;
use crate::ui::{Role, Typed as _};

/// What the total limit starts at when switched on.
const FIRST_TOTAL_LIMIT: u64 = 5 * MIB;

pub(super) fn page(
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    cx: &App,
) -> SettingPage {
    SettingPage::new(super::Page::Speed.title())
        .resettable(false)
        .groups([limits(library, cx), queues(library, main)])
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

fn queues(library: &Entity<Library>, main: &WeakEntity<MainWindow>) -> SettingGroup {
    let list = {
        let (library, main) = (library.clone(), main.clone());
        SettingField::render(move |_, _, cx| queue_list(&library, &main, cx))
    };
    let trial = {
        let library = library.clone();
        SettingField::render(move |_, window, cx| link_trial(&library, window, cx))
    };
    SettingGroup::new()
        .title("Queues")
        .item(
            SettingItem::new("Your queues", list)
                .layout(Axis::Vertical)
                .description("Edit a queue to change its limits, rules, schedule or notifications.")
                .keywords(["queue", "schedule", "rules", "limits", "new queue"]),
        )
        .item(
            SettingItem::new("Test a link", trial)
                .layout(Axis::Vertical)
                .description("Shows which queue a link would go to, and why.")
                .keywords(["try", "link", "url", "rules", "match"]),
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

/// Every queue, with New queue above them.
fn queue_list(
    library: &Entity<Library>,
    main: &WeakEntity<MainWindow>,
    cx: &mut App,
) -> AnyElement {
    let queues = library.read(cx).queues.clone();
    let theme = cx.theme();
    let (border, radius) = (theme.border, theme.radius);
    let adding = main.clone();
    let rows: Vec<AnyElement> = queues
        .iter()
        .enumerate()
        .map(|(index, queue)| queue_row(queue, index, main, cx))
        .collect();
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex().child(
                Button::new("settings-new-queue")
                    .small()
                    .primary()
                    .icon(IconName::Plus)
                    .label("New queue")
                    .on_click(move |_, window, cx| {
                        _ = adding.update(cx, |main, cx| main.edit_queue(None, window, cx));
                    }),
            ),
        )
        .child(
            v_flex()
                .rounded(radius)
                .border_1()
                .border_color(border)
                .children(rows),
        )
        .into_any_element()
}

fn queue_row(queue: &Queue, index: usize, main: &WeakEntity<MainWindow>, cx: &App) -> AnyElement {
    let border = cx.theme().border;
    let id = queue.id;
    let editing = main.clone();
    h_flex()
        .px_3()
        .py_2p5()
        .gap_3()
        .when(index > 0, |row| row.border_t_1().border_color(border))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_role(Role::Strong, cx)
                                .child(queue.settings.name.clone()),
                        )
                        .when(queue.is_default, |line| {
                            line.child(div().text_role(Role::Meta, cx).child("Default"))
                        }),
                )
                .when(!queue.settings.description.is_empty(), |text| {
                    text.child(
                        div()
                            .truncate()
                            .text_role(Role::Secondary, cx)
                            .child(queue.settings.description.clone()),
                    )
                })
                .child(
                    div()
                        .text_role(Role::Meta, cx)
                        .child(queue_facts(queue).join(" · ")),
                ),
        )
        .child(
            Button::new(ElementId::NamedInteger(
                "edit-queue".into(),
                id.0.unsigned_abs(),
            ))
            .small()
            .ghost()
            .icon(Icon::new(Lucide::Pencil))
            .tooltip(format!("Edit {}", queue.settings.name))
            .on_click(move |_, window, cx| {
                _ = editing.update(cx, |main, cx| main.edit_queue(Some(id), window, cx));
            }),
        )
        .into_any_element()
}

/// When a queue runs, how it notifies, its own limits and its rules, in a few words each.
fn queue_facts(queue: &Queue) -> Vec<String> {
    let settings = &queue.settings;
    let mut facts = vec![if queue.status.paused {
        "Paused".to_owned()
    } else {
        text::schedule(settings.schedule.as_ref())
    }];
    if settings.notify == QueueNotify::WhenDone {
        facts.push("Notifies you when the queue is done".to_owned());
    }
    let own = [
        settings
            .max_concurrent
            .own()
            .map(|n| format!("{n} at once")),
        settings
            .speed_limit
            .own()
            .map(|bytes| format!("Up to {}", text::speed(bytes))),
        settings.connections.own().map(|n| {
            format!(
                "{n} {} each",
                if n == 1 { "connection" } else { "connections" }
            )
        }),
    ];
    let follows = own.iter().any(Option::is_none);
    let own: Vec<String> = own.into_iter().flatten().collect();
    if own.is_empty() {
        facts.push("Uses global limits".to_owned());
    } else {
        facts.extend(own);
        if follows {
            facts.push("Other limits are global".to_owned());
        }
    }
    facts.extend(rules_fact(&settings.add_downloads));
    facts
}

/// An auto queue's rules in a few words: `Adds youtube.com and 2 more`.
fn rules_fact(add: &AddDownloads) -> Option<String> {
    let AddDownloads::Auto(rules) = add else {
        return None;
    };
    if !rules.enabled {
        return Some("Rules off".to_owned());
    }
    let all: Vec<Rule> = rules
        .sites
        .iter()
        .cloned()
        .map(Rule::Site)
        .chain(rules.file_names.iter().cloned().map(Rule::FileName))
        .chain(rules.file_types.iter().cloned().map(Rule::FileType))
        .chain(rules.categories.iter().copied().map(Rule::Category))
        .chain(rules.protocols.iter().copied().map(Rule::Protocol))
        .collect();
    let first = all.first()?;
    Some(match all.len() - 1 {
        0 => format!("Adds {}", text::rule_phrase(first)),
        more => format!("Adds {} and {more} more", text::rule_phrase(first)),
    })
}

/// Test a link against the saved rules, kept while the page shows.
fn link_trial(library: &Entity<Library>, window: &mut Window, cx: &mut App) -> AnyElement {
    let library = library.clone();
    window
        .use_keyed_state("settings-link-trial", cx, |window, cx| {
            LinkTrial::new(library, None, window, cx)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::{QueueSettings, Setting};
    use wirecatch_protocol::rules::QueueRules;

    #[test]
    fn an_auto_queue_names_its_first_rule_and_counts_the_rest() {
        let rules = |enabled| {
            AddDownloads::Auto(QueueRules {
                enabled,
                sites: vec!["youtube.com".into(), "vimeo.com".into()],
                file_types: vec!["iso".into()],
                ..QueueRules::default()
            })
        };
        assert_eq!(
            rules_fact(&rules(true)).as_deref(),
            Some("Adds youtube.com and 2 more")
        );
        assert_eq!(rules_fact(&rules(false)).as_deref(), Some("Rules off"));
        assert_eq!(rules_fact(&AddDownloads::Manual), None);
    }

    #[test]
    fn a_queue_with_some_limits_of_its_own_says_the_rest_are_global() {
        let mut settings = QueueSettings::new("Night");
        settings.max_concurrent = Setting::Own(2);
        let queue = Queue {
            id: wirecatch_protocol::QueueId(1),
            is_default: false,
            settings,
            status: wirecatch_protocol::app::QueueStatus::default(),
        };
        let facts = queue_facts(&queue);
        assert!(facts.contains(&"2 at once".to_owned()));
        assert!(facts.contains(&"Other limits are global".to_owned()));
    }
}
