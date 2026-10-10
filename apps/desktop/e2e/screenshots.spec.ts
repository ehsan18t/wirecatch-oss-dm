// Screenshots of the main states for review, in both themes. Saved to docs/screenshots.

import { test } from '@playwright/test';
import { open, row } from './helpers';

const dir = 'docs/screenshots';

for (const theme of ['light', 'dark'] as const) {
  test.describe(`${theme} theme`, () => {
    test('main list', async ({ page }) => {
      await open(page, 'capture=0', { theme, hints: true });
      await page.waitForTimeout(1500);
      await page.screenshot({ path: `${dir}/${theme}-01-main-list.png` });
    });

    test('error with its fix', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await row(page, 'Conference keynote').click();
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${dir}/${theme}-02-error-with-fix.png` });
    });

    test('command palette', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await row(page, 'ubuntu-24.04.3').click();
      await page.keyboard.press('Control+K');
      await page.keyboard.type('pause');
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-03-command-palette.png` });
    });

    test('settings', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.getByRole('button', { name: /^Settings/ }).click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-04-settings.png` });
      await page.getByRole('searchbox', { name: 'Search settings' }).fill('speed');
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${dir}/${theme}-05-settings-search.png` });
    });

    test('capture toast', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+K');
      await page.keyboard.type('pretend the browser sent a download');
      await page.keyboard.press('Enter');
      await page.waitForTimeout(1500);
      await page.screenshot({ path: `${dir}/${theme}-06-capture-toast.png` });
    });

    test('ask before download confirmation', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+K');
      await page.keyboard.type('asks first');
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
      await page.screenshot({ path: `${dir}/${theme}-07-confirmation.png` });
    });

    test('add from a pasted link', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.evaluate(() => {
        const e = new Event('paste', { bubbles: true }) as Event & { clipboardData: unknown };
        Object.defineProperty(e, 'clipboardData', {
          value: { getData: () => 'https://download.example.org/releases/app-2.4.1-x64.msi' },
        });
        document.body.dispatchEvent(e);
      });
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${dir}/${theme}-08-add-from-paste.png` });
    });

    test('queue editor', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.getByRole('button', { name: 'Edit Overnight: name, schedule and limits' }).click();
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${dir}/${theme}-09-queue-editor.png` });
    });

    test('first run', async ({ page }) => {
      await open(page, 'empty&capture=0', { theme });
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-10-first-run.png` });
    });

    test('ten thousand downloads', async ({ page }) => {
      await open(page, 'count=10000&active=60&capture=0', { theme });
      await page.waitForTimeout(1500);
      await page.screenshot({ path: `${dir}/${theme}-11-ten-thousand.png` });
    });
  });
}
