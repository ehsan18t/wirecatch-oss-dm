import { downloadId } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import type { ListLayout } from '$lib/state/view';
import {
  dropIndexAt,
  dropLineAt,
  geometry,
  itemRange,
  OVERSCAN,
  pageRows,
  scrollToShow,
} from './viewport';

const view = { scrollTop: 0, height: 560, header: 28, row: 56 };

/** `n` rows in one section with no heading, as a view of one status shows them. */
function flat(n: number): ListLayout {
  return {
    ids: Array.from({ length: n }, (_, i) => downloadId(i + 1)),
    sections: [{ group: null, count: n, folded: false, start: 0, size: n }],
  };
}

describe('the list viewport', () => {
  it('draws the rows on screen and a few beyond each edge', () => {
    const geo = geometry(flat(1000), 56, 36);
    expect(itemRange(geo, view)).toEqual({ start: 0, end: 11 + OVERSCAN });
    expect(itemRange(geo, { ...view, scrollTop: 56 * 100 })).toEqual({
      start: 100 - OVERSCAN,
      end: 111 + OVERSCAN,
    });
    expect(itemRange(geo, { ...view, scrollTop: 56 * 995 }).end).toBe(1000);
  });

  it('scrolls just enough to show a row whole below the column headers', () => {
    const geo = geometry(flat(100), 56, 36);
    const at = { ...view, scrollTop: 56 * 10 };
    expect(scrollToShow(geo, 12, at)).toBeNull();
    expect(scrollToShow(geo, 4, at)).toBe(56 * 4);
    // The last row that fits under the headers, then one past it.
    expect(scrollToShow(geo, 18, at)).toBeNull();
    expect(scrollToShow(geo, 19, at)).toBe(28 + 56 * 20 - 560);
  });

  it('scrolls to whole CSS pixels, never short of the row', () => {
    // A list 746.5px tall, as a larger text size gives: the scroller stops only on whole CSS pixels.
    const tall = { scrollTop: 0, height: 746.5, header: 36, row: 70 };
    expect(scrollToShow(geometry(flat(100), 70, 36), 35, tall)).toBe(1810);
    // Even at 125%, where a device pixel is 0.8px: 1526.4 would stop at 1526, short of the row.
    const small = { scrollTop: 0, height: 600, header: 26.375, row: 50 };
    expect(scrollToShow(geometry(flat(100), 50, 36), 41, small)).toBe(1527);
    // Rows 59.5px tall: scrolling up stops at or above the row's top.
    expect(
      scrollToShow(geometry(flat(100), 59.5, 36), 3, { ...tall, row: 59.5, scrollTop: 1000 }),
    ).toBe(178);
  });

  it('pages a screenful less one row, and points a drop between rows', () => {
    expect(pageRows(view)).toBe(8);
    expect(pageRows({ ...view, height: 60 })).toBe(1);
    const list = flat(10);
    const geo = geometry(list, 56, 36);
    expect(dropIndexAt(geo, 80)).toBe(1);
    expect(dropIndexAt(geo, -20)).toBe(0);
    expect(dropIndexAt(geo, 5000)).toBe(10);
    expect(dropLineAt(geo, 10, list.sections[0]!)).toBe(560);
  });

  it('places headings between the groups, and no rows under a folded one', () => {
    const layout: ListLayout = {
      ids: [downloadId(1), downloadId(2), downloadId(3)],
      sections: [
        { group: 'active', count: 2, folded: false, start: 0, size: 2 },
        { group: 'finished', count: 5, folded: true, start: 2, size: 0 },
        { group: 'cancelled', count: 1, folded: false, start: 2, size: 1 },
      ],
    };
    const geo = geometry(layout, 50, 30);
    expect(geo.items.map((i) => i.kind)).toEqual(['group', 'row', 'row', 'group', 'group', 'row']);
    expect(geo.tops).toEqual([0, 30, 80, 130, 160, 190, 240]);
    expect(geo.rowItem).toEqual([1, 2, 5]);
    // A drop on a heading points at the first row under it.
    expect(dropIndexAt(geo, 165)).toBe(2);
    expect(scrollToShow(geo, 2, { scrollTop: 0, height: 100, header: 0, row: 50 })).toBe(140);
  });

  it('draws the line for a drop at the end of a group under its last row, not under the next heading', () => {
    const active = { group: 'active' as const, count: 2, folded: false, start: 0, size: 2 };
    const waiting = { group: 'waiting' as const, count: 1, folded: false, start: 2, size: 1 };
    const geo = geometry(
      { ids: [downloadId(1), downloadId(2), downloadId(3)], sections: [active, waiting] },
      50,
      30,
    );
    expect(dropLineAt(geo, 2, active)).toBe(130);
    expect(dropLineAt(geo, 2, waiting)).toBe(160);
    expect(dropLineAt(geo, 1, active)).toBe(80);
  });
});
