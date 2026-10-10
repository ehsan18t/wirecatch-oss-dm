import { defineConfig } from '@playwright/test';

// Runs the interface against the mock backend in Microsoft Edge (installed with Windows), so no
// browser download is needed. On Linux, set PW_CHANNEL=chromium after `playwright install`.
//
// Playwright starts its own server on a port of its own, so it never talks to a stray dev
// server. Set PW_REUSE=1 to reuse one that is already running on that port.
const PORT = 4317;

export default defineConfig({
  testDir: 'e2e',
  testIgnore: ['performance.spec.ts'],
  timeout: 90_000,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    channel: process.env.PW_CHANNEL ?? 'msedge',
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: `pnpm exec vite dev --mode mock --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    timeout: 120_000,
  },
});
