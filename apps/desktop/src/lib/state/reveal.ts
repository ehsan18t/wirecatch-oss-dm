// What the main window does when it comes to the front for someone: for a notification about
// downloads the person clicked (D52), it shows them selected, the first in view, in their queue
// for a queue's summary; for the extension's settings button (D61), the browser settings; for a
// progress window's fix of a proxy error (D71), the proxy settings, or a download's details for
// its Change queue (D73.4).

import type { DownloadId, DownloadSummary, Queue, QueueId, Reveal } from '@wirecatch/protocol';
import type { SvelteSet } from 'svelte/reactivity';
import type { SectionId } from '../settings-sections';
import type { Selection } from './selection.svelte';
import { groupOf, isGrouped, type StatusGroup, type View } from './view';

/** What showing downloads or the browser settings needs from the main window. */
export interface RevealTarget {
  readonly downloads: { get: (id: DownloadId) => DownloadSummary | undefined };
  readonly queues: { byId: (id: QueueId) => Queue | undefined };
  readonly view: View;
  readonly folded: SvelteSet<StatusGroup>;
  readonly selection: Selection;
  show: (view: View) => void;
  openSettingsSection: (section: SectionId) => void;
  /** Opens a download's details, where its queue is chosen. */
  openDetails: (id: DownloadId) => void;
}

/** Shows the browser or proxy settings, or the downloads `reveal` names that are still in the list; none left changes nothing. */
export function revealDownloads(app: RevealTarget, reveal: Reveal): void {
  if (reveal.browser_settings === true) {
    app.openSettingsSection('browser');
    return;
  }
  if (reveal.proxy_settings === true) {
    app.openSettingsSection('network');
    return;
  }
  const rows = reveal.ids.flatMap((id) => app.downloads.get(id) ?? []);
  if (rows.length === 0) return;
  const queue = reveal.queue === null ? undefined : app.queues.byId(reveal.queue);
  app.show(queue ? { kind: 'queue', id: queue.id } : { kind: 'all' });
  if (isGrouped(app.view)) for (const row of rows) app.folded.delete(groupOf(row.status));
  app.selection.select(rows.map((row) => row.id));
  const first = rows[0];
  if (reveal.details === true && first) app.openDetails(first.id);
}
