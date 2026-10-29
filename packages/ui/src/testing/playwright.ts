// The browser specs' `test`, for both apps: it fails a test whose page logged a Svelte warning in development (each links to svelte.dev/e/), as each marks a fault the page lived through, and prints what the page reported as broken when a test fails, which a CI log otherwise leaves out.
import { expect, test as base } from '@playwright/test';

export const test = base.extend<{ svelteWarnings: undefined }>({
  svelteWarnings: [
    async ({ page }, use, testInfo) => {
      const warnings: string[] = [];
      const errors: string[] = [];
      page.on('console', (message) => {
        const text = message.text();
        if (text.includes('svelte.dev/e/')) warnings.push(text);
        else if (message.type() === 'error') errors.push(`console: ${text}`);
      });
      page.on('pageerror', (error) => errors.push(`page: ${error.stack ?? error.message}`));
      page.on('requestfailed', (request) => {
        errors.push(`request: ${request.url()} (${request.failure()?.errorText ?? 'failed'})`);
      });
      await use(undefined);
      if (testInfo.status !== testInfo.expectedStatus && errors.length > 0) {
        console.log(`What the page reported in "${testInfo.title}":\n${errors.join('\n')}`);
      }
      expect(warnings, 'Svelte warnings in development').toEqual([]);
    },
    { auto: true },
  ],
});
