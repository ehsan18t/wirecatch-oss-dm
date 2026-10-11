import type { DownloadSummary } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { countRows, groupOf, sameView, visibleIds } from './view';

const rows: DownloadSummary[] = [
  ['b-video.mp4', 'video', 'downloading', 1, 500, 3],
  ['a-doc.pdf', 'document', 'completed', 1, 100, 1],
  ['c-app.exe', 'program', 'failed', 2, 900, 2],
  ['d-song.mp3', 'audio', 'queued', 2, null, 4],
].map(([file_name, category, status, queue, total, added_at], i) => ({
  id: i + 1,
  url: `https://example.com/${file_name}`,
  file_name: file_name as string,
  path: '',
  queue: queue as number,
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

describe('visibleIds', () => {
  it('filters by view', () => {
    expect(visibleIds(order, get, { kind: 'all' }, '', 'queue')).toEqual([1, 2, 3, 4]);
    expect(visibleIds(order, get, { kind: 'queue', id: 2 }, '', 'queue')).toEqual([3, 4]);
    expect(visibleIds(order, get, { kind: 'status', group: 'failed' }, '', 'queue')).toEqual([3]);
    expect(visibleIds(order, get, { kind: 'category', category: 'video' }, '', 'queue')).toEqual([
      1,
    ]);
  });

  it('searches names and links, ignoring case', () => {
    expect(visibleIds(order, get, { kind: 'all' }, 'SONG', 'queue')).toEqual([4]);
    expect(visibleIds(order, get, { kind: 'all' }, 'example.com/c-', 'queue')).toEqual([3]);
  });

  it('sorts', () => {
    expect(visibleIds(order, get, { kind: 'all' }, '', 'name')).toEqual([2, 1, 3, 4]);
    expect(visibleIds(order, get, { kind: 'all' }, '', 'size')).toEqual([3, 1, 2, 4]);
    expect(visibleIds(order, get, { kind: 'all' }, '', 'newest')).toEqual([4, 1, 3, 2]);
  });
});

describe('countRows', () => {
  it('counts for the sidebar', () => {
    const c = countRows(order, get);
    expect(c.all).toBe(4);
    expect(c.status).toEqual({
      active: 1,
      waiting: 1,
      paused: 0,
      finished: 1,
      failed: 1,
      cancelled: 0,
    });
    expect(c.category.video).toBe(1);
    expect(c.queue.get(1)).toEqual({ total: 1, running: 1, waiting: 0, paused: 0 });
    expect(c.queue.get(2)).toEqual({ total: 2, running: 0, waiting: 1, paused: 0 });
  });
});

describe('helpers', () => {
  it('groups statuses and compares views', () => {
    expect(groupOf('connecting')).toBe('active');
    expect(groupOf('queued')).toBe('waiting');
    expect(sameView({ kind: 'queue', id: 1 }, { kind: 'queue', id: 1 })).toBe(true);
    expect(sameView({ kind: 'queue', id: 1 }, { kind: 'queue', id: 2 })).toBe(false);
    expect(sameView({ kind: 'all' }, { kind: 'status', group: 'active' })).toBe(false);
  });
});
