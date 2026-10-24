// The main window works at 960x600, 1280x800, 1440x900 and 1600x1000, in both themes: names keep real room, the tools above the list stay on one line, nothing scrolls sideways, the navigation is a rail in a narrow window and docks with its labels in a wide one, a download's details open in a dialog over the list at every size (D47), and a row's buttons have a column of their own rather than lying over its figures.

import { expect, test, type Page } from '@playwright/test';
import { keepScreenshot, open, row, rows, showDetails } from './helpers';

const SIZES = [
  { width: 960, height: 600 },
  { width: 1280, height: 800 },
  { width: 1440, height: 900 },
  { width: 1600, height: 1000 },
] as const;

/** From this window width the navigation docks with its labels (`--breakpoint-nav`, 63rem). */
const NAV_WIDTH = 1008;

/** Whether the elements' vertical middles are all on one line. */
async function oneLine(page: Page, selector: ReturnType<Page['locator']>) {
  const centers = await selector.evaluateAll((els) =>
    els.map((e) => {
      const r = e.getBoundingClientRect();
      return Math.round(r.top + r.height / 2);
    }),
  );
  return Math.max(...centers) - Math.min(...centers);
}

async function checkLayout(page: Page) {
  const nameHeader = await page.getByRole('columnheader', { name: 'Name' }).boundingBox();
  expect(nameHeader?.width ?? 0, 'Name column width').toBeGreaterThanOrEqual(160);
  // The name's column by its heading: a queue's own view has "#" before it, the others do not.
  const headings = await page.getByRole('columnheader').allInnerTexts();
  const nameColumn = headings.findIndex((h) => h.trim() === 'Name');
  const firstName = await rows(page).first().getByRole('gridcell').nth(nameColumn).boundingBox();
  expect(firstName?.width ?? 0, 'first row name width').toBeGreaterThanOrEqual(160);

  const tools = page.getByRole('group', { name: 'List tools' }).locator('> *');
  expect(await oneLine(page, tools), 'tools on one line').toBeLessThanOrEqual(2);
  const add = await page.getByRole('button', { name: 'New download' }).boundingBox();
  expect(add?.height ?? 99, 'New download on one line').toBeLessThanOrEqual(40);

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow, 'no sideways scrolling').toBeLessThanOrEqual(0);
}

/** The details open over the list in a dialog, their actions on one line; Escape gives the list back with the row still selected. */
async function checkDetails(page: Page) {
  const details = await showDetails(page, 'Conference keynote');
  await expect(details).toBeVisible();
  await expect(page.locator('dialog[aria-modal="true"]')).toBeVisible();
  const main = await page.getByRole('main', { name: 'Downloads' }).boundingBox();
  const panel = await details.locator('section').first().boundingBox();
  expect(panel?.width ?? 0, 'the details within the window').toBeLessThan(main?.width ?? 0);
  const footer = details.locator('footer').locator('button');
  expect(await oneLine(page, footer), 'the details footer on one line').toBeLessThanOrEqual(2);
  await page.keyboard.press('Escape');
  await expect(details).toBeHidden();
  await expect(row(page, 'Conference keynote')).toHaveAttribute('aria-selected', 'true');
}

/** The text of every cell of the row that lies under its buttons and still shows. */
function figuresUnderButtons(target: ReturnType<Page['locator']>) {
  return target.evaluate((el) => {
    const pill = el.querySelector('button[aria-label="More actions"]')?.parentElement;
    if (!pill) return ['no buttons'];
    const box = pill.getBoundingClientRect();
    return [...el.querySelectorAll('[role="gridcell"]')]
      .filter((cell) => !cell.contains(pill) && cell.textContent.trim() !== '')
      .filter((cell) => {
        const r = cell.getBoundingClientRect();
        const overlaps = r.left < box.right && r.right > box.left && r.width > 0;
        return overlaps && getComputedStyle(cell).opacity !== '0';
      })
      .map((cell) => cell.textContent.trim());
  });
}

for (const size of [SIZES[1], SIZES[2]]) {
  test(`${size.width}x${size.height}: a row's buttons have their own column, beside its figures`, async ({
    page,
  }) => {
    await page.setViewportSize(size);
    await open(page, 'capture=0');
    const target = row(page, 'ubuntu-24');
    await target.hover();
    await expect.poll(() => figuresUnderButtons(target), 'figures under the buttons').toEqual([]);
    await expect(target.getByRole('button', { name: 'Pause' })).toBeVisible();
    await expect(target.getByRole('gridcell').nth(4), 'the speed still shows').toContainText(
      /B\/s/,
    );
  });
}

/** Root sizes at display scales: the browser's default, and ones a desktop's default font size or a browser's minimum gives, whose rows and list end fall between pixels. */
const TEXT_SIZES = [
  { root: 16, scale: 1, height: 900 },
  { root: 20, scale: 1, height: 900 },
  { root: 16, scale: 1.25, height: 777 },
  { root: 17, scale: 1.25, height: 777 },
  { root: 22, scale: 1.5, height: 601 },
] as const;

/** Opens the list with the root at `root` px and waits for the rows to take their height from it (`--spacing-row`, in rem). */
async function withText(page: Page, root: number) {
  await open(page, 'capture=0', { textSize: root });
  const token = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--spacing-row').trim(),
  );
  const rem = /^(\d+(?:\.\d+)?)rem$/.exec(token)?.[1];
  if (rem === undefined) throw new Error(`--spacing-row is not in rem: ${token}`);
  const grid = page.getByRole('grid', { name: 'Downloads' });
  // The page's own stylesheets arrive as modules, a moment after the list is drawn.
  await expect
    .poll(
      () =>
        rows(page)
          .first()
          .evaluate((el) => el.getBoundingClientRect().height),
      'rows follow the text',
    )
    .toBe(Number(rem) * root);
  return grid;
}

/** Waits until the list has not scrolled for a while, so a scroll the browser makes after ours is seen. */
async function settled(grid: ReturnType<Page['locator']>) {
  await grid.evaluate(
    (el) =>
      new Promise<void>((resolve) => {
        let quiet = setTimeout(done, 400);
        const cap = setTimeout(done, 5000);
        function moved() {
          clearTimeout(quiet);
          quiet = setTimeout(done, 400);
        }
        function done() {
          clearTimeout(quiet);
          clearTimeout(cap);
          el.removeEventListener('scroll', moved);
          resolve();
        }
        el.addEventListener('scroll', moved);
      }),
  );
}

/** The selected row is the last download, drawn, and shown whole; only folded groups' headings may follow it. */
async function showsLast(page: Page, grid: ReturnType<Page['locator']>) {
  await settled(grid);
  const active = await grid.getAttribute('aria-activedescendant');
  expect(active, 'the selected row is drawn').not.toBeNull();
  const last = page.locator(`#${active}`);
  expect(await rows(page).last().getAttribute('id'), 'the last download').toBe(active);
  await expect(last).toBeInViewport({ ratio: 1 });
}

for (const { root, scale, height } of TEXT_SIZES) {
  test.describe(`at ${scale * 100}% display scale with ${root}px text`, () => {
    test.use({ deviceScaleFactor: scale, viewport: { width: 1440, height } });

    test('rows are placed where they are drawn', async ({ page }) => {
      const grid = await withText(page, root);
      const fits = await grid.evaluate((el) => {
        const [header, area] = el.querySelectorAll(':scope > [role="rowgroup"]');
        const drawn = area?.querySelector(':scope > .absolute.inset-x-0');
        if (!header || !area || !drawn) return 'no rows';
        // Each drawn heading and row starts where the one before it ends.
        const items = [...drawn.children].map((c) => c.getBoundingClientRect());
        for (let i = 1; i < items.length; i += 1) {
          if (Math.abs(items[i]!.top - items[i - 1]!.bottom) > 1) return `gap at ${String(i)}`;
        }
        const end = Number.parseFloat(getComputedStyle(el).paddingBottom);
        const tall = header.getBoundingClientRect().height + area.getBoundingClientRect().height;
        return Math.abs(el.scrollHeight - (tall + end)) <= 1 ? 'fits' : 'too tall or short';
      });
      expect(fits, 'the list is as tall as its rows and headings').toBe('fits');
      // The keyboard reaches the last row and shows it whole.
      await grid.focus();
      await page.keyboard.press('End');
      await showsLast(page, grid);
    });

    // Paging and stepping scroll a little at a time while the list swaps the rows it draws, which is where a browser's own scroll adjustments can undo ours.
    for (const key of ['PageDown', 'ArrowDown']) {
      test(`${key} reaches the last row and shows it whole`, async ({ page }) => {
        const grid = await withText(page, root);
        // Every row and heading after the column headings: more presses than downloads, which is enough.
        const total = Number(await grid.getAttribute('aria-rowcount')) - 1;
        await grid.focus();
        await page.keyboard.press('Home');
        for (let i = 0; i < total; i += 1) await page.keyboard.press(key);
        await showsLast(page, grid);
      });
    }
  });
}

test("the scrolling areas draw the design system's scrollbar, without arrows", async ({ page }) => {
  await open(page, 'capture=0');
  // Chromium draws the `::-webkit-scrollbar` rules only while the standard properties are left at auto.
  const areas = ['#download-list', 'nav[aria-label="Views and queues"] .overflow-y-auto', 'body'];
  for (const selector of areas) {
    const style = await page
      .locator(selector)
      .first()
      .evaluate((el) => {
        const s = getComputedStyle(el);
        return { width: s.scrollbarWidth, color: s.scrollbarColor };
      });
    expect(style, selector).toEqual({ width: 'auto', color: 'auto' });
  }
});

for (const theme of ['light', 'dark'] as const) {
  for (const size of SIZES) {
    test(`${size.width}x${size.height}, ${theme} theme`, async ({ page }) => {
      await page.setViewportSize(size);
      await open(page, 'capture=0', { theme });
      await checkLayout(page);
      const nav = page.getByRole('navigation', { name: 'Views and queues' });
      const navBox = await nav.boundingBox();
      const docked = size.width >= NAV_WIDTH;
      expect(navBox?.width ?? 0, docked ? 'navigation docked' : 'navigation a rail').toBe(
        docked ? 240 : 52,
      );
      // On first launch the navigation fits the window: no scrollbar, rail or docked.
      const places = nav.locator('.overflow-y-auto');
      const spill = await places.evaluate((el) => el.scrollHeight - el.clientHeight);
      expect(spill, 'navigation without a scrollbar').toBeLessThanOrEqual(0);
      await keepScreenshot(page, `${theme}-layout-${size.width}x${size.height}.png`);

      await checkDetails(page);
      await keepScreenshot(page, `${theme}-layout-${size.width}x${size.height}-selected.png`);
    });

    test(`${size.width}x${size.height}, ${theme} theme: queue actions on one line, labels shown`, async ({
      page,
    }) => {
      await page.setViewportSize(size);
      await open(page, 'capture=0', { theme });
      // Overnight is outside its hours at noon, so all three queue actions show, with New download.
      await page.getByRole('button', { name: /^Overnight/ }).click();
      const actions = page.getByRole('group', { name: 'This queue' }).getByRole('button');
      await expect(actions).toHaveCount(4);
      expect(await oneLine(page, actions), 'queue actions on one line').toBe(0);
      for (const name of ['Start now', 'Pause queue', 'Edit queue', 'New download']) {
        const box = await page.getByRole('button', { name, exact: true }).boundingBox();
        expect(box?.width ?? 0, `${name} shows its label`).toBeGreaterThan(60);
      }
      await checkLayout(page);
    });
  }
}
