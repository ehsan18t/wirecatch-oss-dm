//! When a scheduled queue may run. Pure functions of the schedule and the local time.
//!
//! A schedule with a stop time is a window: the queue runs from `start` until
//! `stop` on the listed days, and a stop earlier than the start means the window
//! ends the next morning. A schedule without a stop time starts the queue at
//! `start` and lets it run until it has nothing left to do.

use std::cmp::Ordering;

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike};
use wirecatch_protocol::app::{Schedule, TimeOfDay, Weekday};

/// How far back a jump of the clock is searched for a missed start time.
const MAX_CATCH_UP_DAYS: i64 = 7;

/// Whether a queue may start downloads, remembering a run that a schedule without
/// a stop time has begun.
#[derive(Debug, Default, Clone, Copy)]
pub struct Gate {
    running_until_empty: bool,
}

impl Gate {
    /// Re-evaluates the gate for the interval of local time `(since, now]`.
    /// `has_work` tells whether the queue still has downloads waiting or running.
    pub fn is_open(
        &mut self,
        schedule: Option<&Schedule>,
        since: NaiveDateTime,
        now: NaiveDateTime,
        has_work: bool,
    ) -> bool {
        let Some(schedule) = schedule else {
            return true;
        };
        if schedule.stop.is_some() {
            return window_open(schedule, now);
        }
        if start_crossed(schedule, since, now) {
            self.running_until_empty = true;
        }
        if !has_work {
            self.running_until_empty = false;
        }
        self.running_until_empty
    }

    /// Continues a run that was under way when the app last stopped.
    pub fn resume_run(&mut self) {
        self.running_until_empty = true;
    }

    /// Forgets any run in progress, for example after the schedule was edited.
    pub fn reset(&mut self) {
        self.running_until_empty = false;
    }
}

/// Whether a schedule with a stop time is inside its window at `now`. A schedule
/// without a stop time has no window and is never open by this test.
fn window_open(schedule: &Schedule, now: NaiveDateTime) -> bool {
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
            let yesterday = today - TimeDelta::days(1);
            (runs_on(schedule, today) && t >= start) || (runs_on(schedule, yesterday) && t < stop)
        }
        // The same time for both: the whole day.
        Ordering::Equal => runs_on(schedule, today),
    }
}

/// Whether a start time of the schedule falls in `(since, now]`.
fn start_crossed(schedule: &Schedule, since: NaiveDateTime, now: NaiveDateTime) -> bool {
    let Some(start) = to_time(schedule.start) else {
        return false;
    };
    if now <= since {
        return false;
    }
    let first = since
        .date()
        .max(now.date() - TimeDelta::days(MAX_CATCH_UP_DAYS));
    first
        .iter_days()
        .take_while(|day| *day <= now.date())
        .any(|day| {
            let at = day.and_time(start);
            since < at && at <= now && runs_on(schedule, day)
        })
}

/// The start of the minute after `now`. Schedules have minute precision, so this
/// is the next moment their answer can change.
#[must_use]
pub fn next_minute(now: NaiveDateTime) -> NaiveDateTime {
    let floor = now
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(now);
    floor + TimeDelta::minutes(1)
}

/// Whether `time` names a real time of day.
#[must_use]
pub fn is_valid_time(time: TimeOfDay) -> bool {
    to_time(time).is_some()
}

fn to_time(time: TimeOfDay) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(time.hour.into(), time.minute.into(), 0)
}

fn runs_on(schedule: &Schedule, day: NaiveDate) -> bool {
    schedule.days.is_empty() || schedule.days.contains(&weekday(day))
}

fn weekday(day: NaiveDate) -> Weekday {
    match day.weekday() {
        chrono::Weekday::Mon => Weekday::Mon,
        chrono::Weekday::Tue => Weekday::Tue,
        chrono::Weekday::Wed => Weekday::Wed,
        chrono::Weekday::Thu => Weekday::Thu,
        chrono::Weekday::Fri => Weekday::Fri,
        chrono::Weekday::Sat => Weekday::Sat,
        chrono::Weekday::Sun => Weekday::Sun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-12 is a Monday.
    fn mon(hour: u32, minute: u32) -> NaiveDateTime {
        day(12, hour, minute)
    }

    fn day(d: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, d)
            .and_then(|d| d.and_hms_opt(hour, minute, 0))
            .unwrap()
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
        let mut gate = Gate::default();
        assert!(!gate.is_open(Some(&s), mon(1, 0), mon(1, 30), true));
        assert!(gate.is_open(Some(&s), mon(1, 30), mon(2, 0), true));
        assert!(gate.is_open(Some(&s), mon(2, 0), mon(9, 0), true));
        assert!(!gate.is_open(Some(&s), mon(9, 0), mon(9, 1), false));
        // Work added later waits for the next start.
        assert!(!gate.is_open(Some(&s), mon(9, 1), mon(12, 0), true));
        assert!(gate.is_open(Some(&s), mon(12, 0), day(13, 2, 0), true));
    }

    #[test]
    fn gate_without_schedule_is_always_open() {
        let mut gate = Gate::default();
        assert!(gate.is_open(None, mon(1, 0), mon(1, 0), false));
    }

    #[test]
    fn next_minute_rounds_up() {
        let t = mon(1, 0) + TimeDelta::seconds(30);
        assert_eq!(next_minute(t), mon(1, 1));
        assert_eq!(next_minute(mon(1, 0)), mon(1, 1));
    }
}
