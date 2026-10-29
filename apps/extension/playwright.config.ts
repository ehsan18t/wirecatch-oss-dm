import { defineConfig } from '@playwright/test';

// Lays the extension's pages out in a real browser, which jsdom cannot: the preview page
// (`preview/`, sample data, every word pseudo-translated longer), built and served on a port of its own.
// Built, not the dev server: on a busy Windows runner the dev server refused one of its hundreds of
// module requests, and a page whose module fails to load never draws. Run with `pnpm e2e`; the gate runs it as `e2e-extension`.
const PORT = 4329;
const OUT = '../.output/e2e-preview';

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  fullyParallel: true,
  reporter: 'list',
  use: {
    baseURL: `http://localhost:${String(PORT)}`,
    // The same browser as the desktop's checks: Edge, installed with Windows, unless PW_CHANNEL names another.
    channel: process.env.PW_CHANNEL ?? 'msedge',
  },
  webServer: {
    command: `pnpm exec vite build --config preview/vite.config.ts --outDir ${OUT} --emptyOutDir && pnpm exec vite preview --config preview/vite.config.ts --outDir ${OUT} --port ${String(PORT)} --strictPort`,
    url: `http://localhost:${String(PORT)}/`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    timeout: 120_000,
  },
});
