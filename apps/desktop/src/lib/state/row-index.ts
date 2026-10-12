// The download rows by id and in queue order, and the per-queue sums of one batch: plain
// bookkeeping on purpose. DownloadStore makes each row its own reactive object and bumps its
// `version` when the rows or their order change, which keeps a progress update proportional to
// the rows it touched; reactive collections here would wake every reader of the list instead.

import type { DownloadId, DownloadSummary, QueueId } from '@wirecatch/protocol';

export class RowIndex {
  readonly #rows = new Map<DownloadId, DownloadSummary>();
  #order: DownloadId[] = [];

  get size(): number {
    return this.#rows.size;
  }

  get(id: DownloadId): DownloadSummary | undefined {
    return this.#rows.get(id);
  }

  /** Every id in queue order. */
  get order(): readonly DownloadId[] {
    return this.#order;
  }

  clear(): void {
    this.#rows.clear();
    this.#order = [];
  }

  /** Adds a row at the end of the order; false, and nothing changes, when its id is there already. */
  add(row: DownloadSummary): boolean {
    if (this.#rows.has(row.id)) return false;
    this.#rows.set(row.id, row);
    this.#order.push(row.id);
    return true;
  }

  /** Removes the rows with these ids and returns the ones that were there. */
  remove(ids: readonly DownloadId[]): DownloadSummary[] {
    const removed: DownloadSummary[] = [];
    for (const id of ids) {
      const row = this.#rows.get(id);
      if (!row) continue;
      this.#rows.delete(id);
      removed.push(row);
    }
    if (removed.length > 0) this.#order = this.#order.filter((id) => this.#rows.has(id));
    return removed;
  }

  /** Puts the given ids of one queue in this order; the queue's other ids follow in theirs. */
  reorder(queue: QueueId, idsInOrder: readonly DownloadId[]): void {
    const inQueue = this.#order.filter((id) => this.#rows.get(id)?.queue === queue);
    const wanted = idsInOrder.filter((id) => inQueue.includes(id));
    const wantedSet = new Set(wanted);
    const next = [...wanted, ...inQueue.filter((id) => !wantedSet.has(id))];
    let i = 0;
    this.#order = this.#order.map((id) =>
      this.#rows.get(id)?.queue === queue ? (next[i++] ?? id) : id,
    );
  }
}

/** Speed changes summed per queue, so each queue's total is written once per batch. */
export class QueueDeltas {
  readonly #deltas = new Map<QueueId, number>();

  add(queue: QueueId, delta: number): void {
    if (delta !== 0) this.#deltas.set(queue, (this.#deltas.get(queue) ?? 0) + delta);
  }

  entries(): IterableIterator<[QueueId, number]> {
    return this.#deltas.entries();
  }
}
