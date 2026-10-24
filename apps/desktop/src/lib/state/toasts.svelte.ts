// The short notes of the main window: what an action did, what failed, and how the app started. The design system's toaster shows them (it times them, pauses while they are read, and tells screen readers); this says what each note is about.

import type { StartupNotice } from '@wirecatch/protocol';
import { Toaster } from '@wirecatch/ui';
import { SvelteMap } from 'svelte/reactivity';

export interface Toast {
  id: string;
  kind: 'info' | 'error' | 'notice';
  message: string;
}

export class Toasts {
  /** The toaster the window's toast region shows. */
  readonly toaster = new Toaster();
  /** What each note on screen is about; a note the toaster has let go of is forgotten when the next one comes. */
  readonly #kinds = new SvelteMap<string, Omit<Toast, 'id'>>();

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
