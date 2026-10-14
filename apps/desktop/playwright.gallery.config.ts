import { defineConfig } from '@playwright/test';

// Checks the component gallery as a production build makes it (`pnpm build:gallery`, which the `e2e:gallery` script runs first), served by `vite preview` on its own port: the build lowers and minifies CSS in ways the dev server does not, and jsdom computes no styles. Run with `pnpm e2e:gallery`.
const PORT = 4319;

export default defineConfig({
  testDir: 'e2e',
  testMatch: ['gallery.spec.ts'],
  timeout: 90_000,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    channel: process.env.PW_CHANNEL ?? 'msedge',
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: `pnpm exec vite preview --mode gallery --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}/gallery/`,
    reuseExistingServer: process.env.PW_REUSE === '1',
    timeout: 120_000,
  },
});
