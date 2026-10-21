import { downloadId, queueId, type DownloadSummary } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  countRows,
  groupOf,
  LIST_FIELDS,
  LIVE_SORT_FIELDS,
  listLayout,
  type ListOptions,
  nextSort,
  placesOf,
  QUEUE_ORDER,
  SORT_LABEL,
  type SortColumn,
  sameView,
  type View,
} from './view';

const rows: DownloadSummary[] = [
  ['b-video.mp4', 'video', 'downloading', 1, 500, 3],
  ['a-doc.pdf', 'document', 'completed', 1, 100, 1],
  ['c-app.exe', 'program', 'failed', 2, 900, 2],
  ['d-song.mp3', 'audio', 'queued', 2, null, 4],
].map(([file_name, category, status, queue, total, added_at], i) => ({
  id: downloadId(i + 1),
  url: `https://example.com/${file_name}`,
  file_name: file_name as string,
  path: '',
  queue: queueId(queue as number),
  category: category as DownloadSummary['category'],
  status: status as DownloadSummary['status'],
  total: total as number | null,
  done: 0,
  speed: 0,
  connections: 0,
  eta_secs: null,
  error: null,
  waiting: null,
  media: null,
  processing: null,
  added_at: added_at as number,
  completed_at: null,
}));
const order = rows.map((r) => r.id);
const get = (id: number) => rows.find((r) => r.id === id);

// The list was rebuilt only when a field in a hand-kept list changed, and that list missed the
// address (which search matches) and the time added (the newest-first sort).
describe('LIST_FIELDS', () => {
  it('names every field the list, the positions and the counts read', () => {
    const read = new Set<string | symbol>();
    const watch = (id: number): DownloadSummary | undefined => {
      const row = get(id);
      return (
        row &&
        new Proxy(row, {
          get(target, key, receiver) {
            read.add(key);
            const value: unknown = Reflect.get(target, key, receiver);
            return value;
          },
        })
      );
    };
    const views: View[] = [
      { kind: 'all' },
      { kind: 'status', group: 'failed' },
      { kind: 'queue', id: queueId(2) },
      { kind: 'category', category: 'video' },
    ];
    const live = new Set<SortColumn>(['progress', 'speed', 'left']);
    for (const view of views) {
      for (const column of Object.keys(SORT_LABEL) as SortColumn[]) {
        if (live.has(column)) continue;
        for (const search of ['', 'example']) {
          listLayout(order, watch, {
            ...options,
            view,
            search,
            sort: { column, descending: false },
          });
        }
      }
    }
    placesOf(order, watch);
    countRows(order, watch);
    read.delete('id');
    const listed = new Set<string | symbol>(LIST_FIELDS);
    expect([...read].filter((field) => !listed.has(field))).toEqual([]);
  });

  it('leaves the live fields to the sorts by them', () => {
    const read = new Set<string | symbol>();
    const watch = (id: number): DownloadSummary | undefined => {
      const row = get(id);
      return (
        row &&
        new Proxy(row, {
          get(target, key, receiver) {
            read.add(key);
            const value: unknown = Reflect.get(target, key, receiver);
            return value;
          },
        })
      );
    };
    for (const column of ['progress', 'speed', 'left'] as const) {
      listLayout(order, watch, { ...options, sort: { column, descending: true } });
    }
    read.delete('id');
    const listed = new Set<string | symbol>([...LIST_FIELDS, ...LIVE_SORT_FIELDS]);
    expect([...read].filter((field) => !listed.has(field))).toEqual([]);
  });
});

const options: ListOptions = {
  view: { kind: 'all' },
  search: '',
  sort: QUEUE_ORDER,
  folded: [],
  queues: [queueId(1), queueId(2)],
};

const ids = (o: Partial<ListOptions>) => listLayout(order, get, { ...options, ...o }).ids;

describe('listLayout', () => {
  it('filters by view', () => {
    expect(ids({ view: { kind: 'queue', id: queueId(2) } })).toEqual([3, 4]);
    expect(ids({ view: { kind: 'status', group: 'failed' } })).toEqual([3]);
    expect(ids({ view: { kind: 'category', category: 'video' } })).toEqual([1]);
  });

  it('groups by state, in the order of what needs the person most', () => {
    const layout = listLayout(order, get, options);
    expect(layout.sections.map((s) => s.group)).toEqual([
      'active',
      'failed',
      'waiting',
      'finished',
    ]);
    expect(layout.ids).toEqual([1, 3, 4, 2]);
  });

  it('leaves one view of one status ungrouped', () => {
    const layout = listLayout(order, get, {
      ...options,
      view: { kind: 'status', group: 'active' },
    });
    expect(layout.sections).toEqual([{ group: null, count: 1, folded: false, start: 0, size: 1 }]);
  });

  it('counts a folded group and draws none of its rows', () => {
    const layout = listLayout(order, get, { ...options, folded: ['finished'] });
    expect(layout.ids).toEqual([1, 3, 4]);
    expect(layout.sections.at(-1)).toEqual({
      group: 'finished',
      count: 1,
      folded: true,
      start: 3,
      size: 0,
    });
  });

  it('shows what a search finds even in a folded group', () => {
    const layout = listLayout(order, get, { ...options, search: 'a-doc', folded: ['finished'] });
    expect(layout.ids).toEqual([2]);
    expect(layout.sections).toEqual([
      { group: 'finished', count: 1, folded: false, start: 0, size: 1 },
    ]);
  });

  it('searches names and links, ignoring case', () => {
    expect(ids({ search: 'SONG' })).toEqual([4]);
    expect(ids({ search: 'example.com/c-' })).toEqual([3]);
  });

  it('sorts inside each group, and never the groups themselves', () => {
    expect(
      ids({ view: { kind: 'queue', id: queueId(2) }, sort: { column: 'name', descending: true } }),
    ).toEqual([3, 4]);
    expect(
      ids({
        view: { kind: 'status', group: 'active' },
        sort: { column: 'size', descending: true },
      }),
    ).toEqual([1]);
  });

  it('keeps Up next in queue order whatever the sort', () => {
    const waiting = rows.filter((r) => r.status === 'queued').map((r) => r.id);
    expect(
      ids({
        view: { kind: 'status', group: 'waiting' },
        sort: { column: 'name', descending: true },
      }),
    ).toEqual(waiting);
  });

  it('lists one queue after another in queue order', () => {
    expect(
      ids({ view: { kind: 'category', category: 'audio' }, queues: [queueId(2), queueId(1)] }),
    ).toEqual([4]);
    expect(
      ids({ view: { kind: 'all' }, queues: [queueId(2), queueId(1)], sort: QUEUE_ORDER }),
    ).toEqual([1, 3, 4, 2]);
  });
});

describe('nextSort', () => {
  it('goes ascending, then descending, then back to queue order', () => {
    const name = nextSort(QUEUE_ORDER, 'name');
    expect(name).toEqual({ column: 'name', descending: false });
    const down = nextSort(name, 'name');
    expect(down).toEqual({ column: 'name', descending: true });
    expect(nextSort(down, 'name')).toEqual(QUEUE_ORDER);
    expect(nextSort(down, 'size')).toEqual({ column: 'size', descending: false });
  });
});

describe('placesOf', () => {
  it('numbers the line of each queue from 1, skipping what is finished', () => {
    const { position, ahead } = placesOf(order, get);
    expect([...position]).toEqual([
      [1, 1],
      [3, 1],
      [4, 2],
    ]);
    expect([...ahead]).toEqual([[4, 0]]);
  });
});

describe('helpers', () => {
  it('groups statuses and compares views', () => {
    expect(groupOf('connecting')).toBe('active');
    expect(groupOf('queued')).toBe('waiting');
    expect(sameView({ kind: 'queue', id: queueId(1) }, { kind: 'queue', id: queueId(1) })).toBe(
      true,
    );
    expect(sameView({ kind: 'queue', id: queueId(1) }, { kind: 'queue', id: queueId(2) })).toBe(
      false,
    );
    expect(sameView({ kind: 'all' }, { kind: 'status', group: 'active' })).toBe(false);
  });
});
