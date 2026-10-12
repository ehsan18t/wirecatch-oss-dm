// The download windows of the sample backend (D23), shown inside the page because a browser tab
// cannot open real windows that share the sample data. It keeps the same rules as the desktop
// (`src-tauri/src/download_windows.rs`): at most MAX_QUESTION_WINDOWS ask at once and the rest wait,
// each window takes the lowest free place in the cascade, and a question whose window the person
// closed does not open again by itself.

import { MAX_QUESTION_WINDOWS, type DownloadId } from '@wirecatch/protocol';

/** What a download window shows. */
export type Shows = { kind: 'question'; id: number } | { kind: 'download'; id: DownloadId };

export interface OpenWindow {
  /** Stable for the window's life, like a Tauri label. */
  label: string;
  shows: Shows;
  /** Its place in the cascade. */
  slot: number;
}

function labelFor(shows: Shows): string {
  return shows.kind === 'question' ? `download-q${shows.id}` : `download-d${shows.id}`;
}

function same(a: Shows, b: Shows): boolean {
  return a.kind === b.kind && a.id === b.id;
}

export class WindowRegistry {
  #windows: OpenWindow[] = [];
  #waiting: number[] = [];
  readonly #putAside = new Set<number>();
  readonly #listeners = new Set<(windows: readonly OpenWindow[]) => void>();
  /** The label of the window brought to the front last. */
  front: string | null = null;

  get windows(): readonly OpenWindow[] {
    return this.#windows;
  }

  get waiting(): readonly number[] {
    return this.#waiting;
  }

  /** Called with the open windows whenever they change. */
  subscribe(listener: (windows: readonly OpenWindow[]) => void): () => void {
    this.#listeners.add(listener);
    listener(this.#windows);
    return () => this.#listeners.delete(listener);
  }

  #changed(): void {
    this.#windows = [...this.#windows];
    for (const listener of this.#listeners) listener(this.#windows);
  }

  #asking(): number {
    return this.#windows.filter((w) => w.shows.kind === 'question').length;
  }

  #freeSlot(): number {
    let slot = 0;
    while (this.#windows.some((w) => w.slot === slot)) slot += 1;
    return slot;
  }

  #find(shows: Shows): OpenWindow | undefined {
    return this.#windows.find((w) => same(w.shows, shows));
  }

  #place(id: number): boolean {
    const shows: Shows = { kind: 'question', id };
    if (this.#find(shows) || this.#waiting.includes(id)) return false;
    if (this.#asking() >= MAX_QUESTION_WINDOWS) {
      this.#waiting.push(id);
      return false;
    }
    const label = labelFor(shows);
    this.#windows.push({ label, shows, slot: this.#freeSlot() });
    this.front = label;
    return true;
  }

  #fill(): void {
    while (this.#asking() < MAX_QUESTION_WINDOWS) {
      const id = this.#waiting.shift();
      if (id === undefined) break;
      this.#place(id);
    }
  }

  /** The core asked question `id`, or moved it on. */
  ask(id: number): void {
    this.#putAside.delete(id);
    if (this.#place(id)) this.#changed();
  }

  /** Question `id` closed in the core. Its window, if any, says so itself. */
  closed(id: number): void {
    this.#waiting = this.#waiting.filter((w) => w !== id);
    this.#putAside.delete(id);
  }

  /** The person asked to see every open question (`open`, oldest first). */
  showAll(open: readonly number[]): void {
    this.#waiting = this.#waiting.filter((id) => open.includes(id));
    for (const id of open) {
      this.#putAside.delete(id);
      const window = this.#find({ kind: 'question', id });
      if (window) this.front = window.label;
      else this.#place(id);
    }
    this.#changed();
  }

  /** The window `label` now shows download `id`. */
  becameDownload(label: string, id: DownloadId): void {
    const window = this.#windows.find((w) => w.label === label);
    if (window) window.shows = { kind: 'download', id };
    this.#fill();
    this.#changed();
  }

  /** The window `label` closed. A question it showed was put aside. */
  gone(label: string): void {
    const window = this.#windows.find((w) => w.label === label);
    if (!window) return;
    this.#windows = this.#windows.filter((w) => w !== window);
    if (window.shows.kind === 'question') this.#putAside.add(window.shows.id);
    this.#fill();
    this.#changed();
  }

  /** Opens, or brings forward, the progress window of download `id`. */
  showDownload(id: DownloadId): void {
    const shows: Shows = { kind: 'download', id };
    const window = this.#find(shows);
    if (window) {
      this.front = window.label;
    } else {
      const label = labelFor(shows);
      this.#windows.push({ label, shows, slot: this.#freeSlot() });
      this.front = label;
    }
    this.#changed();
  }

  /** Brings a window to the front. */
  focus(label: string): void {
    this.front = label;
    this.#changed();
  }
}
