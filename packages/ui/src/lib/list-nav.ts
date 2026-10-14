/** Something a list key can land on; a disabled one is skipped. */
export interface Navigable {
  disabled?: boolean | undefined;
}

/** The next enabled index after `from` (exclusive) in `step` direction, wrapping; -1 when every item is disabled. */
export function nextEnabledIndex(items: readonly Navigable[], from: number, step: 1 | -1): number {
  const length = items.length;
  for (let i = 1; i <= length; i++) {
    const index = (((from + step * i) % length) + length) % length;
    if (items[index]?.disabled !== true) return index;
  }
  return -1;
}

export type Orientation = 'horizontal' | 'vertical' | 'both';

const NEXT: Record<Orientation, readonly string[]> = {
  horizontal: ['ArrowRight'],
  vertical: ['ArrowDown'],
  both: ['ArrowRight', 'ArrowDown'],
};
const PREVIOUS: Record<Orientation, readonly string[]> = {
  horizontal: ['ArrowLeft'],
  vertical: ['ArrowUp'],
  both: ['ArrowLeft', 'ArrowUp'],
};

/** Where an arrow, Home or End key moves from `from` in a list laid out along `orientation`; null for any other key. */
export function listKeyIndex(
  items: readonly Navigable[],
  from: number,
  key: string,
  orientation: Orientation = 'both',
): number | null {
  if (NEXT[orientation].includes(key)) return nextEnabledIndex(items, from, 1);
  if (PREVIOUS[orientation].includes(key)) return nextEnabledIndex(items, from, -1);
  if (key === 'Home') return nextEnabledIndex(items, -1, 1);
  if (key === 'End') return nextEnabledIndex(items, items.length, -1);
  return null;
}

/** The first enabled item from `from + start` on whose label starts with `prefix`, wrapping; -1 when none does. */
function findPrefix(
  items: readonly (Navigable & { label: string })[],
  from: number,
  prefix: string,
  start: number,
): number {
  const length = items.length;
  for (let i = start; i < length + start; i++) {
    const index = (from + i + length) % length;
    const item = items[index];
    if (item && item.disabled !== true && item.label.toLocaleLowerCase().startsWith(prefix)) {
      return index;
    }
  }
  return -1;
}

/**
 * The enabled item that typing `typed` lands on, searching on from `from` and wrapping; -1 when none matches. Type-to-find, as in native lists and the APG examples: a word finds the first item it starts (which may be the current one), and one letter, or the same letter typed again and again, moves on to the next item that starts with it.
 */
export function typeaheadIndex(
  items: readonly (Navigable & { label: string })[],
  from: number,
  typed: string,
): number {
  const prefix = typed.toLocaleLowerCase();
  const letter = prefix.charAt(0);
  if (prefix === '') return -1;
  if (prefix.length > 1) {
    const found = findPrefix(items, from, prefix, 0);
    if (found >= 0 || prefix !== letter.repeat(prefix.length)) return found;
  }
  return findPrefix(items, from, letter, 1);
}

/** A printable key that type-to-find should take: one character, no modifier but Shift. */
export function isTypeaheadKey(event: KeyboardEvent): boolean {
  return (
    event.key.length === 1 && event.key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey
  );
}

/** How long a pause ends a type-to-find word. */
export const TYPEAHEAD_RESET_MS = 700;

/** Collects typed letters into a prefix that resets after a pause. */
export class Typeahead {
  #typed = '';
  #resetAt = 0;

  /** The prefix so far, with `key` added. */
  push(key: string, now = Date.now()): string {
    this.#typed = now > this.#resetAt ? key : this.#typed + key;
    this.#resetAt = now + TYPEAHEAD_RESET_MS;
    return this.#typed;
  }
}
