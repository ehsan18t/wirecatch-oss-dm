// What the main list shows: one sidebar view, an optional search and a sort order.

import type {
  Category,
  DownloadId,
  DownloadStatus,
  DownloadSummary,
  QueueId,
} from '@wirecatch/protocol';

export type StatusGroup = 'active' | 'waiting' | 'paused' | 'finished' | 'failed' | 'cancelled';

/**
 * The fields of a row that `visibleIds` and `countRows` read. A change to one can change which rows are shown, their order or the sidebar counts, so the download list is rebuilt when one changes. A test checks this list against what they actually read.
 */
export const LIST_FIELDS = [
  'status',
  'queue',
  'category',
  'file_name',
  'url',
  'total',
  'added_at',
] as const satisfies readonly (keyof DownloadSummary)[];

export type View =
  | { kind: 'all' }
  | { kind: 'status'; group: StatusGroup }
  | { kind: 'queue'; id: QueueId }
  | { kind: 'category'; category: Category };

export type SortKey = 'queue' | 'name' | 'size' | 'newest';

export const SORT_LABEL: Record<SortKey, string> = {
  queue: 'Queue order',
  name: 'Name',
  size: 'Size',
  newest: 'Newest first',
};

export const STATUS_GROUPS: readonly StatusGroup[] = [
  'active',
  'waiting',
  'paused',
  'finished',
  'failed',
  'cancelled',
];

export const STATUS_GROUP_LABEL: Record<StatusGroup, string> = {
  active: 'Downloading',
  waiting: 'Waiting',
  paused: 'Paused',
  finished: 'Finished',
  failed: 'Needs attention',
  cancelled: 'Cancelled',
};

export function groupOf(status: DownloadStatus): StatusGroup {
  switch (status) {
    case 'connecting':
    case 'downloading':
    case 'processing':
      return 'active';
    case 'queued':
      return 'waiting';
    case 'paused':
      return 'paused';
    case 'completed':
      return 'finished';
    case 'failed':
      return 'failed';
    case 'cancelled':
      return 'cancelled';
  }
}

export function sameView(a: View, b: View): boolean {
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case 'all':
      return true;
    case 'status':
      return b.kind === 'status' && a.group === b.group;
    case 'queue':
      return b.kind === 'queue' && a.id === b.id;
    case 'category':
      return b.kind === 'category' && a.category === b.category;
  }
}

function matches(row: DownloadSummary, view: View): boolean {
  switch (view.kind) {
    case 'all':
      return true;
    case 'status':
      return groupOf(row.status) === view.group;
    case 'queue':
      return row.queue === view.id;
    case 'category':
      return row.category === view.category;
  }
}

/** The ids the list shows, in display order. */
export function visibleIds(
  order: readonly DownloadId[],
  get: (id: DownloadId) => DownloadSummary | undefined,
  view: View,
  search: string,
  sort: SortKey,
): DownloadId[] {
  const needle = search.trim().toLowerCase();
  const rows: DownloadSummary[] = [];
  for (const id of order) {
    const row = get(id);
    if (!row || !matches(row, view)) continue;
    if (
      needle &&
      !row.file_name.toLowerCase().includes(needle) &&
      !row.url.toLowerCase().includes(needle)
    ) {
      continue;
    }
    rows.push(row);
  }
  switch (sort) {
    case 'name':
      rows.sort((a, b) => a.file_name.localeCompare(b.file_name, undefined, { numeric: true }));
      break;
    case 'size':
      rows.sort((a, b) => (b.total ?? -1) - (a.total ?? -1));
      break;
    case 'newest':
      rows.sort((a, b) => b.added_at - a.added_at);
      break;
    case 'queue':
      break;
  }
  return rows.map((r) => r.id);
}

export interface Counts {
  all: number;
  /** Of the active ones, how many are joining video and audio rather than downloading. */
  joining: number;
  status: Record<StatusGroup, number>;
  category: Record<Category, number>;
  queue: Map<QueueId, { total: number; running: number; waiting: number; paused: number }>;
}

/** Counts for the sidebar badges. */
export function countRows(
  order: readonly DownloadId[],
  get: (id: DownloadId) => DownloadSummary | undefined,
): Counts {
  const counts: Counts = {
    all: 0,
    joining: 0,
    status: { active: 0, waiting: 0, paused: 0, finished: 0, failed: 0, cancelled: 0 },
    category: { video: 0, audio: 0, archive: 0, document: 0, program: 0, image: 0, other: 0 },
    queue: new Map(),
  };
  for (const id of order) {
    const row = get(id);
    if (!row) continue;
    counts.all += 1;
    if (row.status === 'processing') counts.joining += 1;
    const group = groupOf(row.status);
    counts.status[group] += 1;
    counts.category[row.category] += 1;
    let q = counts.queue.get(row.queue);
    if (!q) {
      q = { total: 0, running: 0, waiting: 0, paused: 0 };
      counts.queue.set(row.queue, q);
    }
    if (group !== 'finished' && group !== 'cancelled') q.total += 1;
    if (group === 'active') q.running += 1;
    if (group === 'waiting') q.waiting += 1;
    if (group === 'paused') q.paused += 1;
  }
  return counts;
}
