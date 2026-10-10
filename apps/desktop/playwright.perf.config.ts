import { defineConfig } from '@playwright/test';

// Measures the production build made with the mock backend (`pnpm build:mock`), served by
// `vite preview` on its own port. Run with `pnpm e2e:perf`.
const PORT = 4318;

export default defineConfig({
  testDir: 'e2e',
  testMatch: ['performance.spec.ts'],
  timeout: 90_000,
  workers: 1,
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
