import { test, type Page } from '@playwright/test';

/** Where the pictures for review go, tracked in the repository. */
export const SCREENSHOTS = 'docs/screenshots';

/** Keeps a picture of the page for review, in a screenshots run only (`pnpm e2e:screenshots`), so running the checks never rewrites the tracked pictures. */
export async function keepScreenshot(page: Page, name: string): Promise<void> {
  if (test.info().config.metadata.screenshots !== true) return;
  await page.screenshot({ path: `${SCREENSHOTS}/${name}` });
}

/** Opens the app with mock options and waits for the list. Hints start dismissed unless asked. */
export async function open(
  page: Page,
  query = 'capture=0',
  { theme = 'light', hints = false }: { theme?: 'light' | 'dark'; hints?: boolean } = {},
): Promise<void> {
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
  await page.addInitScript((showHints) => {
    if (!showHints) {
      localStorage.setItem(
        'wirecatch.hints.dismissed',
        JSON.stringify(['palette', 'drag-to-queue', 'reorder', 'paste']),
      );
    } else {
      localStorage.removeItem('wirecatch.hints.dismissed');
    }
  }, hints);
  // A fixed sample clock (noon) keeps schedules, and so the screenshots, the same on every run.
  const at = query.includes('at=') ? '' : '&at=12:00';
  await page.goto(`/?${query}${at}`);
  await page.getByRole('navigation', { name: 'Views and queues' }).waitFor();
  // The window draws before the sample backend answers; its badge shows once it has, and with it the palette's sample actions.
  await page.getByRole('banner').getByText('Sample data').waitFor();
  await page.evaluate(() => document.fonts.ready);
}

export function row(page: Page, name: string) {
  return rows(page).filter({ hasText: name }).first();
}

/** The downloads' rows, without the column headers' row. */
export function rows(page: Page) {
  return page.getByRole('grid', { name: 'Downloads' }).locator('[role="row"][data-id]');
}

/** Opens a download's details with the row's own button, as the pointer does. */
export async function showDetails(page: Page, name: string) {
  await row(page, name).hover();
  await row(page, name).getByRole('button', { name: 'Show details' }).click();
  return page.getByRole('dialog', { name: new RegExp(`^${name}`) });
}
