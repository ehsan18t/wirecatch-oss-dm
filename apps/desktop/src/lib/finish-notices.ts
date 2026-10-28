// How and when the person is told that downloads started, finished or failed (D52, D65), in the
// words Settings, the queue dialog and the download complete window share.

import {
  FINISH_NOTE_TIMES,
  RETRY_FIXES,
  START_NOTE_TIMES,
  type DownloadSummary,
  type FinishNotice,
  type FinishNoteTime,
  type QueueNotify,
  type StartNotice,
  type StartNoteTime,
} from '@wirecatch/protocol';
import type { SelectOption } from '@wirecatch/ui';

/** How finished and failed downloads are told, for the setting's dropdown. */
export const FINISH_NOTICE_OPTIONS: readonly SelectOption<FinishNotice>[] = [
  {
    value: 'browser',
    label: 'Show a note in the browser',
    description:
      'In the tab you are looking at, with the same buttons. A notification when the browser is not in front.',
  },
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

/** How a download that started without a window is told, for the setting's dropdown. */
export const START_NOTICE_OPTIONS: readonly SelectOption<StartNotice>[] = [
  {
    value: 'browser',
    label: 'Show a note in the browser',
    description: 'In the tab you are looking at. A notification when the browser is not in front.',
  },
  { value: 'nothing', label: 'Do nothing', description: 'The download list shows it.' },
];

/** How long a note about a download that started stays, from the protocol's seconds (`START_NOTE_TIMES`). */
export const START_NOTE_TIME_OPTIONS: readonly SelectOption<StartNoteTime>[] = START_NOTE_TIMES.map(
  ([value, seconds]) => ({ value, label: `${String(seconds)} seconds` }),
);

/** How long a note about finished downloads stays, from the protocol's seconds (`FINISH_NOTE_TIMES`). */
export const FINISH_NOTE_TIME_OPTIONS: readonly SelectOption<FinishNoteTime>[] =
  FINISH_NOTE_TIMES.map(([value, seconds]) => ({
    value,
    label: seconds === null ? 'Until I close it' : `${String(seconds)} seconds`,
  }));

/** When a queue tells you, for the queue dialog. */
export const QUEUE_NOTIFY_OPTIONS: readonly SelectOption<QueueNotify>[] = [
  { value: 'each_download', label: 'Each download' },
  { value: 'when_done', label: 'When the queue is done' },
];

/** What a queue's choice of when to tell does, given how Settings says to tell (`undefined` before the settings load). */
export function queueNotifyHelp(
  notify: QueueNotify,
  finishNotice: FinishNotice | undefined,
): string {
  if (finishNotice === 'nothing')
    return 'Settings, Notifications, says to tell you nothing, so this applies once you change that.';
  return notify === 'each_download'
    ? 'Downloads that finish a few seconds apart are grouped together.'
    : 'Tells you once, when nothing in it downloads or waits to start. You still hear about a failed download right away.';
}

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
