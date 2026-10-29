import { defineConfig } from '@playwright/test';

// Lays the extension's pages out in a real browser, which jsdom cannot: the preview page
// (`preview/`, sample data, every word pseudo-translated longer), built and served on a port of its own.
// Run with `pnpm e2e`; the gate runs it as `e2e-extension`.
const PORT = 4329;
const OUT = '../.output/e2e-preview';
// An address, not `localhost`: on Windows that name is both `::1` and `127.0.0.1`, and the server
// listens on one, so a request the browser sends to the other is refused and the page never draws.
const HOST = '127.0.0.1';
const ORIGIN = `http://${HOST}:${String(PORT)}`;

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  fullyParallel: true,
  reporter: 'list',
  use: {
    baseURL: ORIGIN,
    // The same browser as the desktop's checks: Edge, installed with Windows, unless PW_CHANNEL names another.
    channel: process.env.PW_CHANNEL ?? 'msedge',
  },
  webServer: {
    command: `pnpm exec vite build --config preview/vite.config.ts --outDir ${OUT} --emptyOutDir && pnpm exec vite preview --config preview/vite.config.ts --outDir ${OUT} --host ${HOST} --port ${String(PORT)} --strictPort`,
    url: `${ORIGIN}/`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    // The server's own lines, with the address it listens on, go to the log.
    stdout: 'pipe',
    timeout: 120_000,
  },
});
