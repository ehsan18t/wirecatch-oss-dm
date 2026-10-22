import { expect, test, type Locator, type Page } from '@playwright/test';

/** Where the pictures for review go, tracked in the repository. */
export const SCREENSHOTS = 'docs/screenshots';

/** Keeps a picture of the page for review, in a screenshots run only (`pnpm e2e:screenshots`), so running the checks never rewrites the tracked pictures. */
export async function keepScreenshot(page: Page, name: string): Promise<void> {
  if (test.info().config.metadata.screenshots !== true) return;
  await page.screenshot({ path: `${SCREENSHOTS}/${name}` });
}

/**
 * Opens the app with mock options and waits for the list.
 * `textSize` is the root font size in px, as a larger text setting gives it: set before the page
 * loads, as a person's setting is, rather than changed under a drawn page, where every size in
 * rem moves to it through a transition the first measurements can race.
 */
export async function open(
  page: Page,
  query = 'capture=0',
  { theme = 'light', textSize }: { theme?: 'light' | 'dark'; textSize?: number } = {},
): Promise<void> {
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
  if (textSize !== undefined) {
    await page.addInitScript((px) => {
      // The script runs before the page has its root element; the style goes in as soon as it does, first, so a style a test adds later still overrides it.
      const add = (root: HTMLElement) => {
        const style = document.createElement('style');
        style.textContent = `html { font-size: ${String(px)}px !important; }`;
        root.prepend(style);
      };
      new MutationObserver((_, observer) => {
        const root = document.querySelector('html');
        if (!root) return;
        observer.disconnect();
        add(root);
      }).observe(document, { childList: true });
    }, textSize);
  }
  // A fixed sample clock (noon) keeps schedules, and so the screenshots, the same on every run.
  const at = query.includes('at=') ? '' : '&at=12:00';
  await page.goto(`/?${query}${at}`);
  await page.getByRole('navigation', { name: 'Views and queues' }).waitFor();
  // The window draws before the sample backend answers; its badge shows once it has, and with it the palette's sample actions.
  await page.getByRole('banner').getByText('Sample data').waitFor();
  await page.evaluate(() => document.fonts.ready);
  if (textSize !== undefined) {
    const root = await page.evaluate(() => getComputedStyle(document.documentElement).fontSize);
    expect(root, 'the text size set before the page loaded').toBe(`${String(textSize)}px`);
  }
}

/** Waits until `window`, a download window, has kept its height for a few frames: it follows its content a frame behind. */
export async function settled(window: Locator): Promise<void> {
  await window.evaluate(
    (el) =>
      new Promise<void>((resolve) => {
        let last = -1;
        let same = 0;
        const tick = () => {
          const height = el.getBoundingClientRect().height;
          same = height === last ? same + 1 : 0;
          last = height;
          if (same >= 3) resolve();
          else requestAnimationFrame(tick);
        };
        tick();
      }),
  );
}

export function row(page: Page, name: string) {
  return rows(page).filter({ hasText: name }).first();
}

/** The downloads' rows, without the column headers' row. */
export function rows(page: Page) {
  return page.getByRole('grid', { name: 'Downloads' }).locator('[role="row"][data-id]');
}

/** Opens a download's details with a click on its row, as the pointer does. */
export async function showDetails(page: Page, name: string) {
  // The name's cell: the second, after the file's tile, in every view but a queue's own.
  await row(page, name)
    .getByRole('gridcell')
    .nth(1)
    .click({ position: { x: 8, y: 8 } });
  return page.getByRole('dialog', { name: new RegExp(`^${name}`) });
}
