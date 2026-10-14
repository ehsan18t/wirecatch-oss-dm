// The short notes at the bottom of the main window: what an action did, what failed, and each
// download the browser handed over.

import type { DownloadId, DownloadSummary } from '@wirecatch/protocol';

export interface Toast {
  id: number;
  kind: 'info' | 'error' | 'capture';
  message: string;
  downloadId?: DownloadId;
}

/** Notes shown at once; a new one pushes out the oldest, so the stack stays short. */
const TOASTS_MAX = 4;

export class Toasts {
  items = $state<Toast[]>([]);
  #nextId = 1;

  #show(t: Omit<Toast, 'id'>): void {
    const id = this.#nextId++;
    this.items = [...this.items, { ...t, id }].slice(-TOASTS_MAX);
  }

  notify(text: string): void {
    this.#show({ kind: 'info', message: text });
  }

  fail(text: string): void {
    this.#show({ kind: 'error', message: text });
  }

  /**
   * A download the browser handed over, by its file name; once per download, since a page that
   * was still loading may get it both from the shell and as an event.
   */
  captured(row: DownloadSummary): void {
    if (this.items.some((t) => t.kind === 'capture' && t.downloadId === row.id)) return;
    this.#show({ kind: 'capture', message: row.file_name, downloadId: row.id });
  }

  dismiss(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
  }
}
