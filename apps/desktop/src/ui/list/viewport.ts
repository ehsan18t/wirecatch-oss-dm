// Which rows of a long list are rendered, and where to scroll to show one: the list draws only the rows on screen and a few beyond, so ten thousand downloads scroll like ten.

/** A row's height before the page has laid one out, and in unit tests, which lay nothing out: `h-row` at the browser's own text size. The list measures the drawn row and places rows by that, so a different text size never leaves gaps or overlaps. */
export const ASSUMED_ROW_HEIGHT = 56;

/** Rows rendered beyond each edge of the view, so a quick scroll never shows a gap before the next frame draws. */
export const OVERSCAN = 6;

/** The rows to render: from `start`, up to but not including `end`. */
export interface RowRange {
  start: number;
  end: number;
}

export interface Viewport {
  scrollTop: number;
  /** The scrolling area's height, the sticky column headers included. */
  height: number;
  /** The column headers' height: they stay at the top, over the first rows' place. */
  header: number;
  row: number;
}

export function rowRange({ scrollTop, height, row }: Viewport, count: number): RowRange {
  return {
    start: Math.max(0, Math.floor(scrollTop / row) - OVERSCAN),
    end: Math.min(count, Math.ceil((scrollTop + height) / row) + OVERSCAN),
  };
}

/**
 * Where to scroll so row `index` shows whole below the column headers: just enough, or null when it shows already.
 *
 * A scroller stops only on whole CSS pixels, at every display scale, so the place is rounded towards showing more: a list of fractional height would otherwise stop short of its last row.
 */
export function scrollToShow(
  index: number,
  { scrollTop, height, header, row }: Viewport,
): number | null {
  const top = index * row;
  if (top < scrollTop) return Math.floor(top);
  const bottom = header + top + row;
  if (bottom > scrollTop + height) return Math.ceil(bottom - height);
  return null;
}

/** Rows one Page Up or Page Down moves: a screenful, keeping one row in view. */
export function pageRows({ height, header, row }: Viewport): number {
  return Math.max(1, Math.floor((height - header) / row) - 1);
}

/** The place between rows a drag at `y` (from the top of the rows) points at, from 0 to `count`. */
export function dropIndexAt(y: number, row: number, count: number): number {
  return Math.max(0, Math.min(count, Math.round(y / row)));
}
