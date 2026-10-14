import { describe, expect, it } from 'vitest';
import { dropIndexAt, OVERSCAN, pageRows, rowRange, scrollToShow } from './viewport';

const view = { scrollTop: 0, height: 560, header: 28, row: 56 };

describe('the list viewport', () => {
  it('draws the rows on screen and a few beyond each edge', () => {
    expect(rowRange(view, 1000)).toEqual({ start: 0, end: 10 + OVERSCAN });
    expect(rowRange({ ...view, scrollTop: 56 * 100 }, 1000)).toEqual({
      start: 100 - OVERSCAN,
      end: 110 + OVERSCAN,
    });
    expect(rowRange({ ...view, scrollTop: 56 * 995 }, 1000).end).toBe(1000);
  });

  it('scrolls just enough to show a row whole below the column headers', () => {
    const at = { ...view, scrollTop: 56 * 10 };
    expect(scrollToShow(12, at)).toBeNull();
    expect(scrollToShow(4, at)).toBe(56 * 4);
    // The last row that fits under the headers, then one past it.
    expect(scrollToShow(18, at)).toBeNull();
    expect(scrollToShow(19, at)).toBe(28 + 56 * 20 - 560);
  });

  it('scrolls to whole CSS pixels, never short of the row', () => {
    // A list 746.5px tall, as a larger text size gives: the scroller stops only on whole CSS pixels.
    const tall = { scrollTop: 0, height: 746.5, header: 36, row: 70 };
    expect(scrollToShow(35, tall)).toBe(1810);
    // Even at 125%, where a device pixel is 0.8px: 1526.4 would stop at 1526, short of the row.
    expect(scrollToShow(41, { scrollTop: 0, height: 600, header: 26.375, row: 50 })).toBe(1527);
    // Rows 59.5px tall: scrolling up stops at or above the row's top.
    expect(scrollToShow(3, { ...tall, row: 59.5, scrollTop: 1000 })).toBe(178);
  });

  it('pages a screenful less one row, and points a drop between rows', () => {
    expect(pageRows(view)).toBe(8);
    expect(pageRows({ ...view, height: 60 })).toBe(1);
    expect(dropIndexAt(80, 56, 10)).toBe(1);
    expect(dropIndexAt(-20, 56, 10)).toBe(0);
    expect(dropIndexAt(5000, 56, 10)).toBe(10);
  });
});
