import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';
import './jsdom';

// jsdom.ts adds the browser features jsdom leaves out; this adds what only this package's tests need.

/** What `(prefers-reduced-motion: reduce)` answers; tests set it through `setReducedMotion`. */
const media = { reducedMotion: false };

export function setReducedMotion(on: boolean): void {
  media.reducedMotion = on;
}

if (typeof window.matchMedia !== 'function') {
  Object.assign(window, {
    matchMedia: (query: string) => ({
      get matches() {
        return query.includes('prefers-reduced-motion') ? media.reducedMotion : false;
      },
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: () => false,
    }),
  });
}

afterEach(() => {
  cleanup();
  media.reducedMotion = false;
  vi.useRealTimers();
});
