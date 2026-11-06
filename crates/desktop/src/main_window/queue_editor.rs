//! A queue's own page in the main window (D255): its settings under tabs, General (name,
//! description, its own folder, D94), Rules, Limits, Schedule, Network and Notifications. A
//! limit follows the app's until it is switched on (D72.1). Its rules and its proxy have editors
//! of their own.

mod tabs;

use chrono::{NaiveTime, Timelike as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::time_field::TimeFieldState;
use gpui_kit::{
    App, AppContext as _, Context, Entity, SharedString, Subscription, WeakEntity, Window,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD, PortablePath, Queue, QueueNotify,
    QueueSettings, Schedule, Setting, TimeOfDay, Weekday,
};
use wirecatch_shell::api;

use super::MainWindow;
use super::queue_proxy::ProxyForm;
use super::queue_rules::RulesForm;
use crate::backend::{Backend, said};
use crate::library::Library;
use crate::{files, text};

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
}

/// A tab of the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Section {
    General,
    Rules,
    Limits,
    Schedule,
    Network,
    Notifications,
}

impl Section {
    const ALL: [Self; 6] = [
        Self::General,
        Self::Rules,
        Self::Limits,
        Self::Schedule,
        Self::Network,
        Self::Notifications,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Rules => "Rules",
            Self::Limits => "Limits",
            Self::Schedule => "Schedule",
            Self::Network => "Network",
            Self::Notifications => "Notifications",
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|section| *section == self)
            .unwrap_or_default()
    }
}

/// What the form cannot save, and the tab to show it on.
type Problem = (Section, String);

pub struct QueueForm {
    library: Entity<Library>,
    main: WeakEntity<MainWindow>,
    /// The queue as it was when the page opened; `None` for a new one.
    original: Option<Queue>,
    /// What the way back says: the queue, or the view the page was opened from.
    back: SharedString,
    section: Section,
    name: Entity<InputState>,
    description: Entity<InputState>,
    folder: Option<String>,
    /// The settings switched on to be the queue's own.
    own: Vec<Own>,
    concurrent: Entity<InputState>,
    limit: Entity<InputState>,
    /// The limit the field opened on, kept exact while its shown value stays.
    opened_limit: u64,
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
    proxy: Entity<ProxyForm>,
    _subscriptions: Vec<Subscription>,
}

/// What the page opens on.
pub(super) enum Opening {
    /// A queue that exists.
    Queue(QueueId),
    /// A queue not made yet, starting from these settings.
    New(Box<QueueSettings>),
}

impl Opening {
    /// The queue as it is, if it exists, and the settings the form starts from.
    fn read(self, library: &Library) -> (Option<Queue>, QueueSettings) {
        match self {
            Self::Queue(id) => {
                let original = library.queue(id).cloned();
                let opened = original
                    .as_ref()
                    .map_or_else(|| QueueSettings::new(""), |queue| queue.settings.clone());
                (original, opened)
            }
            Self::New(settings) => (None, *settings),
        }
    }
}

impl QueueForm {
    pub(super) fn new(
        library: Entity<Library>,
        opening: Opening,
        back: SharedString,
        main: WeakEntity<MainWindow>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let (original, opened) = opening.read(library.read(cx));
        let queue = original.as_ref().map(|queue| queue.id);
        let app = library.read(cx).settings.clone();

        let name =
            crate::ui::dialog::text_field(&opened.name, "For example: Overnight", window, cx);
        let description = crate::ui::dialog::text_field(
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
        let limit = limit_field(limit_bytes, unit, window, cx);
        let schedule = opened.schedule.as_ref();
        let start = time_field(schedule.map_or(FIRST_START, |s| s.start), window, cx);
        let stop = time_field(
            schedule.and_then(|s| s.stop).unwrap_or(FIRST_STOP),
            window,
            cx,
        );
        let edited =
            forget_problem_on_edit([&name, &description, &concurrent, &limit, &connections], cx);
        let (rules, proxy) = editors(&library, queue, &name, &opened, window, cx);
        Self {
            library,
            main,
            rules,
            proxy,
            name,
            description,
            folder: opened.folder.own_ref().map(|folder| folder.0.clone()),
            own: owned(&opened),
            concurrent,
            limit,
            opened_limit: limit_bytes,
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
            back,
            section: Section::General,
            _subscriptions: edited.into(),
        }
    }

    /// Gives the keyboard to the name of `form`'s queue.
    pub(super) fn focus_name(form: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let name = form.read(cx).name.clone();
        name.update(cx, |name, cx| name.focus(window, cx));
    }

    fn read(&self, cx: &App) -> Result<QueueSettings, Problem> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err((Section::General, "Give the queue a name.".to_owned()));
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
        let limits = |problem: String| (Section::Limits, problem);
        settings.max_concurrent = if self.is_own(Own::Concurrent) {
            Setting::Own(whole(&self.concurrent, MAX_CONCURRENT_DOWNLOADS, cx).ok_or_else(
                || limits(format!("Downloads at the same time must be between 1 and {MAX_CONCURRENT_DOWNLOADS}.")),
            )?)
        } else {
            Setting::Global
        };
        settings.connections = if self.is_own(Own::Connections) {
            let most = u32::from(MAX_CONNECTIONS_PER_DOWNLOAD);
            let typed = whole(&self.connections, most, cx).and_then(|n| u8::try_from(n).ok());
            Setting::Own(typed.ok_or_else(|| {
                limits(format!(
                    "Connections per download must be between 1 and {most}."
                ))
            })?)
        } else {
            Setting::Global
        };
        settings.speed_limit = if self.is_own(Own::Speed) {
            Setting::Own(
                limit_from(&self.limit.read(cx).value(), self.unit, self.opened_limit).ok_or_else(
                    || limits("Enter a speed limit above zero, or turn the limit off.".to_owned()),
                )?,
            )
        } else {
            Setting::Global
        };
        settings.schedule = if self.is_own(Own::Schedule) {
            Some(
                self.schedule(cx)
                    .map_err(|problem| (Section::Schedule, problem))?,
            )
        } else {
            None
        };
        settings.notify = self.notify;
        settings.add_downloads = self.rules.read(cx).add_downloads(cx);
        settings.proxy = self.proxy.read(cx).proxy();
        settings.exceptions = self.proxy.read(cx).exceptions(cx);
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
            Err((section, problem)) => {
                self.section = section;
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
            let Some(outcome) = said(saved.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
                form.stage = Stage::Editing;
                match outcome {
                    Ok(saved) => {
                        crate::closing::may_have_closed(main.update(cx, |main, cx| {
                            main.leave_queue_page(saved, window, cx);
                        }));
                    }
                    Err(problem) => {
                        form.problem = Some(problem);
                        cx.notify();
                    }
                }
            }));
        })
        .detach();
    }

    /// Back to the downloads, unsaved changes left behind.
    fn go_back(&self, window: &mut Window, cx: &mut App) {
        let main = self.main.clone();
        crate::closing::may_have_closed(
            main.update(cx, |main, cx| main.close_queue_page(window, cx)),
        );
    }

    /// Asks before deleting the queue; the page closes once it is deleted.
    fn delete(&self, window: &mut Window, cx: &mut App) {
        let Some(id) = self.original.as_ref().map(|queue| queue.id) else {
            return;
        };
        crate::closing::may_have_closed(
            self.main
                .update(cx, |main, cx| main.delete_queue(id, window, cx)),
        );
    }

    fn choose_folder(cx: &mut Context<'_, Self>) {
        let picked = files::choose_folder(cx);
        cx.spawn(async move |form, cx| {
            let Some(folder) = picked.await else {
                return;
            };
            crate::closing::may_have_closed(form.update(cx, |form, cx| {
                form.folder = Some(folder);
                cx.notify();
            }));
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
        let bytes = limit_from(&self.limit.read(cx).value(), self.unit, self.opened_limit);
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

/// The rules and proxy editors, which keep their own state.
fn editors(
    library: &Entity<Library>,
    queue: Option<QueueId>,
    name: &Entity<InputState>,
    opened: &QueueSettings,
    window: &mut Window,
    cx: &mut Context<'_, QueueForm>,
) -> (Entity<RulesForm>, Entity<ProxyForm>) {
    let rules = cx.new(|cx| {
        RulesForm::new(
            library,
            queue,
            name.clone(),
            &opened.add_downloads,
            window,
            cx,
        )
    });
    let proxy = cx.new(|cx| {
        ProxyForm::new(
            library.clone(),
            opened.proxy,
            opened.exceptions.clone(),
            window,
            cx,
        )
    });
    (rules, proxy)
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

fn limit_field(
    bytes: u64,
    unit: Unit,
    window: &mut Window,
    cx: &mut Context<'_, QueueForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).min(0.).step(unit.step());
        state.set_value(in_unit(bytes, unit), window, cx);
        state
    })
}

fn time_field(at: TimeOfDay, window: &mut Window, cx: &mut App) -> Entity<TimeFieldState> {
    let time =
        NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0).unwrap_or_default();
    cx.new(|cx| {
        let mut state = TimeFieldState::new(window, cx).hour_cycle(text::hour_cycle());
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

/// The limit in the field: `opened` while it still reads as shown, as showing it may round it.
fn limit_from(typed: &str, unit: Unit, opened: u64) -> Option<u64> {
    if typed.trim() == in_unit(opened, unit) {
        return Some(opened);
    }
    speed_limit(typed, unit)
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
    use gpui_kit::component::time_field::TimeField;
    use gpui_kit::{IntoElement, Render, TestAppContext};
    use wirecatch_protocol::app::ClockFormat;

    use super::*;

    struct Clock(Entity<TimeFieldState>);

    impl Render for Clock {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            TimeField::new(&self.0)
        }
    }

    #[gpui_kit::test]
    fn a_schedule_time_is_typed_on_the_chosen_clock(cx: &mut TestAppContext) {
        cx.update(gpui_kit::component::init);
        text::set_clock(ClockFormat::TwelveHour);
        let (clock, cx) =
            cx.add_window_view(|window, cx| Clock(time_field(FIRST_START, window, cx)));
        let field = clock.read_with(cx, |clock, _| clock.0.clone());
        cx.update(|window, cx| field.update(cx, |field, cx| field.focus(window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("1 3");
        let typed = field.read_with(cx, |field, _| field.time());
        assert_eq!(typed.hour(), 3, "13 is no hour on a 12-hour clock");
    }

    #[test]
    fn a_limit_left_as_shown_is_saved_unchanged() {
        const ROUNDED: u64 = 1075 * 1024;
        assert_eq!(in_unit(ROUNDED, Unit::Mb), "1");
        assert_eq!(limit_from("1", Unit::Mb, ROUNDED), Some(ROUNDED));
        assert_eq!(limit_from(" 1 ", Unit::Mb, ROUNDED), Some(ROUNDED));
        assert_eq!(limit_from("1075", Unit::Kb, ROUNDED), Some(ROUNDED));
        assert_eq!(limit_from("2", Unit::Mb, ROUNDED), Some(2 * 1024 * 1024));
        assert_eq!(limit_from("", Unit::Mb, ROUNDED), None);
    }

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
