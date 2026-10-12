// Clock and calendar arithmetic with its units named, so a number in a formula says what it is.

import type { TimeOfDay, Weekday } from '@wirecatch/protocol';

export const MS_PER_SECOND = 1000;
export const SECONDS_PER_MINUTE = 60;
export const MINUTES_PER_HOUR = 60;
export const HOURS_PER_DAY = 24;
export const MS_PER_MINUTE = SECONDS_PER_MINUTE * MS_PER_SECOND;
export const MS_PER_HOUR = MINUTES_PER_HOUR * MS_PER_MINUTE;

/** The days of the week as the core names them, Monday first. */
export const WEEKDAYS: readonly Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

export const DAYS_PER_WEEK = WEEKDAYS.length;

/** Monday to Friday. */
export const WORKDAYS: readonly Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri'];

export const WEEKEND: readonly Weekday[] = ['sat', 'sun'];

/** Minutes since midnight. */
export function minutesOfDay(t: TimeOfDay): number {
  return t.hour * MINUTES_PER_HOUR + t.minute;
}

/** The time of day of a date, in local time. */
export function timeOfDay(date: Date): TimeOfDay {
  return { hour: date.getHours(), minute: date.getMinutes() };
}

/** Whether `t` names a real time of day: hours 0 to 23, minutes 0 to 59. */
export function isTimeOfDay(t: TimeOfDay): boolean {
  return t.hour < HOURS_PER_DAY && t.minute < MINUTES_PER_HOUR;
}

/** The weekday of a date, in local time. */
export function weekdayOf(date: Date): Weekday {
  // `getDay` counts from Sunday as 0; `WEEKDAYS` starts on Monday.
  const day = WEEKDAYS[(date.getDay() + DAYS_PER_WEEK - 1) % DAYS_PER_WEEK];
  if (day === undefined) throw new RangeError(`No weekday for ${date.toString()}`);
  return day;
}

/** The same time of day, one calendar day earlier. */
export function dayBefore(date: Date): Date {
  const earlier = new Date(date);
  earlier.setDate(date.getDate() - 1);
  return earlier;
}

/**
 * The date at `ms` since 1970, as the calendar helpers take it. Each call gives a new value for
 * one reading, never kept and changed, so it needs no reactive date: state holds the number.
 */
export function dateAt(ms: number): Date {
  return new Date(ms);
}
