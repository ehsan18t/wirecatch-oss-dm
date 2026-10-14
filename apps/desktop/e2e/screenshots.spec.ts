// Screenshots of the main states for review, in both themes. Saved to docs/screenshots.

import { expect, test } from '@playwright/test';
import { open, row, rows, SCREENSHOTS, settled, showDetails } from './helpers';

const dir = SCREENSHOTS;

for (const theme of ['light', 'dark'] as const) {
  test.describe(`${theme} theme`, () => {
    test('main list', async ({ page }) => {
      await open(page, 'capture=0', { theme, hints: true });
      await page.waitForTimeout(1500);
      await page.screenshot({ path: `${dir}/${theme}-01-main-list.png` });
    });

    test('error with its fix', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await showDetails(page, 'Conference keynote');
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

    test('download window progress', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+K');
      await page.keyboard.type('asks first');
      await page.keyboard.press('Enter');
      const window = page.getByRole('region', { name: /^Wirecatch download window/ });
      await window.getByRole('button', { name: 'Download now' }).click();
      await window.getByRole('progressbar').waitFor();
      // Once it says where it stands in its queue, and the window is as tall as what it shows.
      await window.getByText(/in line in Main/).waitFor();
      await settled(window);
      await page.screenshot({ path: `${dir}/${theme}-23-download-window-progress.png` });
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

    test('add several links, one line left out, a folder chosen', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+N');
      const dialog = page.getByRole('dialog', { name: /^Add/ });
      await dialog
        .getByRole('textbox', { name: 'Links' })
        .fill(
          [
            'https://download.example.org/releases/app-2.4.1-x64.msi',
            'https://cdn.example.net/media/holiday-video-2026-final-cut.mp4',
            'from the release notes',
            'https://files.example.com/archive/backup-october.zip',
          ].join('\n'),
        );
      await dialog.getByRole('button', { name: 'Change' }).click();
      await dialog.getByRole('button', { name: 'Use default' }).waitFor();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-21-add-several-links.png` });
    });

    test('add with no link', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+N');
      const dialog = page.getByRole('dialog', { name: /^Add/ });
      await dialog.getByRole('textbox', { name: 'Links' }).fill('my notes');
      await dialog.getByRole('button', { name: 'Download' }).click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-22-add-no-link.png` });
    });

    test('queue editor', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.getByRole('button', { name: /^Overnight/ }).hover();
      await page.getByRole('button', { name: 'Edit Overnight: name, schedule and limits' }).click();
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${dir}/${theme}-09-queue-editor.png` });
    });

    test('right-click menu', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await row(page, 'LibreOffice').click({ button: 'right' });
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${dir}/${theme}-12-context-menu.png` });
    });

    test('cancel asks first', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      const details = await showDetails(page, 'LibreOffice');
      await details.getByRole('button', { name: 'Cancel and discard the partial file' }).click();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-13-cancel-confirmation.png` });
    });

    test('paused queue and start now', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.getByRole('button', { name: /^Main/ }).hover();
      await page.getByRole('button', { name: 'Pause the Main queue' }).click();
      await page.getByRole('button', { name: /^Overnight/ }).click();
      await page.waitForTimeout(800);
      await page.screenshot({ path: `${dir}/${theme}-14-paused-queue-and-start-now.png` });
    });

    test('quality picker', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+K');
      await page.keyboard.type('several qualities');
      await page.keyboard.press('Enter');
      await page.getByRole('heading', { name: 'Pick a quality' }).waitFor();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-15-quality-picker.png` });
    });

    test('add-on prompt and install progress', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.keyboard.press('Control+K');
      await page.keyboard.type('needs the media add-on');
      await page.keyboard.press('Enter');
      await page.getByRole('button', { name: 'Install', exact: true }).waitFor();
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-16-addon-prompt.png` });
      await page.getByRole('button', { name: 'Install', exact: true }).click();
      await page
        .getByRole('region', { name: /^Wirecatch download window/ })
        .getByRole('progressbar')
        .waitFor();
      await page.waitForTimeout(1200);
      await page.screenshot({ path: `${dir}/${theme}-17-addon-installing.png` });
      // Once installed, the page that asked for it continues with the quality list, in its window.
      await page.getByRole('heading', { name: 'Pick a quality' }).waitFor({ timeout: 15_000 });
    });

    test('add-ons in settings', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      await page.getByRole('button', { name: /^Settings/ }).click();
      await page.getByRole('searchbox', { name: 'Search settings' }).fill('add-on');
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${dir}/${theme}-18-addons-settings.png` });
    });

    test('joining video and audio', async ({ page }) => {
      await open(page, 'capture=0', { theme });
      // Room in the queue, so the video starts at once.
      await page.getByRole('button', { name: 'Pause all' }).click();
      await page.keyboard.press('Control+K');
      await page.keyboard.type('several qualities');
      await page.keyboard.press('Enter');
      // 1080p comes as separate video and audio, so it is joined after downloading. That needs
      // the media add-on: it installs first, then the video downloads by itself.
      await page.getByRole('radio', { name: /1080p/ }).check();
      await page.getByRole('button', { name: 'Install add-on and download' }).click();
      await page
        .getByRole('region', { name: /^Wirecatch download window/ })
        .getByRole('progressbar')
        .waitFor();
      await page.waitForTimeout(1200);
      await page.screenshot({ path: `${dir}/${theme}-20-addon-for-a-quality.png` });
      await page.getByRole('dialog').waitFor({ state: 'hidden', timeout: 15_000 });
      await page.getByRole('searchbox', { name: 'Search downloads' }).fill('Northern lights');
      // The new download, not the sample one of the same video that failed earlier.
      const video = rows(page)
        .filter({ hasText: 'Northern lights' })
        .filter({ hasNotText: 'Needs attention' });
      await video.click();
      await expect(video).toContainText('Joining video and audio', { timeout: 60_000 });
      await page.screenshot({ path: `${dir}/${theme}-19-joining.png` });
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
