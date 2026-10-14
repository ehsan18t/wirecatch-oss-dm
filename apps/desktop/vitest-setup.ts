import '@testing-library/jest-dom/vitest';
import '@wirecatch/ui/testing/jsdom';
import { afterAll, afterEach, vi } from 'vitest';

// Test files share one jsdom window per worker (`isolate: false` in vite.config.ts), so each file
// leaves it as it found it: no stored preferences, no theme, nothing left in the page, real
// timers. Stubbed globals and environment variables are put back by the config.
// A spy on a timer function made under fake timers wraps the fake one: restoring it after real
// timers come back would put the fake back for good. So every test ends here, in this order,
// and no test file switches timers back on its own.
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

afterAll(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  document.body.replaceChildren();
  vi.useRealTimers();
});

// The design system's jsdom.ts adds the browser features jsdom leaves out; this adds what only the
// app's tests need.
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
