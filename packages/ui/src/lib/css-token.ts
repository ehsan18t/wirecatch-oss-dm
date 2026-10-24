// A memo, not state: tokens are fixed for the life of the page, and each window runs its own copy of this module.
const cache = new Map<string, string>();

/** A token from tokens.css as the root computes it, or an empty string where the stylesheet is not loaded (unit tests). Cached, as tokens do not change while a page runs. */
export function token(name: string): string {
  const cached = cache.get(name);
  if (cached !== undefined) return cached;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  if (value !== '') cache.set(name, value);
  return value;
}

const MS_PER_SECOND = 1000;

/** A time token (`150ms`, `0.2s`) in milliseconds; 0 when it is not available. */
export function timeToken(name: string): number {
  const value = token(name);
  const amount = Number.parseFloat(value);
  if (!Number.isFinite(amount)) return 0;
  return value.endsWith('ms') ? amount : amount * MS_PER_SECOND;
}

/** A length token in pixels (`8px`, or rem against the root's font size); 0 when it is not available. */
export function lengthToken(name: string): number {
  const value = token(name);
  const amount = Number.parseFloat(value);
  if (!Number.isFinite(amount)) return 0;
  if (!value.endsWith('rem')) return amount;
  return amount * Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
}
