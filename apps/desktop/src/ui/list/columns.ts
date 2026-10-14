// The download list's columns: the header and every row size each one the same way, from here.

const NUMBER = 'shrink-0 truncate text-right text-small text-ink-muted tabular-nums';

/**
 * Each column's width. The number columns show from a width of the list's own (the grid is the `grid` container), so the list fits beside the details: Time left goes first, then Speed, then Size, and the row then shows those figures under its name (`FOLDED`).
 */
export const CELL = {
  type: 'flex w-9 shrink-0 items-center',
  name: 'min-w-0 flex-1',
  status: 'w-36 shrink-0 truncate text-small',
  size: `hidden w-28 @min-list-size/grid:block ${NUMBER}`,
  speed: `hidden w-24 @min-list-speed/grid:block ${NUMBER}`,
  eta: `hidden w-20 @min-list-eta/grid:block ${NUMBER}`,
  /** The row's own buttons float over its last columns, so this column takes no room, nor the gap before it. */
  actions: '-ml-3 w-0 shrink-0',
} as const;

/** Under the name, the figures of each column the list has no room for. */
export const FOLDED = {
  size: 'hidden @max-list-size/grid:inline',
  speed: 'hidden @max-list-speed/grid:inline',
  eta: 'hidden @max-list-eta/grid:inline',
} as const;

export type Column = keyof typeof CELL;

/** Each column's heading, in order. The first and last say what holds no words: the kind of file, and the row's buttons. */
export const COLUMNS: readonly { id: Column; label: string; hidden?: boolean }[] = [
  { id: 'type', label: 'Type', hidden: true },
  { id: 'name', label: 'Name' },
  { id: 'status', label: 'Status' },
  { id: 'size', label: 'Size' },
  { id: 'speed', label: 'Speed' },
  { id: 'eta', label: 'Time left' },
  { id: 'actions', label: 'Actions', hidden: true },
];
