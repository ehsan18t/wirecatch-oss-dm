// The download list, kept so that a progress update costs work proportional to the rows that
// changed, never to the size of the list.
//
// Each row is its own reactive object: a row component reads `row.done`, and only that row
// re-renders when its bytes move. Anything that changes which rows are visible or their order
// (a new row, a status or queue change, a removal, a reorder) bumps `version`, and only then is
// the filtered list rebuilt.

/* eslint-disable svelte/prefer-svelte-reactivity --
   The plain Map, Set and array here are deliberate: reactivity comes from `version` and from each
   row being its own reactive object, which is what keeps updates proportional to what changed. */

import type { DownloadId, DownloadSummary, QueueId } from '@wirecatch/protocol';

function reactive<T extends object>(value: T): T {
  const proxied = $state(value);
  return proxied;
}

function sameError(a: DownloadSummary['error'], b: DownloadSummary['error']): boolean {
  if (a === b) return true;
  if (a === null || b === null) return false;
  return a.message === b.message && a.fix === b.fix;
}

export class DownloadStore {
  readonly #rows = new Map<DownloadId, DownloadSummary>();
  #order: DownloadId[] = [];

  /** Bumped when rows are added, removed or reordered, or a field used for filtering changes. */
  version = $state(0);
  /** Combined speed of every download, updated by difference on each batch. */
  totalSpeed = $state(0);
  /** Combined speed per queue, updated the same way. */
  queueSpeed: Record<QueueId, number> = $state({});

  /** How many rows each batch touched; tests use this to prove updates stay proportional. */
  lastBatchTouched = 0;

  get size(): number {
    return this.#rows.size;
  }

  /** The reactive row for an id. Reading its fields in a component subscribes to that row only. */
  get(id: DownloadId): DownloadSummary | undefined {
    return this.#rows.get(id);
  }

  /** All ids in queue order. Reading this subscribes to `version`. */
  get order(): readonly DownloadId[] {
    void this.version;
    return this.#order;
  }

  /** Replaces everything, for the first load. */
  load(list: readonly DownloadSummary[]): void {
    this.#rows.clear();
    this.#order = [];
    let total = 0;
    const perQueue: Record<QueueId, number> = {};
    for (const s of list) {
      if (this.#rows.has(s.id)) continue;
      this.#rows.set(s.id, reactive({ ...s }));
      this.#order.push(s.id);
      total += s.speed;
      perQueue[s.queue] = (perQueue[s.queue] ?? 0) + s.speed;
    }
    this.totalSpeed = total;
    this.queueSpeed = perQueue;
    this.version += 1;
  }

  /** Applies a `downloads_changed` batch. */
  apply(changed: readonly DownloadSummary[]): void {
    let structural = false;
    let speedDelta = 0;
    const queueDelta = new Map<QueueId, number>();
    const addQueue = (q: QueueId, d: number) => {
      if (d !== 0) queueDelta.set(q, (queueDelta.get(q) ?? 0) + d);
    };
    for (const s of changed) {
      const row = this.#rows.get(s.id);
      if (!row) {
        this.#rows.set(s.id, reactive({ ...s }));
        this.#order.push(s.id);
        speedDelta += s.speed;
        addQueue(s.queue, s.speed);
        structural = true;
        continue;
      }
      if (
        row.status !== s.status ||
        row.queue !== s.queue ||
        row.category !== s.category ||
        row.file_name !== s.file_name ||
        row.total !== s.total
      ) {
        structural = true;
      }
      speedDelta += s.speed - row.speed;
      if (row.queue !== s.queue) {
        addQueue(row.queue, -row.speed);
        addQueue(s.queue, s.speed);
      } else {
        addQueue(s.queue, s.speed - row.speed);
      }
      // Write only what changed, so untouched fields do not wake anything up.
      if (row.url !== s.url) row.url = s.url;
      if (row.file_name !== s.file_name) row.file_name = s.file_name;
      if (row.path !== s.path) row.path = s.path;
      if (row.queue !== s.queue) row.queue = s.queue;
      if (row.category !== s.category) row.category = s.category;
      if (row.status !== s.status) row.status = s.status;
      if (row.total !== s.total) row.total = s.total;
      if (row.done !== s.done) row.done = s.done;
      if (row.speed !== s.speed) row.speed = s.speed;
      if (row.connections !== s.connections) row.connections = s.connections;
      if (row.eta_secs !== s.eta_secs) row.eta_secs = s.eta_secs;
      if (!sameError(row.error, s.error)) row.error = s.error === null ? null : { ...s.error };
      if (row.added_at !== s.added_at) row.added_at = s.added_at;
      if (row.completed_at !== s.completed_at) row.completed_at = s.completed_at;
    }
    if (speedDelta !== 0) this.totalSpeed += speedDelta;
    for (const [q, d] of queueDelta) this.queueSpeed[q] = (this.queueSpeed[q] ?? 0) + d;
    this.lastBatchTouched = changed.length;
    if (structural) this.version += 1;
  }

  /** Applies a `downloads_removed` batch. */
  remove(ids: readonly DownloadId[]): void {
    const gone = new Set<DownloadId>();
    for (const id of ids) {
      const row = this.#rows.get(id);
      if (!row) continue;
      this.totalSpeed -= row.speed;
      this.queueSpeed[row.queue] = (this.queueSpeed[row.queue] ?? 0) - row.speed;
      this.#rows.delete(id);
      gone.add(id);
    }
    if (gone.size === 0) return;
    this.#order = this.#order.filter((id) => !gone.has(id));
    this.version += 1;
  }

  /** Puts the given ids of one queue in this order, the way the core does after `reorder`. */
  reorder(queue: QueueId, idsInOrder: readonly DownloadId[]): void {
    const inQueue = this.#order.filter((id) => this.#rows.get(id)?.queue === queue);
    const wanted = idsInOrder.filter((id) => inQueue.includes(id));
    const wantedSet = new Set(wanted);
    const next = [...wanted, ...inQueue.filter((id) => !wantedSet.has(id))];
    let i = 0;
    this.#order = this.#order.map((id) =>
      this.#rows.get(id)?.queue === queue ? (next[i++] ?? id) : id,
    );
    this.version += 1;
  }

  /** Ids of one queue, in queue order. */
  idsInQueue(queue: QueueId): DownloadId[] {
    return this.order.filter((id) => this.#rows.get(id)?.queue === queue);
  }
}
