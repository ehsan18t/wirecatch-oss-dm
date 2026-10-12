/** Finding the video and audio elements on a page, and what the button may say about them. */
import { nonEmpty } from '../text';
import { MAX_TITLE_LENGTH, isWebSrc, type ElementInfo } from './messages';

/** A muted, looping video without controls is decoration (a moving background): no button, playing or hovered. */
export function isDecorative(el: HTMLMediaElement): boolean {
  // `defaultMuted` is the `muted` attribute, which only sets `muted` when the page's HTML is parsed.
  return el.tagName === 'VIDEO' && (el.muted || el.defaultMuted) && el.loop && !el.controls;
}

/** Video always qualifies; audio only when it shows its own player (`controls`). */
export function isCandidate(el: HTMLMediaElement): boolean {
  return el.tagName === 'VIDEO' || (el.tagName === 'AUDIO' && el.controls);
}

export function elementInfo(el: HTMLMediaElement): ElementInfo {
  // The browser reports a missing address as an empty string.
  const src = nonEmpty(el.currentSrc) ?? nonEmpty(el.src) ?? el.querySelector('source')?.src ?? '';
  // `blob:`, `data:` and `mediasource:` addresses mean nothing outside the page: not sent.
  return { src: isWebSrc(src) ? src : null, audio: el.tagName === 'AUDIO' };
}

/** Most elements looked at when searching a page, its open shadow roots included, for media. */
const SEARCH_BUDGET = 20_000;

/**
 * The video and audio elements under `root`, including inside open shadow roots
 * (closed ones cannot be seen). Stops after `SEARCH_BUDGET` elements on huge pages.
 */
export function findMedia(root: ParentNode): HTMLMediaElement[] {
  const found: HTMLMediaElement[] = [];
  const roots: ParentNode[] = [root];
  let seen = 0;
  for (let next = roots.shift(); next !== undefined && seen < SEARCH_BUDGET; next = roots.shift()) {
    for (const el of next.querySelectorAll('*')) {
      seen += 1;
      if (el instanceof HTMLMediaElement) found.push(el);
      if (el.shadowRoot !== null) roots.push(el.shadowRoot);
      if (seen >= SEARCH_BUDGET) break;
    }
  }
  return found;
}

/** Like `container.contains(el)`, but also through shadow roots. */
export function containsDeep(container: Node, el: Node): boolean {
  for (let node: Node | null = el; node !== null;) {
    if (container.contains(node)) return true;
    const root = node.getRootNode();
    node = root instanceof ShadowRoot ? root.host : null;
  }
  return false;
}

/** The element's own label, or the page title. */
export function mediaTitle(el: HTMLMediaElement, doc: Document): string | null {
  const own = el.getAttribute('aria-label') ?? el.getAttribute('title') ?? '';
  const title = own.trim() || doc.title.trim();
  return title === '' ? null : title.slice(0, MAX_TITLE_LENGTH);
}
