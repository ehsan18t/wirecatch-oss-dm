// What a download's row says under its name, by the group it is in: what went wrong, in a few words, or where it comes from and how it is going.

import type { DownloadSummary } from '@wirecatch/protocol';
import { hostOf, STATUS_LABEL, statusShort, whenSaid } from '$lib/format';
import type { StatusGroup } from '$lib/state/view';

/** Where it comes from while it downloads; how many connections it uses is the details' to say. */
function source(row: DownloadSummary): string {
  return row.status === 'downloading' ? hostOf(row.url) : statusShort(row);
}

/** What went wrong in a few words: the first sentence of the error, whose rest (what to do) the details and the row's tooltip say. */
export function failureShort(message: string): string {
  const end = message.search(/[.!?](?:\s|$)/);
  return end === -1 ? message : message.slice(0, end + 1);
}

/** When a finished download finished: "Finished today at 12:04", "Finished on 3 Oct 2026". */
export function finishedNote(row: Pick<DownloadSummary, 'completed_at'>, now: Date): string {
  const done = STATUS_LABEL.completed;
  return row.completed_at === null ? done : `${done} ${whenSaid(row.completed_at, now)}`;
}

/** When a finished download finished, under a heading or in a column that already says Finished: "Today at 12:04", "3 Oct 2026". */
export function finishedWhen(row: Pick<DownloadSummary, 'completed_at'>, now: Date): string {
  if (row.completed_at === null) return STATUS_LABEL.completed;
  const when = whenSaid(row.completed_at, now).replace(/^on /, '');
  return `${when.charAt(0).toUpperCase()}${when.slice(1)}`;
}

/**
 * The line under a row's name. A waiting download says why it waits, and a finished one when it finished, there only when the list has no progress column to say it in.
 */
/** What the line under a row's name depends on beyond the row. */
export interface LineContext {
  /** Why a waiting download waits, or where it stands in its queue's line. */
  waitNote: string;
  /** The list shows its progress column, which then says the wait and the finish time. */
  progressShown: boolean;
  now: Date;
}

export function rowLine(
  row: DownloadSummary,
  group: StatusGroup,
  { waitNote, progressShown, now }: LineContext,
): string {
  switch (group) {
    case 'failed':
      return row.error ? failureShort(row.error.message) : statusShort(row);
    case 'active':
      return source(row);
    case 'waiting':
      return progressShown ? hostOf(row.url) : waitNote;
    case 'finished':
      return progressShown ? hostOf(row.url) : finishedNote(row, now);
    case 'cancelled':
      return statusShort(row);
    case 'paused':
      return hostOf(row.url);
  }
}
