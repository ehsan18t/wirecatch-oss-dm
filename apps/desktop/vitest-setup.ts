import '@testing-library/jest-dom/vitest';
import { afterAll, vi } from 'vitest';

// Test files share one jsdom window per worker (`isolate: false` in vite.config.ts), so each file
// leaves it as it found it: no stored preferences, no theme, nothing left in the page, real
// timers. Stubbed globals and environment variables are put back by the config.
afterAll(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  document.body.replaceChildren();
  vi.useRealTimers();
});

// jsdom leaves out a few browser features the interface uses. The DOM types declare every one of
// them, so a truthiness check reads as always true to the type checker; they are checked with `in`
// or `typeof` instead.
if (!('ResizeObserver' in globalThis)) {
  class ResizeObserverStub {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  }
  Object.assign(globalThis, { ResizeObserver: ResizeObserverStub });
}

if (typeof Element !== 'undefined' && !('scrollIntoView' in Element.prototype)) {
  Object.assign(Element.prototype, { scrollIntoView: vi.fn() });
}

if (typeof HTMLDialogElement !== 'undefined' && !('showModal' in HTMLDialogElement.prototype)) {
  Object.assign(HTMLDialogElement.prototype, {
    showModal(this: HTMLDialogElement) {
      this.setAttribute('open', '');
    },
    close(this: HTMLDialogElement) {
      this.removeAttribute('open');
    },
  });
}

// The test environment copies jsdom's window keys onto the global, `matchMedia` among them with no
// value, so here the check is for a function.
if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
  // A wide window that matches every query, unless a test says otherwise.
  Object.assign(window, {
    matchMedia: (query: string): MediaQueryList =>
      ({
        matches: true,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: () => false,
      }) as MediaQueryList,
  });
}
