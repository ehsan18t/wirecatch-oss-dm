// The main window's screens and dialogs, as data: which one is open and what it was opened for.

import type { DownloadId, QueueId } from '@wirecatch/protocol';

export type Screen = 'downloads' | 'settings';

export type Dialog =
  | { kind: 'add'; urls: string[]; queue?: QueueId }
  | { kind: 'remove'; ids: DownloadId[] }
  | { kind: 'cancel'; ids: DownloadId[] }
  | { kind: 'queue'; id: QueueId | null }
  | { kind: 'refresh'; id: DownloadId }
  | { kind: 'export' }
  | { kind: 'import' }
  | { kind: 'shortcuts' }
  | { kind: 'open-program'; id: DownloadId }
  | { kind: 'remove-addon'; id: string };
