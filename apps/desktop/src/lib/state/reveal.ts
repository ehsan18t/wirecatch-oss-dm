// What the main window does when the person clicks a notification about downloads (D52): it
// shows them selected, the first in view, in their queue for a queue's summary.

import type { DownloadId, DownloadSummary, Queue, QueueId, Reveal } from '@wirecatch/protocol';
import type { SvelteSet } from 'svelte/reactivity';
import type { Selection } from './selection.svelte';
import { groupOf, isGrouped, type StatusGroup, type View } from './view';

/** What showing downloads needs from the main window. */
export interface RevealTarget {
  readonly downloads: { get(id: DownloadId): DownloadSummary | undefined };
  readonly queues: { byId(id: QueueId): Queue | undefined };
  readonly view: View;
  readonly folded: SvelteSet<StatusGroup>;
  readonly selection: Selection;
  show(view: View): void;
}

/** Shows the downloads `reveal` names that are still in the list; none left changes nothing. */
export function revealDownloads(app: RevealTarget, reveal: Reveal): void {
  const rows = reveal.ids.flatMap((id) => app.downloads.get(id) ?? []);
  if (rows.length === 0) return;
  const queue = reveal.queue === null ? undefined : app.queues.byId(reveal.queue);
  app.show(queue ? { kind: 'queue', id: queue.id } : { kind: 'all' });
  if (isGrouped(app.view)) for (const row of rows) app.folded.delete(groupOf(row.status));
  app.selection.select(rows.map((row) => row.id));
}
