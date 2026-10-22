// How and when the person is told that downloads finished or failed (D52), in the words Settings,
// the queue dialog and the download complete window share.

import {
  RETRY_FIXES,
  type DownloadSummary,
  type FinishNotice,
  type QueueNotify,
} from '@wirecatch/protocol';
import type { SelectOption } from '@wirecatch/ui';

/** How finished and failed downloads are told, for the setting's dropdown. */
export const FINISH_NOTICE_OPTIONS: readonly SelectOption<FinishNotice>[] = [
  {
    value: 'notification',
    label: 'Show a notification',
    description: 'With buttons to open the file or show it in its folder.',
  },
  {
    value: 'window',
    label: 'Open a window',
    description: 'A small window in the corner that collects what finished until you close it.',
  },
  {
    value: 'nothing',
    label: 'Do nothing',
    description: 'Not even for failures. The download list shows how each download ended.',
  },
];

/** When a queue tells you, for the queue dialog. */
export const QUEUE_NOTIFY_OPTIONS: readonly SelectOption<QueueNotify>[] = [
  { value: 'each_download', label: 'Each download' },
  { value: 'when_done', label: 'When the queue is done' },
];

/** What a queue's choice means, in a sentence about the queue `name`. */
export function queueNotifyLine(name: string, notify: QueueNotify): string {
  return notify === 'each_download'
    ? `${name} tells you each time a download finishes.`
    : `${name} tells you once, when all its downloads are done.`;
}

/** "1 download" or "3 downloads". */
function downloads(count: number): string {
  return count === 1 ? '1 download' : `${String(count)} downloads`;
}

/** What `completed` finished and `failed` failed downloads come to, as a heading. */
export function finishedTitle(completed: number, failed: number): string {
  if (completed === 0 && failed === 0) return 'Your downloads';
  if (failed === 0)
    return completed === 1 ? 'Download finished' : `${downloads(completed)} finished`;
  if (completed === 0) return failed === 1 ? 'Download failed' : `${downloads(failed)} failed`;
  return `${downloads(completed)} finished and ${String(failed)} failed`;
}

/** Whether trying a failed download again can fix it from a notice (`RETRY_FIXES`); the other fixes need the download list. */
export function retryHelps(row: DownloadSummary): boolean {
  const fix = row.error?.fix ?? null;
  return fix === null || RETRY_FIXES.includes(fix);
}
