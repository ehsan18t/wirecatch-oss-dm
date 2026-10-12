// The download list, kept so that a progress update costs work proportional to the rows that
// changed, never to the size of the list.
//
// Each row is its own reactive object: a row component reads `row.done`, and only that row
// re-renders when its bytes move. Anything that changes which rows are visible or their order
// (a new row, a status or queue change, a removal, a reorder) bumps `version`, and only then is
// the filtered list rebuilt.

import type { DownloadId, DownloadSummary, QueueId } from '@wirecatch/protocol';
import { track } from './reactivity';
import { QueueDeltas, RowIndex } from './row-index';
import { LIST_FIELDS } from './view';

function reactive<T extends object>(value: T): T {
  const proxied = $state(value);
  return proxied;
}

type RowField = Exclude<keyof DownloadSummary, 'id'>;

/**
 * Every field a row update copies. The mapped type makes this list complete by construction: when the protocol adds a field to `DownloadSummary`, this object stops compiling until the field is listed, so an update can never again drop one (as it once dropped `processing` and `media`).
 */
const ROW_FIELDS: Readonly<Record<RowField, true>> = {
  url: true,
  file_name: true,
  path: true,
  queue: true,
  category: true,
  status: true,
  total: true,
  done: true,
  speed: true,
  connections: true,
  eta_secs: true,
  error: true,
  waiting: true,
  media: true,
  processing: true,
  added_at: true,
  completed_at: true,
};

const FIELDS = Object.keys(ROW_FIELDS) as readonly RowField[];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** Structural equality for the plain data in a row, without allocating (it runs on every progress tick). */
function same(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (!isRecord(a) || !isRecord(b)) return false;
  for (const key in a) if (!same(a[key], b[key])) return false;
  for (const key in b) if (!(key in a)) return false;
  return true;
}

/** Writes one field only when it changed, so untouched fields do not wake anything up. Returns whether it changed. */
function update<K extends RowField>(
  row: Pick<DownloadSummary, K>,
  next: Pick<DownloadSummary, K>,
  field: K,
): boolean {
  if (same(row[field], next[field])) return false;
  row[field] = next[field];
  return true;
}

export class DownloadStore {
  readonly #rows = new RowIndex();

  /** Bumped when rows are added, removed or reordered, or a field used for filtering changes. */
  version = $state(0);
  /** Combined speed of every download, updated by difference on each batch. */
  totalSpeed = $state(0);
  /** Combined speed per queue, updated the same way. */
  queueSpeed: Record<QueueId, number> = $state({});

  /** How many rows each batch touched; tests use this to prove updates stay proportional. */
  lastBatchTouched = 0;

  /** How many rows there are. Reading this subscribes to `version`. */
  get size(): number {
    track(this.version);
    return this.#rows.size;
  }

  /**
   * The reactive row for an id. Reading this subscribes to `version`, so a reader learns when the
   * row comes or goes; reading the row's fields subscribes to that row only.
   */
  get(id: DownloadId): DownloadSummary | undefined {
    track(this.version);
    return this.#rows.get(id);
  }

  /** All ids in queue order. Reading this subscribes to `version`. */
  get order(): readonly DownloadId[] {
    track(this.version);
    return this.#rows.order;
  }

  /** Replaces everything, for the first load. */
  load(list: readonly DownloadSummary[]): void {
    this.#rows.clear();
    let total = 0;
    const perQueue: Record<QueueId, number> = {};
    for (const s of list) {
      if (this.#rows.get(s.id)) continue;
      this.#rows.add(reactive({ ...s }));
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
    const queueDelta = new QueueDeltas();
    for (const s of changed) {
      const row = this.#rows.get(s.id);
      if (!row) {
        this.#rows.add(reactive({ ...s }));
        speedDelta += s.speed;
        queueDelta.add(s.queue, s.speed);
        structural = true;
        continue;
      }
      if (LIST_FIELDS.some((field) => !same(row[field], s[field]))) structural = true;
      speedDelta += s.speed - row.speed;
      if (row.queue !== s.queue) {
        queueDelta.add(row.queue, -row.speed);
        queueDelta.add(s.queue, s.speed);
      } else {
        queueDelta.add(s.queue, s.speed - row.speed);
      }
      for (const field of FIELDS) update(row, s, field);
    }
    if (speedDelta !== 0) this.totalSpeed += speedDelta;
    for (const [q, d] of queueDelta.entries()) this.queueSpeed[q] = (this.queueSpeed[q] ?? 0) + d;
    this.lastBatchTouched = changed.length;
    if (structural) this.version += 1;
  }

  /** Applies a `downloads_removed` batch. */
  remove(ids: readonly DownloadId[]): void {
    const removed = this.#rows.remove(ids);
    if (removed.length === 0) return;
    for (const row of removed) {
      this.totalSpeed -= row.speed;
      this.queueSpeed[row.queue] = (this.queueSpeed[row.queue] ?? 0) - row.speed;
    }
    this.version += 1;
  }

  /** Puts the given ids of one queue in this order, the way the core does after `reorder`. */
  reorder(queue: QueueId, idsInOrder: readonly DownloadId[]): void {
    this.#rows.reorder(queue, idsInOrder);
    this.version += 1;
  }

  /** Ids of one queue, in queue order. */
  idsInQueue(queue: QueueId): DownloadId[] {
    return this.order.filter((id) => this.#rows.get(id)?.queue === queue);
  }
}
