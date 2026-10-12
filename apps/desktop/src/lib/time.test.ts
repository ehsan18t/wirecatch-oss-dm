import { describe, expect, it } from 'vitest';
import { dayBefore, isTimeOfDay, minutesOfDay, timeOfDay, weekdayOf } from './time';

describe('time', () => {
  it('names the weekday of a date, Monday first', () => {
    // 10 October 2026 is a Saturday, and 12 October a Monday.
    expect(weekdayOf(new Date(2026, 9, 10, 12, 0))).toBe('sat');
    expect(weekdayOf(new Date(2026, 9, 11, 23, 59))).toBe('sun');
    expect(weekdayOf(new Date(2026, 9, 12, 0, 0))).toBe('mon');
    expect(weekdayOf(dayBefore(new Date(2026, 9, 12, 0, 30)))).toBe('sun');
  });

  it('counts minutes since midnight', () => {
    expect(minutesOfDay({ hour: 0, minute: 0 })).toBe(0);
    expect(minutesOfDay({ hour: 7, minute: 5 })).toBe(425);
    expect(minutesOfDay(timeOfDay(new Date(2026, 9, 10, 23, 59)))).toBe(1439);
  });

  it('knows a real time of day', () => {
    expect(isTimeOfDay({ hour: 23, minute: 59 })).toBe(true);
    expect(isTimeOfDay({ hour: 24, minute: 0 })).toBe(false);
    expect(isTimeOfDay({ hour: 7, minute: 60 })).toBe(false);
  });

  it('goes back one calendar day across a month', () => {
    const before = dayBefore(new Date(2026, 9, 1, 8, 15));
    expect([before.getMonth(), before.getDate(), before.getHours()]).toEqual([8, 30, 8]);
  });
});
