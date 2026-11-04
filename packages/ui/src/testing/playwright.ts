// The extension's browser specs' `test`. It fails a test whose page logged a Svelte warning in development (each links to svelte.dev/e/), or lost a request (a stylesheet that did not load leaves an unstyled page a layout check would pass), and prints what the page reported as broken when a test fails, which a CI log otherwise leaves out.
import { expect, test as base } from '@playwright/test';

const EXPECTED_TYPE: Partial<Record<string, RegExp>> = {
  stylesheet: /^text\/css\b/,
  script: /javascript/,
};

export const test = base.extend<{ pageFaults: undefined }>({
  pageFaults: [
    async ({ page }, use, testInfo) => {
      const warnings: string[] = [];
      const lost: string[] = [];
      const errors: string[] = [];
      // Fetched by Playwright, not the browser: on a Windows runner Edge's own sockets lose a request now and then (net::ERR_NO_BUFFER_SPACE), so a page misses a stylesheet with nothing wrong in it.
      await page.route('http://*/**', async (route) => {
        await route.fulfill({ response: await route.fetch() });
      });
      page.on('console', (message) => {
        const text = message.text();
        if (text.includes('svelte.dev/e/')) warnings.push(text);
        else if (message.type() === 'error') errors.push(`console: ${text}`);
      });
      page.on('pageerror', (error) => errors.push(`page: ${error.stack ?? error.message}`));
      page.on('requestfailed', (request) => {
        lost.push(`${request.url()} (${request.failure()?.errorText ?? 'failed'})`);
      });
      page.on('response', (response) => {
        const type = response.headers()['content-type'] ?? '';
        const wanted = EXPECTED_TYPE[response.request().resourceType()];
        if (response.status() >= 400) lost.push(`${response.url()} (${String(response.status())})`);
        // Vite's server answers a missing file with the page itself, status 200.
        else if (wanted && type !== '' && !wanted.test(type))
          lost.push(`${response.url()} (${type})`);
      });
      await use(undefined);
      // Read first: removing the route fails the requests it still holds, which the test did not lose.
      const lostInTheTest = [...lost];
      // A route still fetching when the page closes would otherwise fail the whole run.
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      if (testInfo.status !== testInfo.expectedStatus && errors.length > 0) {
        console.log(`What the page reported in "${testInfo.title}":\n${errors.join('\n')}`);
      }
      expect(warnings, 'Svelte warnings in development').toEqual([]);
      expect(lostInTheTest, 'requests the page lost').toEqual([]);
    },
    { auto: true },
  ],
});
