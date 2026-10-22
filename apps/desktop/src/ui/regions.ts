// The main window's regions, in the order F6 visits them, as Windows apps do: the title bar's search, the navigation's place, and the list (or what the content shows instead). Shift+F6 goes back. A download's details are a modal dialog, which keeps the keyboard to itself.

import { SEARCH_ID } from '$lib/dom';

interface Region {
  /** Whether the focus is in this region now. */
  holds: (el: Element) => boolean;
  /** Where the focus goes in it; null when the region is not on screen. */
  target: () => HTMLElement | null;
}

function element(selector: string, root: ParentNode = document): HTMLElement | null {
  const found = root.querySelector(selector);
  return found instanceof HTMLElement ? found : null;
}

/** What the content takes the keyboard with: the list, or else its first control (settings, an empty list's action). */
function contentTarget(): HTMLElement | null {
  const list = element('#download-list');
  if (list) return list;
  const main = element('main');
  return main ? element('input, select, textarea, button, a[href], [tabindex="0"]', main) : null;
}

const REGIONS: readonly Region[] = [
  {
    holds: (el) => element(`#${SEARCH_ID}`)?.closest('header')?.contains(el) ?? false,
    target: () => element(`#${SEARCH_ID}`),
  },
  {
    holds: (el) => el.closest('nav') !== null,
    target: () => element('nav [data-nav-stop][tabindex="0"]'),
  },
  {
    holds: (el) => el.closest('main') !== null,
    target: contentTarget,
  },
];

/** Moves the focus to the next region on screen (`step` 1) or the one before (-1). */
export function cycleRegion(step: 1 | -1): void {
  const focused = document.activeElement;
  const current = focused ? REGIONS.findIndex((r) => r.holds(focused)) : -1;
  const count = REGIONS.length;
  for (let i = 1; i <= count; i++) {
    const start = current < 0 && step < 0 ? 0 : current;
    const region = REGIONS[(((start + step * i) % count) + count) % count];
    const target = region?.target();
    if (target) {
      target.focus();
      return;
    }
  }
}
