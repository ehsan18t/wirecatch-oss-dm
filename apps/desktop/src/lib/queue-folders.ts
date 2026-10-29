// Where a queue saves its downloads (D94): its own folder, or the folders Settings picks by file type.

import type { Queue, QueueId } from '@wirecatch/protocol';
import { displayPath } from './format';

/** What decides the folder while none is picked: words for the Add dialog's Save to (D94.4). */
export function folderFallback(queues: readonly Queue[], queue: QueueId | null): string {
  if (queue === null) {
    return queues.some((q) => q.settings.folder.source === 'own')
      ? 'By queue or file type'
      : 'By file type (from Settings)';
  }
  const own = ownFolder(queues, queue);
  return own ?? 'By file type (from Settings)';
}

/** The folder a download window shows while the person has picked none (D94.4): the proposal's for the queue it proposed, else the picked queue's own folder or the one for the file type. A `type_folder` of null is a folder picked when the link was added, which no queue changes. */
export function folderInQueue(
  queues: readonly Queue[],
  queue: QueueId,
  proposal: { folder: string; queue: QueueId; type_folder: string | null },
): string {
  if (queue === proposal.queue || proposal.type_folder === null) return proposal.folder;
  return ownFolder(queues, queue) ?? proposal.type_folder;
}

function ownFolder(queues: readonly Queue[], queue: QueueId): string | null {
  const folder = queues.find((q) => q.id === queue)?.settings.folder;
  return folder?.source === 'own' ? displayPath(folder.value) : null;
}
