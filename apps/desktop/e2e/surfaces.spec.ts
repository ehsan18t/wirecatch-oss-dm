// What floats over the window is solid, and what lays out a page leaves no empty room: every dialog (the design system's and the older ones), the command palette and the menus have an opaque surface, since the one blur behind a dialog must never show through it (D46); and every settings row is as tall as what it holds, in each section.

import { expect, test, type Locator, type Page } from '@playwright/test';
import { open, row, showDetails } from './helpers';

/** The alpha of an element's own background colour, from 0 to 1. */
function backgroundAlpha(target: Locator): Promise<number> {
  return target.evaluate((el) => {
    // `rgb(r, g, b)` or `rgba(r, g, b, a)`, as browsers compute a colour.
    const parts =
      /\(([^)]*)\)/.exec(getComputedStyle(el).backgroundColor)?.[1]?.split(/[\s,/]+/) ?? [];
    return parts.length > 3 ? Number(parts[3]) : 1;
  });
}

/** The surface of the dialog on top: its panel, the element that holds its title. */
function topDialog(page: Page): Locator {
  return page.locator('dialog[open]').last().locator('section').first();
}

async function expectSolid(target: Locator, what: string) {
  await expect(target, what).toBeVisible();
  expect(await backgroundAlpha(target), `${what} is opaque`).toBe(1);
}

for (const theme of ['light', 'dark'] as const) {
  test(`dialogs, the palette and menus are solid, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });

    await showDetails(page, 'LibreOffice');
    await expectSolid(topDialog(page), 'the details');
    await page.locator('dialog[open]').getByRole('button', { name: 'Remove' }).click();
    await expectSolid(topDialog(page), 'the remove question');
    await page.keyboard.press('Escape');
    await page.locator('dialog[open]').getByRole('button', { name: 'Cancel', exact: true }).click();
    await expectSolid(topDialog(page), 'the cancel question');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');

    await page.keyboard.press('Control+N');
    await expectSolid(topDialog(page), 'New download');
    await page.keyboard.press('Escape');

    await page.getByRole('button', { name: 'New queue' }).click();
    await expectSolid(topDialog(page), 'New queue');
    await page.keyboard.press('Escape');

    await page.keyboard.press('F1');
    await expectSolid(topDialog(page), 'the shortcuts');
    await page.keyboard.press('Escape');

    await page.keyboard.press('Control+Shift+P');
    await expectSolid(page.locator('dialog.palette[open]'), 'the command palette');
    await page.keyboard.press('Escape');

    await row(page, 'Distributed').click({ button: 'right' });
    await expectSolid(page.getByRole('menu'), 'the context menu');
    await page.keyboard.press('Escape');
  });
}

test('every settings row is as tall as what it holds, in each section', async ({ page }) => {
  await open(page, 'capture=0');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sections = page.getByRole('navigation', { name: 'Settings sections' });
  const names = (await sections.getByRole('button').allInnerTexts())
    .map((t) => t.trim())
    .filter((t) => t !== '' && t !== 'Back to downloads');
  expect(names.length).toBeGreaterThan(5);
  for (const name of names) {
    await sections.getByRole('button', { name: new RegExp(`^${name}`) }).click();
    const slack = await page.locator('.setting').evaluateAll((rows) =>
      rows.map((r) => {
        // Each part of the row (its name and help, its control) is no taller than what is in it.
        const parts = [...r.children].map((part) => {
          const kids = [...part.children].map((k) => k.getBoundingClientRect());
          if (kids.length === 0) return 0;
          const top = Math.min(...kids.map((k) => k.top));
          const bottom = Math.max(...kids.map((k) => k.bottom));
          return part.getBoundingClientRect().height - (bottom - top);
        });
        return { id: r.id, slack: Math.max(...parts) };
      }),
    );
    for (const { id, slack: room } of slack) {
      expect(room, `${name}: ${id} has empty room`).toBeLessThanOrEqual(4);
    }
  }
});
