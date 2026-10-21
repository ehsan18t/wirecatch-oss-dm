// Which rows of a long list are rendered, and where to scroll to show one: the list draws only the rows on screen and a few beyond, so ten thousand downloads scroll like ten. Group headings sit between the rows, at a height of their own.

import type { DownloadId } from '@wirecatch/protocol';
import type { ListLayout, ListSection } from '$lib/state/view';

/** A row's height before the page has laid one out, and in unit tests, which lay nothing out: `h-row` at the browser's own text size. The list measures the drawn row and places rows by that, so a different text size never leaves gaps or overlaps. */
export const ASSUMED_ROW_HEIGHT = 50;

/** A group heading's height before one is laid out: `h-group` at the browser's own text size. */
export const ASSUMED_GROUP_HEIGHT = 36;

/** Items rendered beyond each edge of the view, so a quick scroll never shows a gap before the next frame draws. */
export const OVERSCAN = 6;

export type ListItem =
  { kind: 'group'; section: ListSection } | { kind: 'row'; id: DownloadId; index: number };

/** Every item of the list, where each starts from the top of the rows, and which item each row is. */
export interface Geometry {
  items: ListItem[];
  /** One more than `items`: the last is the height of them all. */
  tops: number[];
  /** For each row of `ListLayout.ids`, its place in `items`. */
  rowItem: number[];
}

export function geometry(layout: ListLayout, row: number, group: number): Geometry {
  const items: ListItem[] = [];
  const tops: number[] = [];
  const rowItem: number[] = [];
  let top = 0;
  for (const section of layout.sections) {
    if (section.group !== null) {
      items.push({ kind: 'group', section });
      tops.push(top);
      top += group;
    }
    for (let i = section.start; i < section.start + section.size; i++) {
      const id = layout.ids[i];
      if (id === undefined) continue;
      rowItem.push(items.length);
      items.push({ kind: 'row', id, index: i });
      tops.push(top);
      top += row;
    }
  }
  tops.push(top);
  return { items, tops, rowItem };
}

/** The item at `y` from the top of the rows: the last one starting at or above it. */
export function itemAt({ tops, items }: Geometry, y: number): number {
  let lo = 0;
  let hi = items.length - 1;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if ((tops[mid] ?? 0) <= y) lo = mid;
    else hi = mid - 1;
  }
  return Math.max(0, lo);
}

export interface Viewport {
  scrollTop: number;
  /** The scrolling area's height, the sticky column headers included. */
  height: number;
  /** The column headers' height: they stay at the top, over the first rows' place. */
  header: number;
  row: number;
}

/** The items to render: from `start`, up to but not including `end`. */
export interface ItemRange {
  start: number;
  end: number;
}

export function itemRange(geo: Geometry, { scrollTop, height }: Viewport): ItemRange {
  if (geo.items.length === 0) return { start: 0, end: 0 };
  return {
    start: Math.max(0, itemAt(geo, scrollTop) - OVERSCAN),
    end: Math.min(geo.items.length, itemAt(geo, scrollTop + height) + 1 + OVERSCAN),
  };
}

/**
 * Where to scroll so row `index` shows whole below the column headers: just enough, or null when it shows already.
 *
 * A scroller stops only on whole CSS pixels, at every display scale, so the place is rounded towards showing more: a list of fractional height would otherwise stop short of its last row.
 */
export function scrollToShow(
  geo: Geometry,
  index: number,
  { scrollTop, height, header, row }: Viewport,
): number | null {
  const item = geo.rowItem[index];
  if (item === undefined) return null;
  const top = geo.tops[item] ?? 0;
  if (top < scrollTop) return Math.floor(top);
  const bottom = header + top + row;
  if (bottom > scrollTop + height) return Math.ceil(bottom - height);
  return null;
}

/** Rows one Page Up or Page Down moves: a screenful, keeping one row in view. */
export function pageRows({ height, header, row }: Viewport): number {
  return Math.max(1, Math.floor((height - header) / row) - 1);
}

/** The place between rows a drag at `y` (from the top of the rows) points at, from 0 to the number of rows; on a heading, the first row under it. */
export function dropIndexAt(geo: Geometry, y: number): number {
  const count = geo.rowItem.length;
  if (geo.items.length === 0) return 0;
  const at = itemAt(geo, y);
  const item = geo.items[at];
  if (!item) return count;
  if (item.kind === 'group') return item.section.start;
  const top = geo.tops[at] ?? 0;
  const bottom = geo.tops[at + 1] ?? top;
  return y - top < (bottom - top) / 2 ? item.index : item.index + 1;
}

/** Where the line between rows for a drop at `index` into `section` is drawn, from the top of the rows: at the end of a group, under its last row rather than under the next group's heading. */
export function dropLineAt(geo: Geometry, index: number, section: ListSection): number {
  const atEnd = section.size > 0 && index === section.start + section.size;
  const item = atEnd ? undefined : geo.rowItem[index];
  if (item !== undefined) return geo.tops[item] ?? 0;
  const above = geo.rowItem[index - 1];
  return above === undefined ? 0 : (geo.tops[above + 1] ?? 0);
}
