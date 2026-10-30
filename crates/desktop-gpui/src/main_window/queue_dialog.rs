//! Making a queue or changing one (D72): its name and description, its own folder (D94), its own
//! limits, its hours and when it tells of finished downloads. A limit follows the app's until it
//! is switched on (D72.1). Its rules have their own editor; its proxy is kept as it is.

use chrono::{NaiveTime, Timelike as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::time_field::{TimeField, TimeFieldState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD, PortablePath, Queue, QueueNotify,
    QueueSettings, Schedule, Setting, TimeOfDay, Weekday,
};
use wirecatch_shell::api;

use super::queue_rules::RulesForm;
use super::{MainWindow, View};
use crate::backend::Backend;
use crate::library::Library;
use crate::ui::switch_row::group;
use crate::ui::{Field, FolderRow, Role, SwitchRow, Typed as _, choice};
use crate::{files, text};

const WIDTH: f32 = 600.;
/// What a speed limit switched on starts at when the app has none.
const FIRST_SPEED_LIMIT: u64 = 2 * Unit::Mb.bytes();
/// A schedule switched on starts as one night.
const FIRST_START: TimeOfDay = TimeOfDay { hour: 1, minute: 0 };
const FIRST_STOP: TimeOfDay = TimeOfDay { hour: 7, minute: 0 };
const WEEK: [(Weekday, &str); 7] = [
    (Weekday::Mon, "Mon"),
    (Weekday::Tue, "Tue"),
    (Weekday::Wed, "Wed"),
    (Weekday::Thu, "Thu"),
    (Weekday::Fri, "Fri"),
    (Weekday::Sat, "Sat"),
    (Weekday::Sun, "Sun"),
];
const FOLDER_BY_TYPE: &str = "By file type, as Settings sorts them";

/// A setting of the queue's own, off until switched on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Own {
    Concurrent,
    Speed,
    Connections,
    Schedule,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Kb,
    Mb,
}

impl Unit {
    const fn bytes(self) -> u64 {
        match self {
            Self::Kb => 1024,
            Self::Mb => 1024 * 1024,
        }
    }

    fn of(bytes: u64) -> Self {
        if bytes >= Self::Mb.bytes() {
            Self::Mb
        } else {
            Self::Kb
        }
    }

    fn step(self) -> f64 {
        match self {
            Self::Kb => 50.,
            Self::Mb => 0.5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Editing,
    Saving,
    AskingToDelete,
}

/// Opens the queue editor on `queue`, or on a new queue when `None`.
pub(super) fn open(
    library: Entity<Library>,
    queue: Option<QueueId>,
    main: WeakEntity<MainWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| QueueForm::new(library, queue, main, window, cx));
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let saving = form.clone();
        let title = match &form.read(cx).original {
            Some(queue) => format!("Edit {}", queue.settings.name),
            None => "New queue".to_owned(),
        };
        dialog
            .title(title)
            .w(px(WIDTH))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                saving.update(cx, |form, cx| form.save(window, cx));
                false
            })
            .footer(footer(&form, cx))
    });
}

/// Save and Cancel, with Delete queue on the left; once Delete queue is pressed, the question.
fn footer(form: &Entity<QueueForm>, cx: &App) -> Div {
    let state = form.read(cx);
    let row = h_flex().w_full().gap_2().items_center();
    let (save, delete, keep, confirm) = (form.clone(), form.clone(), form.clone(), form.clone());
    if state.stage == Stage::AskingToDelete
        && let Some(queue) = &state.original
    {
        return row
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_role(Role::Secondary, cx)
                    .child(format!(
                        "Delete {}? {}",
                        queue.settings.name,
                        state.moving_note(cx)
                    )),
            )
            .child(
                Button::new("keep-queue")
                    .label("Keep queue")
                    .on_click(move |_, _, cx| {
                        keep.update(cx, |form, cx| form.ask_delete(false, cx));
                    }),
            )
            .child(
                Button::new("delete-queue")
                    .danger()
                    .label("Delete queue")
                    .on_click(move |_, window, cx| {
                        confirm.update(cx, |form, cx| form.delete(window, cx));
                    }),
            );
    }
    let deletable = state
        .original
        .as_ref()
        .is_some_and(|queue| !queue.is_default);
    // Shown here, not above the form, so it is in sight wherever the form is scrolled.
    let left = match &state.problem {
        Some(problem) => div()
            .text_role(Role::Secondary, cx)
            .text_color(cx.theme().danger)
            .child(problem.clone())
            .into_any_element(),
        None if deletable => Button::new("ask-delete")
            .ghost()
            .text_color(cx.theme().danger)
            .label("Delete queue")
            .on_click(move |_, _, cx| delete.update(cx, |form, cx| form.ask_delete(true, cx)))
            .into_any_element(),
        None => div().into_any_element(),
    };
    row.child(h_flex().flex_1().min_w_0().child(left))
        .child(
            Button::new("cancel")
                .label("Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new("save")
                .primary()
                .label(if state.original.is_some() {
                    "Save"
                } else {
                    "Create queue"
                })
                .loading(state.stage == Stage::Saving)
                .disabled(state.stage == Stage::Saving)
                .on_click(move |_, window, cx| save.update(cx, |form, cx| form.save(window, cx))),
        )
}

pub struct QueueForm {
    library: Entity<Library>,
    main: WeakEntity<MainWindow>,
    /// The queue as it was when the editor opened; `None` for a new one.
    original: Option<Queue>,
    name: Entity<InputState>,
    description: Entity<InputState>,
    folder: Option<String>,
    /// The settings switched on to be the queue's own.
    own: Vec<Own>,
    concurrent: Entity<InputState>,
    limit: Entity<InputState>,
    unit: Unit,
    connections: Entity<InputState>,
    start: Entity<TimeFieldState>,
    until_empty: bool,
    stop: Entity<TimeFieldState>,
    /// The days pressed: all seven for every day.
    days: Vec<Weekday>,
    notify: QueueNotify,
    /// Why the form cannot be saved, or why saving failed.
    problem: Option<String>,
    stage: Stage,
    rules: Entity<RulesForm>,
    _subscriptions: Vec<Subscription>,
}

impl QueueForm {
    fn new(
        library: Entity<Library>,
        queue: Option<QueueId>,
        main: WeakEntity<MainWindow>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let original = queue.and_then(|id| library.read(cx).queue(id).cloned());
        let opened = original.as_ref().map(|queue| queue.settings.clone());
        let opened = opened.unwrap_or_else(|| QueueSettings::new(""));
        let app = library.read(cx).settings.clone();

        let name = text_field(&opened.name, "For example: Overnight", window, cx);
        let description = text_field(
            &opened.description,
            "For example: Large files, overnight",
            window,
            cx,
        );
        let concurrent = number_field(
            opened.max_concurrent.own().unwrap_or_else(|| {
                app.as_ref()
                    .map_or(1, |s| s.max_concurrent_downloads.saturating_sub(1).max(1))
            }),
            MAX_CONCURRENT_DOWNLOADS,
            window,
            cx,
        );
        let connections = number_field(
            u32::from(
                opened
                    .connections
                    .own()
                    .unwrap_or_else(|| app.as_ref().map_or(1, |s| s.connections_per_download)),
            ),
            u32::from(MAX_CONNECTIONS_PER_DOWNLOAD),
            window,
            cx,
        );
        let limit_bytes = opened
            .speed_limit
            .own()
            .or(app.as_ref().and_then(|s| s.global_speed_limit))
            .unwrap_or(FIRST_SPEED_LIMIT);
        let unit = Unit::of(limit_bytes);
        let limit = cx.new(|cx| {
            let mut state = InputState::new(window, cx).min(0.).step(unit.step());
            state.set_value(in_unit(limit_bytes, unit), window, cx);
            state
        });
        let schedule = opened.schedule.as_ref();
        let start = time_field(schedule.map_or(FIRST_START, |s| s.start), window, cx);
        let stop = time_field(
            schedule.and_then(|s| s.stop).unwrap_or(FIRST_STOP),
            window,
            cx,
        );
        let edited =
            forget_problem_on_edit([&name, &description, &concurrent, &limit, &connections], cx);
        let rules = {
            let (library, name) = (library.clone(), name.clone());
            cx.new(|cx| RulesForm::new(library, queue, name, &opened.add_downloads, window, cx))
        };
        Self {
            library,
            main,
            rules,
            name,
            description,
            folder: opened.folder.own_ref().map(|folder| folder.0.clone()),
            own: owned(&opened),
            concurrent,
            limit,
            unit,
            connections,
            start,
            until_empty: schedule.is_some_and(|s| s.stop.is_none()),
            stop,
            days: pressed_days(schedule),
            notify: opened.notify,
            problem: None,
            stage: Stage::Editing,
            original,
            _subscriptions: edited.into(),
        }
    }

    fn read(&self, cx: &App) -> Result<QueueSettings, String> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("Give the queue a name.".to_owned());
        }
        let mut settings = self
            .original
            .as_ref()
            .map_or_else(|| QueueSettings::new(""), |queue| queue.settings.clone());
        settings.name = name;
        self.description
            .read(cx)
            .value()
            .trim()
            .clone_into(&mut settings.description);
        settings.folder = own_if(self.folder.is_some(), || {
            PortablePath(self.folder.clone().unwrap_or_default())
        });
        settings.max_concurrent = if self.is_own(Own::Concurrent) {
            Setting::Own(whole(&self.concurrent, MAX_CONCURRENT_DOWNLOADS, cx).ok_or_else(
                || format!("Downloads at the same time must be between 1 and {MAX_CONCURRENT_DOWNLOADS}."),
            )?)
        } else {
            Setting::Global
        };
        settings.connections =
            if self.is_own(Own::Connections) {
                let most = u32::from(MAX_CONNECTIONS_PER_DOWNLOAD);
                let typed = whole(&self.connections, most, cx).and_then(|n| u8::try_from(n).ok());
                Setting::Own(typed.ok_or_else(|| {
                    format!("Connections per download must be between 1 and {most}.")
                })?)
            } else {
                Setting::Global
            };
        settings.speed_limit = if self.is_own(Own::Speed) {
            Setting::Own(
                speed_limit(&self.limit.read(cx).value(), self.unit).ok_or_else(|| {
                    "Enter a speed limit above zero, or turn the limit off.".to_owned()
                })?,
            )
        } else {
            Setting::Global
        };
        settings.schedule = if self.is_own(Own::Schedule) {
            Some(self.schedule(cx)?)
        } else {
            None
        };
        settings.notify = self.notify;
        settings.add_downloads = self.rules.read(cx).add_downloads(cx);
        Ok(settings)
    }

    fn schedule(&self, cx: &App) -> Result<Schedule, String> {
        let start = time_of_day(self.start.read(cx).time());
        let stop = (!self.until_empty).then(|| time_of_day(self.stop.read(cx).time()));
        if stop == Some(start) {
            return Err(
                "The start and stop times are the same. Pick a different stop time.".to_owned(),
            );
        }
        if self.days.is_empty() {
            return Err("Pick at least one day.".to_owned());
        }
        Ok(Schedule {
            start,
            stop,
            days: saved_days(&self.days),
        })
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.stage == Stage::Saving {
            return;
        }
        let settings = match self.read(cx) {
            Ok(settings) => settings,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        let id = self.original.as_ref().map(|queue| queue.id);
        self.stage = Stage::Saving;
        self.problem = None;
        cx.notify();
        let saved = Backend::run(cx, move |manager| async move {
            match id {
                Some(id) => api::update_queue(&manager, id, settings).map(|()| id),
                None => api::create_queue(&manager, settings).map(|queue| queue.id),
            }
        });
        let main = self.main.clone();
        cx.spawn_in(window, async move |form, cx| {
            let outcome = match saved.await {
                Ok(Ok(result)) => result.map_err(|error| error.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = form.update_in(cx, |form, window, cx| {
                form.stage = Stage::Editing;
                match outcome {
                    Ok(saved) => {
                        window.close_dialog(cx);
                        if id.is_none() {
                            _ = main.update(cx, |main, cx| {
                                main.show(View::Queue(saved), window, cx);
                            });
                        }
                    }
                    Err(problem) => {
                        form.problem = Some(problem);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn ask_delete(&mut self, asking: bool, cx: &mut Context<'_, Self>) {
        self.stage = if asking {
            Stage::AskingToDelete
        } else {
            Stage::Editing
        };
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(id) = self.original.as_ref().map(|queue| queue.id) else {
            return;
        };
        window.close_dialog(cx);
        _ = self.main.update(cx, |main, cx| {
            if main.view == View::Queue(id) {
                main.show(View::All, window, cx);
            }
        });
        crate::backend::act(window.window_handle(), cx, move |manager| {
            api::delete_queue(manager, id)
        });
    }

    /// What deleting the queue does with its downloads.
    fn moving_note(&self, cx: &App) -> String {
        let Some(queue) = &self.original else {
            return String::new();
        };
        let library = self.library.read(cx);
        let count = library
            .downloads
            .iter()
            .filter(|row| row.queue == queue.id)
            .count();
        let to = library
            .queues
            .iter()
            .find(|queue| queue.is_default)
            .map_or("the default queue", |queue| queue.settings.name.as_str());
        match count {
            0 => "It holds no downloads.".to_owned(),
            1 => format!("Its download moves to {to}."),
            n => format!("Its {n} downloads move to {to}."),
        }
    }

    fn choose_folder(cx: &mut Context<'_, Self>) {
        let picked = files::choose_folder(cx);
        cx.spawn(async move |form, cx| {
            let Some(folder) = picked.await else {
                return;
            };
            _ = form.update(cx, |form, cx| {
                form.folder = Some(folder);
                cx.notify();
            });
        })
        .detach();
    }

    fn is_own(&self, setting: Own) -> bool {
        self.own.contains(&setting)
    }

    fn switch(&mut self, setting: Own, on: bool, cx: &mut Context<'_, Self>) {
        self.own.retain(|own| *own != setting);
        if on {
            self.own.push(setting);
        }
        self.problem = None;
        cx.notify();
    }

    /// Types the limit in `unit`, keeping the speed typed.
    fn set_unit(&mut self, unit: Unit, window: &mut Window, cx: &mut Context<'_, Self>) {
        if unit == self.unit {
            return;
        }
        let bytes = speed_limit(&self.limit.read(cx).value(), self.unit);
        self.unit = unit;
        self.limit.update(cx, |state, cx| {
            if let Some(bytes) = bytes {
                state.set_value(in_unit(bytes, unit), window, cx);
            }
            state.set_step(Some(unit.step().into()), window, cx);
        });
        cx.notify();
    }
}

impl Render for QueueForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let folder = FolderRow::new(
            self.folder.clone(),
            FOLDER_BY_TYPE,
            cx.listener(|_, _, _, cx| Self::choose_folder(cx)),
            cx.listener(|form, _, _, cx| {
                form.folder = None;
                cx.notify();
            }),
        );
        v_flex()
            .gap_5()
            .pb_1()
            .text_sm()
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
            .child(group("Limits", self.limits(cx), cx))
            .child(group(
                "Rules",
                vec![self.rules.clone().into_any_element()],
                cx,
            ))
            .child(group("Schedule", vec![self.schedule_row(cx)], cx))
            .child(group("Notifications", vec![self.notify_row(cx)], cx))
    }
}

impl QueueForm {
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
                        .w(px(72.))
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

/// A field edited takes away the problem the form showed.
fn forget_problem_on_edit(
    fields: [&Entity<InputState>; 5],
    cx: &mut Context<'_, QueueForm>,
) -> [Subscription; 5] {
    fields.map(|field| {
        cx.subscribe(field, |form: &mut QueueForm, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                form.problem = None;
                cx.notify();
            }
        })
    })
}

fn narrow(field: impl IntoElement) -> Div {
    div().w(px(132.)).child(field)
}

fn text_field(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<'_, QueueForm>,
) -> Entity<InputState> {
    let value = value.to_owned();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        state.set_value(value, window, cx);
        state
    })
}

fn number_field(
    value: u32,
    most: u32,
    window: &mut Window,
    cx: &mut Context<'_, QueueForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx)
            .step(1.)
            .min(1.)
            .max(f64::from(most));
        state.set_value(value.to_string(), window, cx);
        state
    })
}

fn time_field(
    at: TimeOfDay,
    window: &mut Window,
    cx: &mut Context<'_, QueueForm>,
) -> Entity<TimeFieldState> {
    let time =
        NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0).unwrap_or_default();
    cx.new(|cx| {
        let mut state = TimeFieldState::new(window, cx);
        state.set_time(time, window, cx);
        state
    })
}

fn time_of_day(time: NaiveTime) -> TimeOfDay {
    TimeOfDay {
        hour: u8::try_from(time.hour()).unwrap_or_default(),
        minute: u8::try_from(time.minute()).unwrap_or_default(),
    }
}

fn own_if<T>(own: bool, value: impl FnOnce() -> T) -> Setting<T> {
    if own {
        Setting::Own(value())
    } else {
        Setting::Global
    }
}

/// A whole number from 1 to `most` typed in `field`.
fn whole(field: &Entity<InputState>, most: u32, cx: &App) -> Option<u32> {
    let typed: u32 = field.read(cx).value().trim().parse().ok()?;
    (1..=most).contains(&typed).then_some(typed)
}

/// A typed speed limit in bytes per second, to the thousandth of `unit`; `None` unless it is a
/// number above zero. A comma reads as a decimal point.
fn speed_limit(typed: &str, unit: Unit) -> Option<u64> {
    let typed = typed.trim().replace(',', ".");
    let (whole, fraction) = typed.split_once('.').unwrap_or((&typed, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .chars()
            .chain(fraction.chars())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let thousandths: u64 = format!("{:0<3}", &fraction[..fraction.len().min(3)])
        .parse()
        .ok()?;
    let bytes = whole
        .checked_mul(unit.bytes())?
        .checked_add(thousandths * unit.bytes() / 1000)?;
    (bytes > 0).then_some(bytes)
}

/// `bytes` in `unit` to one decimal, without a trailing `.0`.
fn in_unit(bytes: u64, unit: Unit) -> String {
    let tenths = bytes.saturating_mul(10).saturating_add(unit.bytes() / 2) / unit.bytes();
    match tenths % 10 {
        0 => (tenths / 10).to_string(),
        tenth => format!("{}.{tenth}", tenths / 10),
    }
}

fn owned(opened: &QueueSettings) -> Vec<Own> {
    [
        (Own::Concurrent, opened.max_concurrent.own().is_some()),
        (Own::Speed, opened.speed_limit.own().is_some()),
        (Own::Connections, opened.connections.own().is_some()),
        (Own::Schedule, opened.schedule.is_some()),
    ]
    .into_iter()
    .filter_map(|(setting, own)| own.then_some(setting))
    .collect()
}

/// The days to press for a saved schedule, where none means every day.
fn pressed_days(schedule: Option<&Schedule>) -> Vec<Weekday> {
    match schedule.map(|s| s.days.as_slice()) {
        Some(days) if !days.is_empty() => days.to_vec(),
        _ => WEEK.iter().map(|(day, _)| *day).collect(),
    }
}

/// The days a schedule keeps: none for every day.
fn saved_days(pressed: &[Weekday]) -> Vec<Weekday> {
    if pressed.len() == WEEK.len() {
        return Vec::new();
    }
    WEEK.iter()
        .map(|(day, _)| *day)
        .filter(|day| pressed.contains(day))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_speed_limit_reads_in_its_unit() {
        assert_eq!(speed_limit("2", Unit::Mb), Some(2 * 1024 * 1024));
        assert_eq!(speed_limit("1,5", Unit::Mb), Some(1_572_864));
        assert_eq!(speed_limit(".5", Unit::Mb), Some(524_288));
        assert_eq!(speed_limit("300", Unit::Kb), Some(300 * 1024));
        assert_eq!(speed_limit("0", Unit::Kb), None);
        assert_eq!(speed_limit("-1", Unit::Kb), None);
        assert_eq!(speed_limit("fast", Unit::Mb), None);
        assert_eq!(in_unit(1_572_864, Unit::Mb), "1.5");
        assert_eq!(in_unit(300 * 1024, Unit::Kb), "300");
        assert_eq!(Unit::of(1_572_864), Unit::Mb);
    }

    #[test]
    fn every_day_is_saved_as_none_and_the_rest_in_week_order() {
        let all: Vec<_> = WEEK.iter().map(|(day, _)| *day).collect();
        assert_eq!(saved_days(&all), Vec::new());
        assert_eq!(
            saved_days(&[Weekday::Sun, Weekday::Mon]),
            vec![Weekday::Mon, Weekday::Sun]
        );
    }
}
