// Finding elements in the page by what they are, checked, rather than by what a selector is
// assumed to match.

/** The first HTML element under `root` that matches `selector`, or null. */
export function findElement(
  root: ParentNode | null | undefined,
  selector: string,
): HTMLElement | null {
  const found = root?.querySelector(selector);
  return found instanceof HTMLElement ? found : null;
}

const DIGITS = /^\d+$/;

/**
 * The id in the `data-id` of `target` or the nearest element around it that has one: digits
 * only, as the list writes it. Null when there is none or it is anything else.
 */
export function dataIdAt(target: EventTarget | null): number | null {
  if (!(target instanceof Element)) return null;
  const owner = target.closest('[data-id]');
  if (!(owner instanceof HTMLElement)) return null;
  const text = owner.dataset.id ?? '';
  if (!DIGITS.test(text)) return null;
  const id = Number(text);
  return Number.isSafeInteger(id) ? id : null;
}

/** The id of the title bar's one search field, which F6, Ctrl+K and its clear button all reach: downloads on the list, settings while they show. */
export const SEARCH_ID = 'search';
