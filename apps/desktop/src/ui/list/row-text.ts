// What a download's row says under its name, by the group it is in: what went wrong, how it is going, where it comes from, or where a finished file is.

import type { DownloadSummary } from '@wirecatch/protocol';
import { folderOf, hostOf, STATUS_LABEL, statusShort, whenSaid } from '$lib/format';
import type { StatusGroup } from '$lib/state/view';

/** Where it comes from, and over how many connections while it downloads. */
function source(row: DownloadSummary): string {
  if (row.status !== 'downloading') return statusShort(row);
  const host = hostOf(row.url);
  if (row.connections === 0) return host;
  return `${host} · ${String(row.connections)} connection${row.connections === 1 ? '' : 's'}`;
}

/** When a finished download finished: "Finished today at 12:04", "Finished on 3 Oct 2026". */
export function finishedNote(row: Pick<DownloadSummary, 'completed_at'>, now: Date): string {
  const done = STATUS_LABEL.completed;
  return row.completed_at === null ? done : `${done} ${whenSaid(row.completed_at, now)}`;
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
      return row.error?.message ?? statusShort(row);
    case 'active':
      return source(row);
    case 'waiting':
      return progressShown ? hostOf(row.url) : waitNote;
    case 'finished':
      return progressShown ? folderOf(row.path) : finishedNote(row, now);
    case 'cancelled':
      return statusShort(row);
    case 'paused':
      return hostOf(row.url);
  }
}
