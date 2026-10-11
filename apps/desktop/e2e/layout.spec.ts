// Review blocker 1: the window works at 960x600, 1280x800, 1440x900 and 1600x1000, in both themes. Names
// keep real room, the toolbar stays on one line, nothing scrolls sideways, and the detail panel
// floats over the list in narrower windows.

import { expect, test, type Page } from '@playwright/test';
import { open, row, rows } from './helpers';

const SIZES = [
  { width: 960, height: 600 },
  { width: 1280, height: 800 },
  { width: 1440, height: 900 },
  { width: 1600, height: 1000 },
] as const;

/** From this window width the detail panel docks beside the list (`DOCK_WIDTH` in Shell). */
const DOCK_WIDTH = 1152;

/** With the panel docked, Name, Size, Done and Status stay in view, left of the panel. */
async function checkColumnsBesidePanel(page: Page) {
  const panel = await page.getByRole('complementary', { name: 'Selected download' }).boundingBox();
  expect(panel, 'panel shown').not.toBeNull();
  for (const name of ['Name', 'Size', 'Done', 'Status']) {
    const cell = await page
      .locator('.header span', { hasText: new RegExp(`^${name}$`) })
      .boundingBox();
    expect(cell, `${name} column shown`).not.toBeNull();
    const right = (cell?.x ?? 0) + (cell?.width ?? 0);
    expect(right, `${name} column left of the panel`).toBeLessThanOrEqual((panel?.x ?? 0) + 1);
  }
}

async function checkLayout(page: Page) {
  const header = page.locator('.header span', { hasText: /^Name$/ });
  const nameHeader = await header.boundingBox();
  expect(nameHeader?.width ?? 0, 'Name column width').toBeGreaterThanOrEqual(160);
  const firstName = await rows(page).first().locator('.name').boundingBox();
  expect(firstName?.width ?? 0, 'first row name width').toBeGreaterThanOrEqual(160);

  const tools = await page.locator('.toolbar .tools > *').evaluateAll((els) =>
    els.map((e) => {
      const r = e.getBoundingClientRect();
      return { top: Math.round(r.top + r.height / 2), height: Math.round(r.height) };
    }),
  );
  const centers = new Set(tools.map((t) => t.top));
  expect(Math.max(...centers) - Math.min(...centers), 'toolbar on one line').toBeLessThanOrEqual(2);
  const add = await page.getByRole('button', { name: 'Add download' }).boundingBox();
  expect(add?.height ?? 99, 'Add download on one line').toBeLessThanOrEqual(32);

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow, 'no sideways scrolling').toBeLessThanOrEqual(0);
}

for (const theme of ['light', 'dark'] as const) {
  for (const size of SIZES) {
    test(`${size.width}x${size.height}, ${theme} theme`, async ({ page }) => {
      await page.setViewportSize(size);
      await open(page, 'capture=0', { theme });
      await checkLayout(page);
      await page.screenshot({
        path: `docs/screenshots/${theme}-layout-${size.width}x${size.height}.png`,
      });

      await row(page, 'Conference keynote').click();
      const panel = page.getByRole('complementary', { name: 'Selected download' });
      await expect(panel).toBeVisible();
      const docked = await panel.evaluate((el) => !el.classList.contains('overlay'));
      expect(docked, 'docked whenever there is room').toBe(size.width >= DOCK_WIDTH);
      await checkLayout(page);
      if (docked) await checkColumnsBesidePanel(page);
      await page.screenshot({
        path: `docs/screenshots/${theme}-layout-${size.width}x${size.height}-selected.png`,
      });

      if (!docked) {
        await page.getByRole('button', { name: 'Close details and clear the selection' }).click();
        await expect(panel).toBeHidden();
      }
    });

    test(`${size.width}x${size.height}, ${theme} theme: queue actions on one line, labels shown`, async ({
      page,
    }) => {
      await page.setViewportSize(size);
      await open(page, 'capture=0', { theme });
      // Overnight is outside its hours at noon, so all three queue actions show.
      await page.getByRole('button', { name: /^Overnight/ }).click();
      const actions = page.getByRole('group', { name: 'This queue' }).getByRole('button');
      await expect(actions).toHaveCount(3);
      const centers = await actions.evaluateAll((els) =>
        els.map((e) => {
          const r = e.getBoundingClientRect();
          return Math.round(r.top + r.height / 2);
        }),
      );
      expect(Math.max(...centers) - Math.min(...centers), 'queue actions on one line').toBe(0);
      // There is room for every toolbar label at these sizes, so none is reduced to an icon.
      const labels = await page
        .locator('.toolbar .label')
        .evaluateAll((els) => els.map((e) => e.getBoundingClientRect().width));
      expect(labels.length).toBeGreaterThan(0);
      for (const w of labels) expect(w, 'toolbar label shown').toBeGreaterThan(20);
      await checkLayout(page);
    });
  }
}
