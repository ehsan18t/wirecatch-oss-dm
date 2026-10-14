// How a download's status looks wherever it shows: the state of its progress bar, the colour its status is written in, and what screen readers hear for the bar.

import type { DownloadStatus, DownloadSummary } from '@wirecatch/protocol';
import type { ProgressState, Tone } from '@wirecatch/ui';
import { statusShort, wireFraction } from '$lib/format';

/** The bar each status draws; a cancelled download has no file left to show the progress of. */
export const PROGRESS_STATE: Record<DownloadStatus, ProgressState | null> = {
  queued: 'queued',
  connecting: 'active',
  downloading: 'active',
  processing: 'active',
  paused: 'paused',
  completed: 'done',
  failed: 'error',
  cancelled: null,
};

/** The tone of a status: the accent for data that moves, the warning colour for a pause, danger for a failure. */
export const STATUS_TONE: Record<DownloadStatus, Tone> = {
  queued: 'neutral',
  connecting: 'accent',
  downloading: 'accent',
  processing: 'accent',
  paused: 'warning',
  completed: 'success',
  failed: 'danger',
  cancelled: 'neutral',
};

/** Text in a status's tone, readable on every surface the list and the details use. */
export const STATUS_TEXT: Record<DownloadStatus, string> = {
  queued: 'text-ink-muted',
  connecting: 'text-accent',
  downloading: 'text-accent',
  processing: 'text-accent',
  paused: 'text-warning',
  completed: 'text-ink-muted',
  failed: 'text-danger',
  cancelled: 'text-ink-subtle',
};

export interface RowProgress {
  state: ProgressState;
  /** From 0 to 1, or null while how far it is cannot be told. */
  value: number | null;
  /** The bar's value for screen readers: the status, which says the percentage where there is one. */
  valueText: string;
}

/** The progress bar of a download, or null where there is none to draw. */
export function rowProgress(
  row: Pick<DownloadSummary, 'status' | 'processing' | 'done' | 'total' | 'waiting'>,
): RowProgress | null {
  const state = PROGRESS_STATE[row.status];
  if (state === null) return null;
  return { state, value: wireFraction(row), valueText: statusShort(row) };
}
