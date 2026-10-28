// A queue's hours as its editor reads them (D72): the times typed and the days picked, checked into a schedule.

import type { QueueSettings, TimeOfDay, Weekday } from '@wirecatch/protocol';
import { formatTime, parseTime } from './format';
import { DAYS_PER_WEEK, WEEKDAYS } from './time';

/** What the form suggests when a schedule is turned on for a queue that has none. */
export const FIRST_START: TimeOfDay = { hour: 1, minute: 0 };
export const FIRST_STOP: TimeOfDay = { hour: 7, minute: 0 };

/** The schedule fields of the queue editor. */
export interface ScheduleForm {
  on: boolean;
  start: string;
  untilEmpty: boolean;
  stop: string;
  pickingDays: boolean;
  days: readonly Weekday[];
}

/** The schedule `form` describes, or why it is not one. */
export function readSchedule(form: ScheduleForm): { schedule: QueueSettings['schedule'] } | string {
  if (!form.on) return { schedule: null };
  const s = parseTime(form.start);
  const e = form.untilEmpty ? null : parseTime(form.stop);
  if (!s) return `Enter a start time like ${formatTime(FIRST_START)}.`;
  if (!form.untilEmpty && !e) return `Enter a stop time like ${formatTime(FIRST_STOP)}.`;
  if (e?.hour === s.hour && e.minute === s.minute)
    return 'The start and stop times are the same. Pick a different stop time.';
  if (form.pickingDays && form.days.length === 0)
    return 'Pick at least one day for the queue to run.';
  // Seven ticked days is the same as every day.
  const every = form.days.length === DAYS_PER_WEEK;
  return {
    schedule: {
      start: s,
      stop: e,
      days: every ? [] : WEEKDAYS.filter((d) => form.days.includes(d)),
    },
  };
}
