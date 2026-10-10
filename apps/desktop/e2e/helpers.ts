import type { Page } from '@playwright/test';

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
  await page.evaluate(() => document.fonts.ready);
}

export function row(page: Page, name: string) {
  return rows(page).filter({ hasText: name }).first();
}

export function rows(page: Page) {
  return page.getByRole('grid', { name: 'Downloads' }).getByRole('row');
}
