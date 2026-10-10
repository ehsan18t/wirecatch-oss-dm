import { defineConfig } from '@playwright/test';

// Runs the interface against the mock backend in Microsoft Edge (installed with Windows), so no
// browser download is needed. On Linux, set PW_CHANNEL=chromium after `playwright install`.
export default defineConfig({
  testDir: 'e2e',
  timeout: 90_000,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: 'http://localhost:1420',
    channel: process.env.PW_CHANNEL ?? 'msedge',
    viewport: { width: 1440, height: 880 },
  },
  // PW_SERVER=preview measures the production build (run `pnpm build` first); outside Tauri it
  // uses the mock backend too.
  webServer: {
    command:
      process.env.PW_SERVER === 'preview'
        ? 'pnpm exec vite preview --port 1420 --strictPort'
        : 'pnpm exec vite dev --mode mock --port 1420 --strictPort',
    url: 'http://localhost:1420',
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
