// The days a queue's schedule runs on, as the queue dialog picks them: a pattern first (every day, weekdays, weekends), single days only when asked for.
import type { Weekday } from '@wirecatch/protocol';
import { daysInPhrase, type DayPattern } from './format';
import { DAYS_PER_WEEK, WEEKDAYS, WEEKEND, WORKDAYS } from './time';

const DAYS_OF: Record<Exclude<DayPattern, 'custom'>, readonly Weekday[]> = {
  every: [],
  weekdays: WORKDAYS,
  weekends: WEEKEND,
};

/** The days once `next` is picked; picking days starts from the days it runs on now, every day being all seven ticked. */
export function daysForPattern(next: DayPattern, days: readonly Weekday[]): Weekday[] {
  if (next === 'custom') return days.length === 0 ? [...WEEKDAYS] : [...days];
  return [...DAYS_OF[next]];
}

/** Picking days limits the queue to those days; picking none means every day. */
export function toggledDay(days: readonly Weekday[], day: Weekday): Weekday[] {
  return days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
}

export function pickedDaysHelp(days: readonly Weekday[]): string {
  if (days.length === 0) return 'Pick at least one day.';
  if (days.length === DAYS_PER_WEEK) return 'Runs every day.';
  return `Runs only on ${daysInPhrase(days)}.`;
}
