// The main window's screens and dialogs, as data: which one is open and what it was opened for.

import type { AddonId, DownloadId, QueueId } from '@wirecatch/protocol';

export type Screen = 'downloads' | 'settings';

export type Dialog =
  | { kind: 'add'; urls: string[]; queue?: QueueId }
  | { kind: 'remove'; ids: DownloadId[] }
  | { kind: 'cancel'; ids: DownloadId[] }
  /** `deleting` opens it at the question whether to delete the queue. */
  | { kind: 'queue'; id: QueueId | null; deleting?: boolean }
  | { kind: 'refresh'; id: DownloadId }
  | { kind: 'export' }
  | { kind: 'import' }
  | { kind: 'shortcuts' }
  | { kind: 'remove-addon'; id: AddonId };
