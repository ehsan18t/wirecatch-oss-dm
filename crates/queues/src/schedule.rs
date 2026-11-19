//! With a stop time a schedule is a window, which ends the next morning when the stop is before
//! the start; without one, the queue starts at `start` and runs until it has nothing left.

use std::cmp::Ordering;

use jiff::ToSpan as _;
use jiff::civil::{Date, DateTime, Time};
use wirecatch_protocol::app::{Schedule, TimeOfDay, Weekday};

/// How far back a jump of the clock is searched for a missed start time.
const MAX_CATCH_UP_DAYS: i64 = 7;

/// Brings the saved `running_until_empty` of a schedule without a stop time up to local time `(since, now]`; `has_work`: downloads still waiting, running or about to go on.
pub fn follow_run(
    schedule: &Schedule,
    since: DateTime,
    now: DateTime,
    has_work: bool,
    running_until_empty: &mut bool,
) {
    if schedule.stop.is_some() {
        *running_until_empty = false;
        return;
    }
    if start_crossed(schedule, since, now) {
        *running_until_empty = true;
    }
    if !has_work {
        *running_until_empty = false;
    }
}

/// Whether `schedule` lets its queue run at local time `now`: inside its window, or, without a stop time, during a run. The core and the window both read this (D155).
#[must_use]
pub fn allows(schedule: &Schedule, running_until_empty: bool, now: DateTime) -> bool {
    if schedule.stop.is_some() {
        window_open(schedule, now)
    } else {
        running_until_empty
    }
}

/// A schedule without a stop time has no window and is never open here.
fn window_open(schedule: &Schedule, now: DateTime) -> bool {
    let (Some(start), Some(stop)) = (to_time(schedule.start), schedule.stop.and_then(to_time))
    else {
        return false;
    };
    let today = now.date();
    let t = now.time();
    match start.cmp(&stop) {
        Ordering::Less => runs_on(schedule, today) && start <= t && t < stop,
        // The window runs past midnight.
        Ordering::Greater => {
            let yesterday = today - 1.day();
            (runs_on(schedule, today) && t >= start) || (runs_on(schedule, yesterday) && t < stop)
        }
        // The same time for both: the whole day.
        Ordering::Equal => runs_on(schedule, today),
    }
}

fn start_crossed(schedule: &Schedule, since: DateTime, now: DateTime) -> bool {
    let Some(start) = to_time(schedule.start) else {
        return false;
    };
    if now <= since {
        return false;
    }
    let first = since.date().max(now.date() - MAX_CATCH_UP_DAYS.days());
    first
        .series(1.day())
        .take_while(|day| *day <= now.date())
        .any(|day| {
            let at = day.to_datetime(start);
            since < at && at <= now && runs_on(schedule, day)
        })
}

/// Schedules have minute precision: the next moment their answer can change.
#[must_use]
pub fn next_minute(now: DateTime) -> DateTime {
    let floor = now
        .with()
        .second(0)
        .subsec_nanosecond(0)
        .build()
        .unwrap_or(now);
    floor + 1.minute()
}

#[must_use]
pub fn is_valid_time(time: TimeOfDay) -> bool {
    to_time(time).is_some()
}

/// `None` when `time` is not a time of day.
#[must_use]
pub fn to_time(time: TimeOfDay) -> Option<Time> {
    let hour = i8::try_from(time.hour).ok()?;
    let minute = i8::try_from(time.minute).ok()?;
    Time::new(hour, minute, 0, 0).ok()
}

fn runs_on(schedule: &Schedule, day: Date) -> bool {
    schedule.days.is_empty() || schedule.days.contains(&weekday(day))
}

fn weekday(day: Date) -> Weekday {
    match day.weekday() {
        jiff::civil::Weekday::Monday => Weekday::Mon,
        jiff::civil::Weekday::Tuesday => Weekday::Tue,
        jiff::civil::Weekday::Wednesday => Weekday::Wed,
        jiff::civil::Weekday::Thursday => Weekday::Thu,
        jiff::civil::Weekday::Friday => Weekday::Fri,
        jiff::civil::Weekday::Saturday => Weekday::Sat,
        jiff::civil::Weekday::Sunday => Weekday::Sun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-12 is a Monday.
    fn mon(hour: i8, minute: i8) -> DateTime {
        day(12, hour, minute)
    }

    fn day(d: i8, hour: i8, minute: i8) -> DateTime {
        jiff::civil::datetime(2026, 10, d, hour, minute, 0, 0)
    }

    fn tod(hour: u8, minute: u8) -> TimeOfDay {
        TimeOfDay { hour, minute }
    }

    fn window(start: TimeOfDay, stop: TimeOfDay, days: Vec<Weekday>) -> Schedule {
        Schedule {
            start,
            stop: Some(stop),
            days,
        }
    }

    #[test]
    fn a_schedule_without_a_stop_allows_running_from_its_start_on_its_days() {
        let s = Schedule {
            start: tod(22, 0),
            stop: None,
            days: vec![Weekday::Mon],
        };
        assert!(!allows(&s, false, mon(22, 30)));
        assert!(allows(&s, true, mon(21, 59)));
        assert!(allows(&s, true, day(13, 3, 0)));
        let windowed = window(tod(2, 0), tod(6, 30), vec![]);
        assert_eq!(
            allows(&windowed, false, mon(3, 0)),
            window_open(&windowed, mon(3, 0))
        );
    }

    #[test]
    fn same_day_window_is_half_open() {
        let s = window(tod(2, 0), tod(6, 30), vec![]);
        assert!(!window_open(&s, mon(1, 59)));
        assert!(window_open(&s, mon(2, 0)));
        assert!(window_open(&s, mon(6, 29)));
        assert!(!window_open(&s, mon(6, 30)));
    }

    #[test]
    fn overnight_window_belongs_to_the_day_it_starts() {
        let s = window(tod(23, 0), tod(7, 0), vec![Weekday::Mon]);
        assert!(window_open(&s, mon(23, 30)));
        // Tuesday morning is still Monday night's window.
        assert!(window_open(&s, day(13, 6, 59)));
        assert!(!window_open(&s, day(13, 7, 0)));
        // Monday morning belongs to Sunday night, which is not listed.
        assert!(!window_open(&s, mon(3, 0)));
        assert!(!window_open(&s, day(13, 23, 30)));
    }

    #[test]
    fn days_limit_the_window() {
        let s = window(tod(9, 0), tod(17, 0), vec![Weekday::Sat, Weekday::Sun]);
        assert!(!window_open(&s, mon(10, 0)));
        assert!(window_open(&s, day(17, 10, 0)));
    }

    #[test]
    fn equal_start_and_stop_means_all_day() {
        let s = window(tod(8, 0), tod(8, 0), vec![Weekday::Mon]);
        assert!(window_open(&s, mon(0, 0)));
        assert!(window_open(&s, mon(23, 59)));
        assert!(!window_open(&s, day(13, 12, 0)));
    }

    #[test]
    fn invalid_times_never_open() {
        let s = window(tod(25, 0), tod(6, 0), vec![]);
        assert!(!window_open(&s, mon(3, 0)));
        assert!(!is_valid_time(tod(12, 60)));
        assert!(is_valid_time(tod(23, 59)));
    }

    #[test]
    fn start_crossed_finds_starts_inside_the_interval_only() {
        let s = Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![],
        };
        assert!(start_crossed(&s, mon(1, 59), mon(2, 0)));
        assert!(!start_crossed(&s, mon(2, 0), mon(2, 1)));
        assert!(!start_crossed(&s, mon(1, 0), mon(1, 59)));
        // A jump across midnight still sees the next day's start.
        assert!(start_crossed(&s, mon(23, 0), day(13, 3, 0)));
        assert!(!start_crossed(&s, mon(3, 0), mon(2, 0)));
    }

    #[test]
    fn start_crossed_respects_days() {
        let s = Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![Weekday::Tue],
        };
        assert!(!start_crossed(&s, mon(1, 0), mon(3, 0)));
        assert!(start_crossed(&s, day(13, 1, 0), day(13, 3, 0)));
    }

    #[test]
    fn gate_without_stop_runs_until_the_queue_is_empty() {
        let s = Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![],
        };
        let mut run = false;
        let mut step = |since, now, has_work| {
            follow_run(&s, since, now, has_work, &mut run);
            run
        };
        assert!(!step(mon(1, 0), mon(1, 30), true));
        assert!(step(mon(1, 30), mon(2, 0), true));
        assert!(step(mon(2, 0), mon(9, 0), true));
        assert!(!step(mon(9, 0), mon(9, 1), false));
        // Work added later waits for the next start.
        assert!(!step(mon(9, 1), mon(12, 0), true));
        assert!(step(mon(12, 0), day(13, 2, 0), true));
    }

    #[test]
    fn next_minute_rounds_up() {
        let t = mon(1, 0) + 30.seconds();
        assert_eq!(next_minute(t), mon(1, 1));
        assert_eq!(next_minute(mon(1, 0)), mon(1, 1));
    }
}
