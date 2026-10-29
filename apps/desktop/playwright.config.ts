import { defineConfig } from '@playwright/test';

// Runs the interface against the mock backend in Microsoft Edge (installed with Windows), so no
// browser download is needed. On Linux, set PW_CHANNEL=chromium after `playwright install`.
//
// Against the mock build (`pnpm build:mock`, run first by `pnpm e2e`) under `vite preview`, not the
// dev server: its hundreds of module requests per page fail under load and leave a page unrendered.
//
// Playwright starts its own server on a port of its own, so it never talks to a stray server.
// Set PW_REUSE=1 to reuse one that is already running on that port.
//
// The checks the gate runs (`pnpm e2e`): accessibility and layout. They write nothing to the
// tree; `pnpm e2e:screenshots` (playwright.screenshots.config.ts) takes the pictures for review.
const PORT = 4317;

export default defineConfig({
  testDir: 'e2e',
  testIgnore: ['performance.spec.ts', 'gallery.spec.ts', 'screenshots.spec.ts'],
  timeout: 90_000,
  // Each test has a browser context of its own and the mock backend lives in the page, so tests share nothing.
  fullyParallel: true,
  reporter: 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    channel: process.env.PW_CHANNEL ?? 'msedge',
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: `pnpm exec vite preview --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    timeout: 120_000,
  },
});
