import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';
import { stubMatchMedia } from './jsdom';

// jsdom.ts adds the browser features jsdom leaves out; this adds what only this package's tests need.

/** What `(prefers-reduced-motion: reduce)` answers; tests set it through `setReducedMotion`. */
const media = { reducedMotion: false };

export function setReducedMotion(on: boolean): void {
  media.reducedMotion = on;
}

stubMatchMedia((query) => query.includes('prefers-reduced-motion') && media.reducedMotion);

afterEach(() => {
  cleanup();
  media.reducedMotion = false;
  vi.useRealTimers();
});
