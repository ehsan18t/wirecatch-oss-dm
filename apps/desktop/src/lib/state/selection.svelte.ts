// Which downloads are selected, and the row the keyboard is on. Ranges and arrow keys follow
// the order the list shows.

import type { DownloadId, DownloadSummary } from '@wirecatch/protocol';
import { SvelteSet } from 'svelte/reactivity';

export class Selection {
  readonly ids = new SvelteSet<DownloadId>();
  /** The row the keyboard is on. */
  active = $state<DownloadId | null>(null);
  /** Where a Shift range starts: the row last clicked or chosen. */
  #anchor: DownloadId | null = null;
  readonly #visible: () => readonly DownloadId[];
  readonly #row: (id: DownloadId) => DownloadSummary | undefined;

  /** `visible` is the list's order; `row` finds a download. */
  constructor(
    visible: () => readonly DownloadId[],
    row: (id: DownloadId) => DownloadSummary | undefined,
  ) {
    this.#visible = visible;
    this.#row = row;
  }

  /** The selected downloads that are still in the list. */
  rows = $derived.by(() => {
    const rows: DownloadSummary[] = [];
    for (const id of this.ids) {
      const row = this.#row(id);
      if (row) rows.push(row);
    }
    return rows;
  });

  get size(): number {
    return this.ids.size;
  }

  has(id: DownloadId): boolean {
    return this.ids.has(id);
  }

  /** The selected ids, in the order they were selected. */
  get list(): DownloadId[] {
    return [...this.ids];
  }

  only(id: DownloadId): void {
    this.ids.clear();
    this.ids.add(id);
    this.#anchor = id;
    this.active = id;
  }

  toggle(id: DownloadId): void {
    if (this.ids.has(id)) this.ids.delete(id);
    else this.ids.add(id);
    this.#anchor = id;
    this.active = id;
  }

  /** Selects from the last clicked row to this one, in display order. */
  range(id: DownloadId, additive = false): void {
    const list = this.#visible();
    const from = this.#anchor === null ? -1 : list.indexOf(this.#anchor);
    const to = list.indexOf(id);
    if (from < 0 || to < 0) {
      this.only(id);
      return;
    }
    if (!additive) this.ids.clear();
    const [a, b] = from < to ? [from, to] : [to, from];
    for (let i = a; i <= b; i++) {
      const x = list[i];
      if (x !== undefined) this.ids.add(x);
    }
    this.active = id;
  }

  /** Selects every download the list shows. */
  all(): void {
    for (const id of this.#visible()) this.ids.add(id);
  }

  clear(): void {
    this.ids.clear();
    this.#anchor = null;
    this.active = null;
  }

  /** Arrow keys: move the keyboard row, optionally extending the selection. */
  move(delta: number, extend: boolean): void {
    const list = this.#visible();
    if (list.length === 0) return;
    const current = this.active === null ? -1 : list.indexOf(this.active);
    const next =
      current < 0
        ? delta > 0
          ? 0
          : list.length - 1
        : Math.min(list.length - 1, Math.max(0, current + delta));
    const id = list[next];
    if (id === undefined) return;
    if (extend) this.range(id);
    else this.only(id);
  }

  /** Downloads that left the list are no longer selected, nor the keyboard row. */
  forget(ids: readonly DownloadId[]): void {
    for (const id of ids) this.ids.delete(id);
    if (this.active !== null && ids.includes(this.active)) this.active = null;
  }
}
