import { defineConfig } from '@playwright/test';
import checks from './playwright.config';

// Takes the pictures in docs/screenshots for review: the main states (screenshots.spec.ts) and the layout at each window size (layout.spec.ts, which runs its checks as well). Run with `pnpm e2e:screenshots`.
export default defineConfig({
  ...checks,
  testIgnore: undefined,
  testMatch: ['screenshots.spec.ts', 'layout.spec.ts'],
  metadata: { screenshots: true },
});
