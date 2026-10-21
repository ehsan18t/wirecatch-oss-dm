import { downloadId } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { dropTarget, nudged, placed } from './order';

const ids = (...values: number[]) => values.map(downloadId);

describe('placed', () => {
  const queue = ids(1, 2, 3, 4, 5);

  it('moves rows before another, keeping their own order', () => {
    expect(placed(queue, ids(4, 2), downloadId(1))).toEqual([2, 4, 1, 3, 5]);
  });

  it('moves rows to the end without a row to go before, or one not in the queue', () => {
    expect(placed(queue, ids(1), null)).toEqual([2, 3, 4, 5, 1]);
    expect(placed(queue, ids(1), downloadId(99))).toEqual([2, 3, 4, 5, 1]);
  });

  it('says nothing changes when the rows are already there, or none is in the queue', () => {
    expect(placed(queue, ids(2), downloadId(3))).toBeNull();
    expect(placed(queue, ids(99), downloadId(1))).toBeNull();
  });
});

describe('nudged', () => {
  const queue = ids(1, 2, 3, 4, 5);
  const selected = (ids: number[]) => (id: number) => ids.includes(id);

  it('goes up one place, before the row above', () => {
    expect(nudged(queue, selected([3, 4]), -1)).toEqual({ chosen: [3, 4], before: 2 });
  });

  it('goes down one place, before the row after the next', () => {
    expect(nudged(queue, selected([2, 3]), 1)).toEqual({ chosen: [2, 3], before: 5 });
    expect(nudged(queue, selected([3, 4]), 1)).toEqual({ chosen: [3, 4], before: null });
  });

  it('stays at either end, and needs a selected row of the queue', () => {
    expect(nudged(queue, selected([1, 3]), -1)).toBeNull();
    expect(nudged(queue, selected([5]), 1)).toBeNull();
    expect(nudged(queue, selected([]), 1)).toBeNull();
  });
});

const [da, db, dc, dd, de] = [
  downloadId(1),
  downloadId(2),
  downloadId(3),
  downloadId(4),
  downloadId(5),
];

describe('dropTarget', () => {
  const line = [da, db, dc, dd];

  it('goes before the first row of the line at or after the drop', () => {
    expect(dropTarget([da, db, dc, dd], 1, line, new Set([dd]))).toBe(db);
  });

  it('goes after the last row above when none of the line follows on screen', () => {
    // On screen: a, then a row of another queue; dropped at the end.
    expect(dropTarget([da, de], 2, line, new Set([dc]))).toBe(db);
    expect(dropTarget([dc, dd], 2, line, new Set([da]))).toBeNull();
  });

  it('says nothing when no other row of the line is on screen', () => {
    expect(dropTarget([da, de], 1, line, new Set([da]))).toBeUndefined();
  });
});
