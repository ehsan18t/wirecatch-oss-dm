/** The specs' shared `test` itself (`@wirecatch/ui/testing/playwright`). */
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { test } from '@wirecatch/ui/testing/playwright';

let silent: Server;
let asked: Promise<void>;

test.beforeAll(async () => {
  let arrived = (): void => undefined;
  asked = new Promise((resolve) => {
    arrived = resolve;
  });
  silent = createServer(() => {
    arrived();
  });
  await new Promise<void>((resolve) => {
    silent.listen(0, '127.0.0.1', resolve);
  });
});

test.afterAll(() => {
  silent.closeAllConnections();
  silent.close();
});

test('a test may end with a request still being fetched', async ({ page }) => {
  await page.goto('/?state=connected');
  await page.locator('main').waitFor();
  const { port } = silent.address() as AddressInfo;
  await page.evaluate(
    (url) => {
      void fetch(url, { mode: 'no-cors' });
    },
    `http://127.0.0.1:${String(port)}/never-answered`,
  );
  await asked;
});
