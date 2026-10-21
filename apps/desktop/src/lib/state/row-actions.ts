// Which actions apply to a download, decided once: the list's hover buttons, its context menu and the details panel all ask here, so no place offers what another refuses.

import type { DownloadSummary, QueueId } from '@wirecatch/protocol';
import { isUnderWay } from './view';

type Row = Pick<DownloadSummary, 'status'>;

/** Running, or waiting in its queue to run. */
export const canPause = (row: Row): boolean => isUnderWay(row.status);

export const canResume = (row: Row): boolean => row.status === 'paused';

/** A failed download tries again from where it stopped; a cancelled one starts over, since its partial file is gone. */
export const canRetry = (row: Row): boolean =>
  row.status === 'failed' || row.status === 'cancelled';

export const canOpen = (row: Row): boolean => row.status === 'completed';

/** Cancelling discards the partial file, so it is for a download that still has one. */
export const canCancel = (row: Row): boolean =>
  row.status !== 'completed' && row.status !== 'cancelled';

/** A fresh link stands in for an expired one while bytes are still to come from it. */
export const canRefreshLink = (row: Row): boolean =>
  canCancel(row) && row.status !== 'failed' && row.status !== 'processing';

/** A new link is offered for what still downloads from its link, and for a failure whose fix is one. */
export const offersNewLink = (row: Pick<DownloadSummary, 'status' | 'error'>): boolean =>
  canRefreshLink(row) || row.error?.fix === 'refresh_link';

/** Moving to `queue` is offered while some of the downloads are in another queue. */
export const canMoveTo = (
  rows: readonly Pick<DownloadSummary, 'queue'>[],
  queue: QueueId,
): boolean => rows.some((r) => r.queue !== queue);

/** The link may be why a download is stuck: it is paused (a link often expires while it waits), or the server turned it away. Only then do the details ask whether it expired; the action stays in the context menu either way. */
export const linkMayHaveExpired = (row: Pick<DownloadSummary, 'status' | 'waiting'>): boolean =>
  canRefreshLink(row) && (row.status === 'paused' || row.waiting?.type === 'server_refused');

/** Where the file goes can change until it is finished. */
export const canChangeFolder = (row: Row): boolean => row.status !== 'completed';

/** What a selection offers: an action applies when at least one of its downloads takes it. */
export interface Applicable {
  pause: boolean;
  resume: boolean;
  retry: boolean;
  /** Opening is for one finished file. */
  open: boolean;
  cancel: boolean;
}

export function applicable(rows: readonly Row[]): Applicable {
  const only = rows.length === 1 ? rows[0] : undefined;
  return {
    pause: rows.some(canPause),
    resume: rows.some(canResume),
    retry: rows.some(canRetry),
    open: only !== undefined && canOpen(only),
    cancel: rows.some(canCancel),
  };
}
