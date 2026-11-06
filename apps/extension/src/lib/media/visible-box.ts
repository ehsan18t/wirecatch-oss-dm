/** The part of a page element its ancestors show, in viewport pixels. */

import type { Box } from './placement';

/** Most ancestors whose clipping is taken into account; a player is a few levels up. */
const MOST_CLIPPING_ANCESTORS = 32;

/**
 * The part of `el` its ancestors show: its box cut by every ancestor that hides what overflows it.
 * A player may size its video larger than itself, and the button belongs on what is seen. When none
 * of the video shows (YouTube keeps it outside the player before playback, which shows a picture of
 * it), the innermost such ancestor, its player, stands in for it.
 */
function visibleBox(el: Element): Box {
  const doc = el.ownerDocument;
  let shown = boxOf(el.getBoundingClientRect());
  let player: Box | null = null;
  let ancestor = el.parentElement;
  for (let level = 0; ancestor !== null && level < MOST_CLIPPING_ANCESTORS; level += 1) {
    // The body's and the root's overflow is the window's (CSS Overflow 3, 3.3): the body itself
    // clips nothing, and YouTube's body box is 0 pixels tall.
    if (ancestor === doc.body || ancestor === doc.documentElement) break;
    if (hidesOverflow(ancestor)) {
      const clip = boxOf(ancestor.getBoundingClientRect());
      player ??= clip;
      shown = cut(shown, clip);
    }
    ancestor = ancestor.parentElement;
  }
  return shown.height === 0 && player !== null ? player : shown;
}

function boxOf(rect: Box): Box {
  const { top, left, right, bottom } = rect;
  return { top, left, right, bottom, width: right - left, height: bottom - top };
}

/** `box` cut to `clip`, empty when they do not meet. */
function cut(box: Box, clip: Box): Box {
  const top = Math.max(box.top, clip.top);
  const left = Math.max(box.left, clip.left);
  const right = Math.min(box.right, clip.right);
  const bottom = Math.min(box.bottom, clip.bottom);
  return {
    top,
    left,
    right,
    bottom,
    width: Math.max(0, right - left),
    height: Math.max(0, bottom - top),
  };
}

/** Whether `el` hides what overflows it, in either direction. */
function hidesOverflow(el: Element): boolean {
  const style = el.ownerDocument.defaultView?.getComputedStyle(el);
  if (style === undefined) return false;
  return clips(style.overflow) || clips(style.overflowX) || clips(style.overflowY);
}

function clips(overflow: string): boolean {
  return overflow !== '' && overflow !== 'visible';
}

/** Step between the points tested down the video's right edge. */
const PROBE_STEP = 4;
/** Deepest a fixed bar is looked for below the video's top; page headers are shallower. */
const PROBE_DEPTH = 200;
/** Most ancestors of a hit element looked through for a fixed or sticky one. */
const MOST_BAR_ANCESTORS = 16;

/**
 * Where `box`, the part of `el` its ancestors show, comes out from under the page's fixed or
 * sticky bars along its right edge: its own top when nothing covers it. A page header stays put
 * while the page scrolls, and a video scrolled under it is hidden there.
 */
function belowFixedBars(el: Element, box: Box, own: Element): number {
  const doc = el.ownerDocument;
  if (typeof doc.elementFromPoint !== 'function') return box.top;
  const x = box.right - PROBE_STEP;
  const start = Math.max(box.top, 0);
  const deepest = Math.min(box.bottom, start + PROBE_DEPTH);
  for (let y = start; y < deepest; y += PROBE_STEP) {
    const hit = doc.elementFromPoint(x, y);
    if (hit === null || !isFixedBar(hit, el, own)) return y;
  }
  return box.top;
}

/** Whether `hit` belongs to a fixed or sticky element that holds neither the media nor the button. */
function isFixedBar(hit: Element, media: Element, own: Element): boolean {
  if (own.contains(hit) || hit.contains(media) || media.contains(hit)) return false;
  const view = hit.ownerDocument.defaultView;
  let node: Element | null = hit;
  for (let level = 0; node !== null && level < MOST_BAR_ANCESTORS; level += 1) {
    if (node.contains(media)) return false;
    const position = view?.getComputedStyle(node).position;
    if (position === 'fixed' || position === 'sticky') return true;
    node = node.parentElement;
  }
  return false;
}

/** The part of `el` a person sees, below any fixed bar of the page, and whether such a bar covers its top; `own` is the button's host. */
export function shownPart(el: Element, own: Element): { box: Box; covered: boolean } {
  const seen = visibleBox(el);
  const clear = belowFixedBars(el, seen, own);
  return {
    box: { ...seen, top: clear, height: Math.max(0, seen.bottom - clear) },
    covered: clear > seen.top,
  };
}
