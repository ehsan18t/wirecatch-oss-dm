// The navigation is one Tab stop, as Windows' navigation view is: the place shown takes Tab, and the arrows move through every place, section toggle and section button. A place's own buttons (a queue's pause and edit) are reached with the right arrow and left again with the left one.

import { closestElement, findElement, findElements } from '../lib/dom';
import { listKeyIndex } from '../lib/list-nav';

/** Where the arrows go, in order: the places, and the buttons beside a section's heading. */
const STOPS = '[data-nav-stop], [data-nav-section-action] button';
/** A place and the buttons beside it. */
const ENTRY = '[data-nav-entry]';
const ENTRY_BUTTONS = '[data-nav-actions] button';

function stops(root: HTMLElement): HTMLElement[] {
  return findElements(root, STOPS);
}

/** Gives Tab to one place only: the one shown, or the first. Every other place, and every button beside one, leaves the tab order. */
export function keepOneStop(root: HTMLElement): void {
  const all = stops(root);
  const chosen = all.find((el) => el.getAttribute('aria-current') === 'page') ?? all[0];
  for (const el of all) el.tabIndex = el === chosen ? 0 : -1;
  for (const el of findElements(root, ENTRY_BUTTONS)) el.tabIndex = -1;
}

/** Moves between a place's own buttons, or back to the place; whether the key was used. */
function withinEntry(key: string, target: HTMLElement, place: HTMLElement | null): boolean {
  const group = closestElement(target, '[data-nav-actions]');
  if (!group || !place) return false;
  const buttons = findElements(group, 'button');
  const index = buttons.indexOf(target);
  if (key === 'ArrowLeft') {
    (buttons[index - 1] ?? place).focus();
    return true;
  }
  if (key === 'ArrowRight') {
    buttons[index + 1]?.focus();
    return true;
  }
  return false;
}

/** The right arrow on a place: to the first of its own buttons, if it has any. */
function intoEntry(place: HTMLElement): boolean {
  const entry = closestElement(place, ENTRY);
  const first = entry ? findElement(entry, ENTRY_BUTTONS) : null;
  first?.focus();
  return first !== null;
}

/** Up, Down, Home and End: through every stop, from `from`. */
function alongStops(key: string, from: HTMLElement | null, root: HTMLElement): boolean {
  const list = stops(root);
  const index = from ? list.indexOf(from) : -1;
  if (index < 0) return false;
  const items = list.map((el) => ({ disabled: el.matches(':disabled') }));
  const next = listKeyIndex(items, index, key, 'vertical');
  if (next === null || next < 0) return false;
  list[next]?.focus();
  return true;
}

/** The navigation's keys; returns whether it used the key, so the caller can stop its default. */
export function navKey(event: KeyboardEvent, root: HTMLElement): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement) || event.altKey || event.ctrlKey || event.metaKey) {
    return false;
  }
  const inButtons = closestElement(target, '[data-nav-actions]') !== null;
  if (!inButtons) {
    return event.key === 'ArrowRight' ? intoEntry(target) : alongStops(event.key, target, root);
  }
  const entry = closestElement(target, ENTRY);
  const place = entry ? findElement(entry, '[data-nav-stop]') : null;
  return withinEntry(event.key, target, place) || alongStops(event.key, place, root);
}
