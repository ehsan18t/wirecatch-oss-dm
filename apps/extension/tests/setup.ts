// The design system's jsdom.ts adds the browser features jsdom leaves out that its components use
// (native dialogs, popovers); this adds what only the extension's page tests need.
import '@wirecatch/ui/testing/jsdom';
import { vi } from 'vitest';

// The test environment copies jsdom's window keys onto the global, `matchMedia` among them with no
// value, so the check is for a function. No query matches: full motion, the light scheme.
if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
  Object.assign(window, {
    matchMedia: (query: string) => ({
      matches: false,
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
