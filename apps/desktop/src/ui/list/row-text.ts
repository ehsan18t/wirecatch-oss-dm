// What a download's row says under its name, by the group it is in: what went wrong, how it is going, where it comes from, or where a finished file is.

import type { DownloadSummary } from '@wirecatch/protocol';
import { folderOf, hostOf, statusShort } from '$lib/format';
import type { StatusGroup } from '$lib/state/view';

/** Where it comes from, and over how many connections while it downloads. */
function source(row: DownloadSummary): string {
  if (row.status !== 'downloading') return statusShort(row);
  const host = hostOf(row.url);
  if (row.connections === 0) return host;
  return `${host} · ${String(row.connections)} connection${row.connections === 1 ? '' : 's'}`;
}

/**
 * The line under a row's name. A waiting download says why it waits there only when the list has no progress column to say it in (`waitNote`).
 */
export function rowLine(
  row: DownloadSummary,
  group: StatusGroup,
  waitNote: string,
  progressShown: boolean,
): string {
  switch (group) {
    case 'failed':
      return row.error?.message ?? statusShort(row);
    case 'active':
      return source(row);
    case 'waiting':
      return progressShown ? hostOf(row.url) : waitNote;
    case 'finished':
      return folderOf(row.path);
    case 'cancelled':
      return statusShort(row);
    case 'paused':
      return hostOf(row.url);
  }
}
