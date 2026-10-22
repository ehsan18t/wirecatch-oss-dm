// What the main list shows: one sidebar view, an optional search and a sort order, and how its rows fall into groups by state.

import type {
  Category,
  DownloadId,
  DownloadStatus,
  DownloadSummary,
  QueueId,
} from '@wirecatch/protocol';

export type StatusGroup = 'active' | 'waiting' | 'paused' | 'finished' | 'failed' | 'cancelled';

/**
 * The fields of a row that `listLayout`, `placesOf` and `countRows` read under every sort. A change to one can change which rows are shown, their order or the sidebar counts, so the download list is rebuilt when one changes. A test checks this list against what they actually read.
 */
export const LIST_FIELDS = [
  'status',
  'queue',
  'category',
  'file_name',
  'url',
  'total',
  'added_at',
  'completed_at',
] as const satisfies readonly (keyof DownloadSummary)[];

/**
 * The fields only a sort by progress, speed or time left reads. They move on every progress tick, which must not rebuild the list, so such a sort is taken when it is chosen and whenever the list changes: rows never jump under the pointer.
 */
export const LIVE_SORT_FIELDS = [
  'done',
  'speed',
  'eta_secs',
] as const satisfies readonly (keyof DownloadSummary)[];

export type View =
  | { kind: 'all' }
  | { kind: 'status'; group: StatusGroup }
  | { kind: 'queue'; id: QueueId }
  | { kind: 'category'; category: Category };

/** The columns the list sorts by; `position` is each download's place in its queue. */
export type SortColumn = 'position' | 'name' | 'progress' | 'size' | 'speed' | 'left';

export interface Sort {
  column: SortColumn;
  descending: boolean;
}

/** The list's own order: each download where its queue will run it. */
export const QUEUE_ORDER: Sort = { column: 'position', descending: false };

export const SORT_LABEL: Record<SortColumn, string> = {
  position: 'Queue order',
  name: 'Name',
  progress: 'Progress',
  size: 'Size',
  speed: 'Speed',
  left: 'Time left',
};

/** A header clicked: its column ascending, then descending, then back to queue order. */
export function nextSort(current: Sort, column: SortColumn): Sort {
  if (column === 'position' || current.column !== column) return { column, descending: false };
  return current.descending ? QUEUE_ORDER : { column, descending: true };
}

export function sameSort(a: Sort, b: Sort): boolean {
  return a.column === b.column && a.descending === b.descending;
}

/** The sidebar's order of the status views. */
export const STATUS_GROUPS: readonly StatusGroup[] = [
  'active',
  'waiting',
  'paused',
  'finished',
  'failed',
  'cancelled',
];

/** The list's order of its groups: what needs the person first, so a few failures are never buried under many downloads; then what moves, what is next, and what is done. */
export const GROUP_ORDER: readonly StatusGroup[] = [
  'failed',
  'active',
  'waiting',
  'paused',
  'finished',
  'cancelled',
];

/** Groups folded until the person opens them: work that is over. */
export const FOLDED_GROUPS: readonly StatusGroup[] = ['finished', 'cancelled'];

export const STATUS_GROUP_LABEL: Record<StatusGroup, string> = {
  active: 'Downloading',
  waiting: 'Up next',
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

/** On its way: running, or waiting in its queue to run. Pause applies to it. */
export function isUnderWay(status: DownloadStatus): boolean {
  const group = groupOf(status);
  return group === 'active' || group === 'waiting';
}

/** Still in its queue's line, so it has a place there: anything not finished or cancelled. */
export function isInLine(status: DownloadStatus): boolean {
  const group = groupOf(status);
  return group !== 'finished' && group !== 'cancelled';
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

/** A view of one status holds one group, which needs no heading; every other view is grouped. */
export function isGrouped(view: View): boolean {
  return view.kind !== 'status';
}

/** Work that is over: in a queue's own view these keep their groups below its line. */
const OVER: ReadonlySet<StatusGroup> = new Set(['finished', 'cancelled']);

/** The section a row goes in: none in an ungrouped view; in a queue's own view its line, in the order it runs, so its numbers read 1, 2, 3 down the list, with only work that is over in a group; else its state's group. */
function sectionGroup(row: DownloadSummary, grouped: boolean, inLine: boolean): StatusGroup | null {
  const status = groupOf(row.status);
  return !grouped || (inLine && !OVER.has(status)) ? null : status;
}

/** The sections in the order they show. */
function groupOrder(grouped: boolean, inLine: boolean): readonly (StatusGroup | null)[] {
  if (!grouped) return [null];
  return inLine ? [null, ...GROUP_ORDER.filter((g) => OVER.has(g))] : GROUP_ORDER;
}

/** What the list shows and in which order: the person's choices above the list. */
export interface ListOptions {
  view: View;
  /** Text the name or the link must contain; empty shows every row of the view. */
  search: string;
  sort: Sort;
  /** Groups whose rows are hidden under their heading. */
  folded: readonly StatusGroup[];
  /** Every queue, in the sidebar's order: queue order lists one queue's downloads after another's. */
  queues: readonly QueueId[];
}

/** One group of the list: its heading, then its rows from `start` in `ListLayout.ids` (none while folded). */
export interface ListSection {
  /** Null in an ungrouped view, or where only one group has downloads: that one section has no heading. */
  group: StatusGroup | null;
  /** How many downloads the group holds, folded or not. */
  count: number;
  folded: boolean;
  start: number;
  /** The rows drawn: `count`, or none while folded. */
  size: number;
}

export interface ListLayout {
  /** The rows drawn, in display order; the selection and the keyboard move through these. */
  ids: DownloadId[];
  sections: ListSection[];
}

type Compare = (a: DownloadSummary, b: DownloadSummary) => number;

/** Sorts by `value`, rows without one last in either direction. */
function by(value: (row: DownloadSummary) => number | string | null, descending: boolean): Compare {
  return (a, b) => {
    const x = value(a);
    const y = value(b);
    if (x === null || y === null) return x === null ? (y === null ? 0 : 1) : -1;
    const order =
      typeof x === 'string' && typeof y === 'string'
        ? x.localeCompare(y, undefined, { numeric: true })
        : Number(x) - Number(y);
    return descending ? -order : order;
  };
}

const FINISHED_FIRST: Compare = (a, b) =>
  (b.completed_at ?? b.added_at) - (a.completed_at ?? a.added_at);

function compareFor(sort: Sort, group: StatusGroup | null, place: Compare): Compare {
  // Up next always runs in queue order, so it always shows in it; finished work has no place in a queue and shows newest first.
  const column = group === 'waiting' ? 'position' : sort.column;
  const descending = group === 'waiting' ? false : sort.descending;
  switch (column) {
    case 'position':
      return (a, b) => {
        const inLine = Number(isInLine(b.status)) - Number(isInLine(a.status));
        if (inLine !== 0) return inLine;
        const order = isInLine(a.status) ? place(a, b) : FINISHED_FIRST(a, b);
        return descending ? -order : order;
      };
    case 'name':
      return by((r) => r.file_name, descending);
    case 'progress':
      return by((r) => (r.total === null || r.total === 0 ? null : r.done / r.total), descending);
    case 'size':
      return by((r) => r.total, descending);
    case 'speed':
      return by((r) => (r.speed > 0 ? r.speed : null), descending);
    case 'left':
      return by((r) => (r.speed > 0 ? r.eta_secs : null), descending);
  }
}

/** The rows the list shows, in their groups and in display order. */
export function listLayout(
  order: readonly DownloadId[],
  get: (id: DownloadId) => DownloadSummary | undefined,
  { view, search, sort, folded, queues }: ListOptions,
): ListLayout {
  const needle = search.trim().toLowerCase();
  const queueIndex = new Map(queues.map((q, i) => [q, i]));
  const orderIndex = new Map<DownloadId, number>();
  const grouped = isGrouped(view);
  const inLine = view.kind === 'queue';
  const byGroup = new Map<StatusGroup | null, DownloadSummary[]>();
  order.forEach((id, i) => {
    const row = get(id);
    if (!row || !matches(row, view)) return;
    if (
      needle &&
      !row.file_name.toLowerCase().includes(needle) &&
      !row.url.toLowerCase().includes(needle)
    ) {
      return;
    }
    orderIndex.set(id, i);
    const group = sectionGroup(row, grouped, inLine);
    const rows = byGroup.get(group);
    if (rows) rows.push(row);
    else byGroup.set(group, [row]);
  });
  const place: Compare = (a, b) =>
    (queueIndex.get(a.queue) ?? queues.length) - (queueIndex.get(b.queue) ?? queues.length) ||
    (orderIndex.get(a.id) ?? 0) - (orderIndex.get(b.id) ?? 0);

  const ids: DownloadId[] = [];
  const sections: ListSection[] = [];
  const groups = groupOrder(grouped, inLine).filter((group) => byGroup.has(group));
  // One group alone needs no heading saying so, and is never folded away behind one.
  const headless = groups.length === 1;
  for (const group of groups) {
    const rows = byGroup.get(group);
    if (!rows) continue;
    // A stable sort, so ties keep queue order.
    // A queue's line is the order it runs in, whatever the sort; the sort orders the groups of work that is over.
    const by = inLine && group === null ? QUEUE_ORDER : sort;
    rows.sort(place).sort(compareFor(by, group, place));
    // A search shows everything it finds, folded or not.
    const isFolded = !headless && group !== null && needle === '' && folded.includes(group);
    sections.push({
      group: headless ? null : group,
      count: rows.length,
      folded: isFolded,
      start: ids.length,
      size: isFolded ? 0 : rows.length,
    });
    if (!isFolded) for (const row of rows) ids.push(row.id);
  }
  return { ids, sections };
}

/** The rows a section draws. */
export function sectionIds(layout: ListLayout, section: ListSection): DownloadId[] {
  return layout.ids.slice(section.start, section.start + section.size);
}

/** The section of `group` in a layout of `view`: its heading's, or the whole list in that status's own view. */
export function sectionOf(
  layout: ListLayout,
  view: View,
  group: StatusGroup,
): ListSection | undefined {
  const whole = view.kind === 'status' && view.group === group;
  return layout.sections.find((s) => s.group === group || (whole && s.group === null));
}

/** Where each download stands in its queue. */
export interface Places {
  /** Its place in the line, from 1: the "#" column. Finished and cancelled downloads have none. */
  position: Map<DownloadId, number>;
  /** For a waiting download, how many of its queue wait before it. */
  ahead: Map<DownloadId, number>;
}

export function placesOf(
  order: readonly DownloadId[],
  get: (id: DownloadId) => DownloadSummary | undefined,
): Places {
  const line = new Map<QueueId, number>();
  const waiting = new Map<QueueId, number>();
  const places: Places = { position: new Map(), ahead: new Map() };
  for (const id of order) {
    const row = get(id);
    if (!row || !isInLine(row.status)) continue;
    const place = (line.get(row.queue) ?? 0) + 1;
    line.set(row.queue, place);
    places.position.set(id, place);
    if (groupOf(row.status) === 'waiting') {
      const before = waiting.get(row.queue) ?? 0;
      places.ahead.set(id, before);
      waiting.set(row.queue, before + 1);
    }
  }
  return places;
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
    if (isInLine(row.status)) q.total += 1;
    if (group === 'active') q.running += 1;
    if (group === 'waiting') q.waiting += 1;
    if (group === 'paused') q.paused += 1;
  }
  return counts;
}
