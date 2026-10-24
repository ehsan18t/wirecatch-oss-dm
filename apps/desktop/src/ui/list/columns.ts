// The download list's columns: the header and every row lay them out on one grid, from here.

import type { SortColumn, View } from '$lib/state/view';

/** The columns whose width the person drags. */
export type Resizable = 'progress' | 'size' | 'speed' | 'left';

export const RESIZABLE: readonly Resizable[] = ['progress', 'size', 'speed', 'left'];

/** Each resizable column's width until the person drags it, and the least it takes, in rem so they follow the text size. */
export const DEFAULT_WIDTH: Record<Resizable, number> = {
  progress: 10.5,
  size: 7.5,
  speed: 5.5,
  left: 4.5,
};

export const MIN_WIDTH: Record<Resizable, number> = {
  progress: 5,
  size: 4.5,
  speed: 3.75,
  left: 3,
};

/** The columns a view has: the place in the queue only in a queue's own view, where it is the order the queue runs in (elsewhere rows of several queues would repeat 1, 2, 3); and Speed and Left only while something in it downloads, the only rows that fill them. */
export interface ViewColumns {
  place: boolean;
  offered: readonly Resizable[];
}

export function viewColumns(view: View, downloading: boolean): ViewColumns {
  const offered = downloading ? RESIZABLE : STILL;
  return { place: view.kind === 'queue', offered };
}

/** The number columns of a list where nothing downloads: no speed, no time left. */
const STILL: readonly Resizable[] = ['progress', 'size'];

const EVERY_COLUMN: ViewColumns = { place: true, offered: RESIZABLE };

/** Widths in pixels the person set; columns not in it keep their default. */
export type Widths = Partial<Record<Resizable, number>>;

/** The fixed columns, in rem: the place in the queue, the kind of file, and the row's buttons. */
const PLACE = 2;
const TILE = 2.75;
const ACTIONS = 4.75;
/** Between columns (`gap-3`), and around them: the row's padding (`px-3`) and the list's (`px-2`), both sides. */
const GAP = 0.75;
const PADDING = 2.5;
/** The columns every row has: the kind of file, the name and the buttons. */
const FIXED_COLUMNS = 3;
/** The least a download's name keeps. */
const NAME = 16;
/** The number columns in the order they give way when the list is narrow: the last to show first. */
const GIVE_WAY: readonly Resizable[] = ['left', 'speed', 'size', 'progress'];

/** A column's width in pixels: as the person set it, else its usual width. */
function widthOf(column: Resizable, widths: Widths, rem: number): number {
  return widths[column] ?? DEFAULT_WIDTH[column] * rem;
}

/**
 * The number columns a list `width` pixels wide has room for, at the widths they have, so the name keeps at least 16rem: Time left gives way first, then Speed, then Size, whose figures then show under the name, then the progress column, whose wire then runs under the name. Where nothing is laid out (unit tests), all of them.
 */
export function shownColumns(
  width: number,
  widths: Widths,
  { place, offered }: ViewColumns = EVERY_COLUMN,
): Set<Resizable> {
  const shown = new Set(offered);
  if (width === 0) return shown;
  const rem = remInPixels();
  const fixed = place ? PLACE : 0;
  const needs = () => {
    const columns = FIXED_COLUMNS + (place ? 1 : 0) + shown.size;
    let total = (fixed + TILE + ACTIONS + PADDING + NAME + GAP * (columns - 1)) * rem;
    for (const column of shown) total += widthOf(column, widths, rem);
    return total;
  };
  for (const column of GIVE_WAY) {
    if (needs() <= width) break;
    shown.delete(column);
  }
  return shown;
}

/** The grid every row and the header share: the place in the queue (when the view has it), the kind of file, the name, the shown number columns at their widths, and the row's buttons. */
export function gridColumns(shown: ReadonlySet<Resizable>, widths: Widths, place = true): string {
  const numbers = RESIZABLE.filter((c) => shown.has(c)).map((c) => {
    const px = widths[c];
    return px === undefined ? `${String(DEFAULT_WIDTH[c])}rem` : `${String(px)}px`;
  });
  return [
    ...(place ? [`${String(PLACE)}rem`] : []),
    `${String(TILE)}rem`,
    'minmax(0, 1fr)',
    ...numbers,
    `${String(ACTIONS)}rem`,
  ].join(' ');
}

/** Each column's heading, the sort it sets when clicked, and the width it resizes. */
export const HEADINGS: readonly {
  id: string;
  label: string;
  sort?: SortColumn;
  /** A number column the person drags wider or narrower; it shows only where the list has room. */
  resize?: Resizable;
  /** A heading people read as a symbol, with its meaning for the pointer and screen readers. */
  title?: string;
  /** Read only by screen readers. */
  hidden?: boolean;
  numeric?: boolean;
}[] = [
  { id: 'position', label: '#', sort: 'position', title: 'Place in its queue', numeric: true },
  { id: 'type', label: 'Type', hidden: true },
  { id: 'name', label: 'Name', sort: 'name' },
  { id: 'progress', label: 'Progress', sort: 'progress', resize: 'progress' },
  { id: 'size', label: 'Size', sort: 'size', resize: 'size', numeric: true },
  { id: 'speed', label: 'Speed', sort: 'speed', resize: 'speed', numeric: true },
  { id: 'left', label: 'Left', sort: 'left', resize: 'left', title: 'Time left', numeric: true },
  { id: 'actions', label: 'Actions', hidden: true },
];

/** The browser's own text size, for where the page cannot say its own. */
const BROWSER_TEXT_SIZE = 16;

/** A rem in pixels, so widths in rem follow the person's text size. */
export function remInPixels(): number {
  return (
    Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || BROWSER_TEXT_SIZE
  );
}

/** The most a column widens: this many times its usual width. */
export const MOST_WIDTH = 3;

/** A number cell's text. */
export const NUMBER = 'truncate text-right text-small text-ink-muted tabular-nums';
