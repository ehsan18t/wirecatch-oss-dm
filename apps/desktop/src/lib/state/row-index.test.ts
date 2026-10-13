import { downloadId, queueId, type DownloadSummary } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { MockBackend } from '$lib/backend/mock';
import { QueueDeltas, RowIndex } from './row-index';

const SAMPLE = (await new MockBackend({ autoTick: false }).listDownloads()).slice(0, 6);
const rows = (): DownloadSummary[] => SAMPLE.map((row) => ({ ...row }));

describe('RowIndex', () => {
  it('keeps the first row of an id and the order rows came in', () => {
    const index = new RowIndex();
    const [a, b] = rows();
    expect(index.add(a!)).toBe(true);
    expect(index.add(b!)).toBe(true);
    expect(index.add({ ...a!, file_name: 'again' })).toBe(false);
    expect(index.order).toEqual([a!.id, b!.id]);
    expect(index.get(a!.id)?.file_name).toBe(a!.file_name);
  });

  it('removes only rows it has and returns them', () => {
    const index = new RowIndex();
    const list = rows();
    for (const row of list) index.add(row);
    const removed = index.remove([list[1]!.id, downloadId(999_999)]);
    expect(removed).toEqual([list[1]]);
    expect(index.order).toEqual(list.filter((_, i) => i !== 1).map((r) => r.id));
    expect(index.size).toBe(list.length - 1);
  });

  it('reorders one queue and leaves the other rows where they are', () => {
    const index = new RowIndex();
    const queue = queueId(1);
    const list = rows().map((row, i) => ({ ...row, queue: i % 2 === 0 ? queueId(1) : queueId(2) }));
    for (const row of list) index.add(row);
    const inQueue = list.filter((r) => r.queue === queue).map((r) => r.id);
    index.reorder(queue, [inQueue[2]!, inQueue[0]!]);
    const after = index.order.filter((id) => index.get(id)?.queue === queue);
    expect(after).toEqual([inQueue[2], inQueue[0], inQueue[1]]);
    const others = index.order.filter((id) => index.get(id)?.queue !== queue);
    expect(others).toEqual(list.filter((r) => r.queue !== queue).map((r) => r.id));
  });
});

describe('QueueDeltas', () => {
  it('sums per queue and skips zero changes', () => {
    const deltas = new QueueDeltas();
    deltas.add(queueId(1), 5);
    deltas.add(queueId(2), 0);
    deltas.add(queueId(1), -2);
    expect([...deltas.entries()]).toEqual([[1, 3]]);
  });
});
