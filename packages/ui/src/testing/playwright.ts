// The browser specs' `test`, for both apps: it fails a test whose page logged a Svelte warning in development (each links to svelte.dev/e/), as each marks a fault the page lived through.
import { expect, test as base } from '@playwright/test';

export const test = base.extend<{ svelteWarnings: undefined }>({
  svelteWarnings: [
    async ({ page }, use) => {
      const warnings: string[] = [];
      page.on('console', (message) => {
        if (message.text().includes('svelte.dev/e/')) warnings.push(message.text());
      });
      await use(undefined);
      expect(warnings, 'Svelte warnings in development').toEqual([]);
    },
    { auto: true },
  ],
});
