// The widths the person dragged the list's columns to: a display preference of this window, kept in its page storage like the sidebar's.

import { readStored, writeStored } from '$lib/state/page-storage';
import { RESIZABLE, type Widths } from './columns';

const KEY = 'wirecatch.list.widths';

/** The widths kept, leaving out anything that is not a column and a width. */
export function storedWidths(): Widths {
  const raw = readStored(KEY);
  if (raw === null) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return {};
    const widths: Widths = {};
    for (const column of RESIZABLE) {
      const value: unknown = Reflect.get(parsed, column);
      if (typeof value === 'number' && Number.isFinite(value) && value > 0) widths[column] = value;
    }
    return widths;
  } catch {
    return {};
  }
}

export function keepWidths(widths: Widths): void {
  writeStored(KEY, JSON.stringify(widths));
}
