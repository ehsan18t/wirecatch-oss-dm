import type { DownloadSummary } from '@wirecatch/protocol';
import { flushSync, untrack } from 'svelte';
import { describe, expect, it } from 'vitest';
import { DownloadStore } from './downloads.svelte';
import { track } from './reactivity';
import { visibleIds } from './view';

function row(id: number, patch: Partial<DownloadSummary> = {}): DownloadSummary {
  return {
    id,
    url: `https://example.com/f${id}.bin`,
    file_name: `f${id}.bin`,
    path: `C:\\d\\f${id}.bin`,
    queue: 1,
    category: 'other',
    status: 'completed',
    total: 1000,
    done: 1000,
    speed: 0,
    connections: 0,
    eta_secs: null,
    error: null,
    waiting: null,
    media: null,
    processing: null,
    added_at: id,
    completed_at: null,
    ...patch,
  };
}

function bigList(n: number, active: number): DownloadSummary[] {
  return Array.from({ length: n }, (_, i) =>
    i < active ? row(i + 1, { status: 'downloading', done: 0, speed: 100 }) : row(i + 1),
  );
}

describe('DownloadStore', () => {
  it('tells readers of a row and of the size when rows come and go', () => {
    const store = new DownloadStore();
    const seen: [number, string | undefined][] = [];
    const cleanup = $effect.root(() => {
      $effect(() => {
        seen.push([store.size, store.get(2)?.file_name]);
      });
    });
    flushSync();
    store.load([row(1)]);
    flushSync();
    store.apply([row(2)]);
    flushSync();
    store.remove([2]);
    flushSync();
    expect(seen).toEqual([
      [0, undefined],
      [1, undefined],
      [2, 'f2.bin'],
      [1, undefined],
    ]);
    cleanup();
  });

  it('keeps totals right as speeds change, rows move and rows go', () => {
    const store = new DownloadStore();
    store.load([
      row(1, { status: 'downloading', speed: 100 }),
      row(2, { queue: 2, status: 'downloading', speed: 50 }),
    ]);
    expect(store.totalSpeed).toBe(150);
    store.apply([row(1, { status: 'downloading', speed: 300 })]);
    expect(store.totalSpeed).toBe(350);
    expect(store.queueSpeed[1]).toBe(300);
    store.apply([row(1, { status: 'downloading', speed: 300, queue: 2 })]);
    expect(store.queueSpeed[1]).toBe(0);
    expect(store.queueSpeed[2]).toBe(350);
    store.remove([2]);
    expect(store.totalSpeed).toBe(300);
    expect(store.size).toBe(1);
  });

  it('appends new rows and reorders inside one queue only', () => {
    const store = new DownloadStore();
    store.load([row(1), row(2, { queue: 2 }), row(3), row(4)]);
    store.apply([row(5)]);
    expect(store.order).toEqual([1, 2, 3, 4, 5]);
    store.reorder(1, [4, 1]);
    // Queue 1 had 1, 3, 4, 5; now 4, 1, then the rest in their old order. Queue 2 keeps its slot.
    expect(store.order).toEqual([4, 2, 1, 3, 5]);
    expect(store.idsInQueue(1)).toEqual([4, 1, 3, 5]);
  });

  it('bumps the version only for changes that affect what the list shows', () => {
    const store = new DownloadStore();
    store.load([row(1, { status: 'downloading', done: 0 })]);
    const v = store.version;
    store.apply([row(1, { status: 'downloading', done: 10, speed: 5, eta_secs: 3 })]);
    expect(store.version).toBe(v);
    store.apply([row(1, { status: 'paused', done: 10 })]);
    expect(store.version).toBe(v + 1);
  });

  it('wakes only the row that changed', () => {
    const store = new DownloadStore();
    store.load(bigList(100, 100));
    const runs = new Map<number, number>();
    const cleanup = $effect.root(() => {
      for (const id of [1, 2, 3]) {
        $effect(() => {
          track(store.get(id)?.done);
          untrack(() => runs.set(id, (runs.get(id) ?? 0) + 1));
        });
      }
    });
    flushSync();
    store.apply([row(2, { status: 'downloading', done: 50, speed: 100 })]);
    flushSync();
    expect(runs.get(1)).toBe(1);
    expect(runs.get(2)).toBe(2);
    expect(runs.get(3)).toBe(1);
    cleanup();
  });

  it('handles progress for 10,000 rows without rebuilding the list', () => {
    const store = new DownloadStore();
    store.load(bigList(10_000, 60));
    let rebuilds = 0;
    let visible: number[] = [];
    const cleanup = $effect.root(() => {
      const ids = $derived.by(() => {
        const order = store.order;
        rebuilds += 1;
        return untrack(() =>
          visibleIds(order, (id) => store.get(id), {
            view: { kind: 'all' },
            search: '',
            sort: 'queue',
          }),
        );
      });
      $effect(() => {
        visible = ids;
      });
    });
    flushSync();
    expect(visible).toHaveLength(10_000);
    const before = rebuilds;

    // 200 batches of 60 changed rows, like 50 seconds of a busy core at four batches a second.
    const batches = Array.from({ length: 200 }, (_, b) =>
      Array.from({ length: 60 }, (_, i) =>
        row(i + 1, { status: 'downloading', done: b * 10 + i, speed: 100 + b, eta_secs: 1000 - b }),
      ),
    );
    const start = performance.now();
    for (const batch of batches) {
      store.apply(batch);
      flushSync();
    }
    const perBatch = (performance.now() - start) / batches.length;
    process.stdout.write(
      `[bench] apply 60 changed rows into 10,000: ${perBatch.toFixed(3)} ms per batch\n`,
    );

    expect(rebuilds).toBe(before);
    expect(store.lastBatchTouched).toBe(60);
    // Ceiling of one 60 Hz frame, generous so the test is not flaky when other test files run in
    // parallel; the measured value is printed. Alone it is under a millisecond.
    expect(perBatch).toBeLessThan(16);

    store.apply([row(1, { status: 'completed', done: 1000 })]);
    flushSync();
    expect(rebuilds).toBe(before + 1);
    cleanup();
  });

  // The store once copied fields from a hand-written list that missed `processing` and `media`, so
  // a conversion's step and percent never reached the main window after the row first appeared.
  it('takes every field of an update', () => {
    const store = new DownloadStore();
    store.load([row(1, { status: 'downloading', done: 0, speed: 5 })]);
    const update: DownloadSummary = {
      id: 1,
      url: 'https://example.com/other.mp4',
      file_name: 'other.mp4',
      path: 'D:\\v\\other.mp4',
      queue: 2,
      category: 'video',
      status: 'processing',
      total: 2000,
      done: 2000,
      speed: 0,
      connections: 0,
      eta_secs: 12,
      error: { message: 'Something went wrong.', fix: 'retry' },
      waiting: { type: 'in_line', data: { position: 3 } },
      media: { quality: '1080p', source: 'hls', video_key: 'abc' },
      processing: { step: 'converting', per_mille: 450 },
      added_at: 7,
      completed_at: 9,
    };
    store.apply([update]);
    expect({ ...store.get(1) }).toEqual(update);
  });
});
