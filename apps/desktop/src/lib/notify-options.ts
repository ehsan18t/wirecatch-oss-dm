// How and when the person is told that downloads started, finished or failed (D52, D65), in the
// words Settings, the queue dialog and the download complete window share.

import {
  FINISH_NOTICE_TIMES,
  RETRY_FIXES,
  START_NOTICE_TIMES,
  type DownloadSummary,
  type NotifyOnFinish,
  type FinishNoticeTime,
  type QueueNotify,
  type NotifyOnStart,
  type StartNoticeTime,
} from '@wirecatch/protocol';
import type { SelectOption } from '@wirecatch/ui';

/** How finished and failed downloads are told, for the setting's dropdown. */
export const NOTIFY_ON_FINISH_OPTIONS: readonly SelectOption<NotifyOnFinish>[] = [
  {
    value: 'browser',
    label: 'Show a notice in the browser',
    description:
      'In the current tab, with the same buttons. A system notification appears instead when the browser is in the background.',
  },
  {
    value: 'notification',
    label: 'Show a notification',
    description: 'With buttons to open the file or show it in its folder.',
  },
  {
    value: 'window',
    label: 'Open a summary window',
    description: 'A small window in the corner that lists finished downloads until you close it.',
  },
  {
    value: 'nothing',
    label: 'Don’t notify',
    description: 'Not even for failed downloads. The download list still shows how each one ended.',
  },
];

/** How a download that started without a window is told, for the setting's dropdown. */
export const NOTIFY_ON_START_OPTIONS: readonly SelectOption<NotifyOnStart>[] = [
  {
    value: 'browser',
    label: 'Show a notice in the browser',
    description:
      'In the current tab, or a system notification when the browser is in the background.',
  },
  { value: 'nothing', label: 'Don’t notify', description: 'The download list shows it.' },
];

/** How long a note about a download that started stays, from the protocol's seconds (`START_NOTICE_TIMES`). */
export const START_NOTICE_TIME_OPTIONS: readonly SelectOption<StartNoticeTime>[] =
  START_NOTICE_TIMES.map(([value, seconds]) => ({ value, label: `${String(seconds)} seconds` }));

/** How long a note about finished downloads stays, from the protocol's seconds (`FINISH_NOTICE_TIMES`). */
export const FINISH_NOTICE_TIME_OPTIONS: readonly SelectOption<FinishNoticeTime>[] =
  FINISH_NOTICE_TIMES.map(([value, seconds]) => ({
    value,
    label: seconds === null ? 'Until I close it' : `${String(seconds)} seconds`,
  }));

/** When a queue tells you, for the queue dialog. */
export const QUEUE_NOTIFY_OPTIONS: readonly SelectOption<QueueNotify>[] = [
  { value: 'each_download', label: 'Each download' },
  { value: 'when_done', label: 'When the queue is done' },
];

/** What a queue's choice of when to notify means, the same whichever is chosen (D84.1). */
export const QUEUE_NOTIFY_HELP =
  'Each download groups those that finish a few seconds apart. When the queue is done notifies once nothing in it is downloading or waiting; failed downloads still notify at once.';

/** Why the queue's choice does nothing yet; `undefined` before the settings load. */
export function queueNotifyWarning(notifyOnFinish: NotifyOnFinish | undefined): string | null {
  return notifyOnFinish === 'nothing'
    ? 'Finish notifications are off in Settings > Notifications.'
    : null;
}

/** What a queue's choice means, in a sentence about the queue `name`. */
export function queueNotifyLine(name: string, notify: QueueNotify): string {
  return notify === 'each_download'
    ? `The ${name} queue notifies you each time a download finishes.`
    : `The ${name} queue notifies you once, when all its downloads are done.`;
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
