// The short notes of the main window: what an action did, what failed, each download the browser handed over, and how the app started. The design system's toaster shows them (it times them, pauses while they are read, and tells screen readers); this says what each note is about.

import type { DownloadId, DownloadSummary, StartupNotice } from '@wirecatch/protocol';
import { Toaster } from '@wirecatch/ui';
import { SvelteMap } from 'svelte/reactivity';
import { folderOf } from '../format';

export interface Toast {
  id: string;
  kind: 'info' | 'error' | 'capture' | 'notice';
  message: string;
  downloadId?: DownloadId;
}

/** What a note needs from the window it is shown in. */
export interface ToastsHost {
  /** Shows one download's details: a browser download's note offers it. */
  reveal: (id: DownloadId) => void;
}

export class Toasts {
  /** The toaster the window's toast region shows. */
  readonly toaster = new Toaster();
  /** What each note on screen is about; a note the toaster has let go of is forgotten when the next one comes. */
  readonly #kinds = new SvelteMap<string, Omit<Toast, 'id'>>();
  readonly #host: ToastsHost;

  constructor(host: ToastsHost) {
    this.#host = host;
  }

  /** The notes on screen, oldest first. */
  get items(): Toast[] {
    return this.toaster.list.flatMap((t) => {
      const kind = this.#kinds.get(t.id);
      return kind ? [{ id: t.id, ...kind }] : [];
    });
  }

  #record(id: string, toast: Omit<Toast, 'id'>): void {
    for (const key of [...this.#kinds.keys()]) {
      if (!this.toaster.list.some((t) => t.id === key)) this.#kinds.delete(key);
    }
    this.#kinds.set(id, toast);
  }

  notify(text: string): void {
    this.#record(this.toaster.info(text), { kind: 'info', message: text });
  }

  fail(text: string): void {
    this.#record(this.toaster.danger(text), { kind: 'error', message: text });
  }

  /**
   * A download the browser handed over, by its file name; once per download, since a page that was still loading may get it both from the shell and as an event. Its folder can be changed, and the file opened, from the details it offers.
   */
  captured(row: DownloadSummary): void {
    if (this.items.some((t) => t.kind === 'capture' && t.downloadId === row.id)) return;
    const message = `Downloading to ${folderOf(row.path)}`;
    const id = this.toaster.info(message, {
      title: row.file_name,
      action: { label: 'Show details', run: () => this.#host.reveal(row.id) },
    });
    this.#record(id, { kind: 'capture', message, downloadId: row.id });
  }

  /** How the app started, when the person must know (a damaged list set aside): it stays until dismissed, since the system notification may long be gone. A list set aside is offered in its folder, as the path in the message cannot be clicked. */
  notice(notice: StartupNotice, showSetAside: () => void): void {
    const id = this.toaster.warning(notice.message, {
      title: notice.title,
      duration: 0,
      action: notice.set_aside ? { label: 'Show in folder', run: showSetAside } : undefined,
    });
    this.#record(id, { kind: 'notice', message: notice.message });
  }

  dismiss(id: string): void {
    this.toaster.dismiss(id);
  }

  /** Stops every note's timer, for a window that is going away. */
  dispose(): void {
    this.toaster.dispose();
  }
}
