import { defineConfig } from '@playwright/test';

// Lays the extension's pages out in a real browser, which jsdom cannot: the preview page
// (`preview/`, sample data, every word pseudo-translated longer) served by its Vite dev server on
// a port of its own. Run with `pnpm e2e`; the gate runs it as `e2e-extension`.
const PORT = 4329;

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: `http://localhost:${String(PORT)}`,
    // The same browser as the desktop's checks: Edge, installed with Windows, unless PW_CHANNEL names another.
    channel: process.env.PW_CHANNEL ?? 'msedge',
  },
  webServer: {
    command: `pnpm exec vite --config preview/vite.config.ts --port ${String(PORT)} --strictPort`,
    url: `http://localhost:${String(PORT)}/`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    timeout: 120_000,
  },
});
