// The browser features the design system uses that jsdom leaves out, for every test setup that renders its components: this package's own and the apps'. The DOM types declare every one of them, so each is checked with `in` or `typeof`, not for truthiness.

import { vi } from 'vitest';

if (typeof HTMLDialogElement !== 'undefined' && !('showModal' in HTMLDialogElement.prototype)) {
  Object.assign(HTMLDialogElement.prototype, {
    show(this: HTMLDialogElement) {
      this.setAttribute('open', '');
    },
    showModal(this: HTMLDialogElement) {
      this.setAttribute('open', '');
    },
    close(this: HTMLDialogElement) {
      this.removeAttribute('open');
    },
  });
}

// jsdom hides popovers but has no API to show them; without the API, the components leave a flyout where it is, so here it shows, as in a browser that lacks the API. Important, because jsdom weighs its own rule by specificity and not by origin.
if (typeof document !== 'undefined' && !('showPopover' in HTMLElement.prototype)) {
  const style = document.createElement('style');
  style.textContent = '[popover] { display: block !important; }';
  document.head.append(style);
}

if (!('ResizeObserver' in globalThis)) {
  class ResizeObserverStub {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  }
  Object.assign(globalThis, { ResizeObserver: ResizeObserverStub });
}

// Svelte's `animate:` asks an element for its running animations.
if (typeof Element !== 'undefined' && !('getAnimations' in Element.prototype)) {
  Object.assign(Element.prototype, { getAnimations: () => [] });
}

if (typeof Element !== 'undefined' && !('scrollIntoView' in Element.prototype)) {
  Object.assign(Element.prototype, { scrollIntoView: vi.fn() });
}

/**
 * Gives the window the `matchMedia` jsdom leaves out, answering each query with `matches(query)`,
 * asked whenever a component reads it. The test environment copies jsdom's window keys onto the
 * global, `matchMedia` among them with no value, so the check is for a function.
 */
export function stubMatchMedia(matches: (query: string) => boolean): void {
  if (typeof window === 'undefined' || typeof window.matchMedia === 'function') return;
  Object.assign(window, {
    matchMedia: (query: string) => ({
      get matches() {
        return matches(query);
      },
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: () => false,
    }),
  });
}
