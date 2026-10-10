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
  await page.goto(`/?${query}`);
  await page.getByRole('navigation', { name: 'Views and queues' }).waitFor();
  await page.evaluate(() => document.fonts.ready);
}

export function row(page: Page, name: string) {
  return rows(page).filter({ hasText: name }).first();
}

export function rows(page: Page) {
  return page.getByRole('listbox', { name: 'Downloads' }).getByRole('option');
}
