// The main window works at 960x600, 1280x800, 1440x900 and 1600x1000, in both themes: names keep real room, the tools above the list stay on one line, nothing scrolls sideways, the navigation is a rail in a narrow window and docks with its labels in a wide one, the details sit beside the list where the content is wide and over it with a veil where it is narrow, never covering the list's tools, and a row's buttons take the place of its figures rather than lying over them.

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

/** From this window width the content is wide enough for the details beside the list (`--width-details-beside` of content, with the docked navigation): the sizes below it get the details over a veil. */
const BESIDE_WIDTH = 1280;

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
  const firstName = await rows(page).first().getByRole('gridcell').nth(1).boundingBox();
  expect(firstName?.width ?? 0, 'first row name width').toBeGreaterThanOrEqual(160);

  const tools = page.getByRole('group', { name: 'List tools' }).locator('> *');
  expect(await oneLine(page, tools), 'tools on one line').toBeLessThanOrEqual(2);
  const add = await page.getByRole('button', { name: 'Add download' }).boundingBox();
  expect(add?.height ?? 99, 'Add download on one line').toBeLessThanOrEqual(40);

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow, 'no sideways scrolling').toBeLessThanOrEqual(0);
}

/** Whether the element is what the pointer finds at its own middle: nothing lies over it. */
function uncovered(target: ReturnType<Page['locator']>) {
  return target.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return hit !== null && el.contains(hit);
  });
}

/** With the details open, the list's tools and its Status column are never under them: either the details sit beside the list, or the list waits under a veil (a modal panel) that says it is out of reach. */
async function checkNothingCovered(page: Page, veiled: boolean) {
  const veil = page.locator('dialog[aria-modal="true"]');
  if (veiled) {
    await expect(veil, 'details over a veil').toBeVisible();
    return;
  }
  await expect(veil, 'details beside the list').toBeHidden();
  const targets = [
    ...(await page.getByRole('group', { name: 'List tools' }).locator('> *').all()),
    page.getByRole('columnheader', { name: 'Status' }),
  ];
  for (const target of targets) {
    const name = (await target.getAttribute('aria-label')) ?? (await target.innerText());
    expect(await uncovered(target), `"${name}" not covered by the details`).toBe(true);
  }
}

async function box(target: ReturnType<Page['locator']>) {
  const found = await target.boundingBox();
  if (!found) throw new Error('not on screen');
  return found;
}

/** The details beside the list (a wide window) or over it with a veil (a narrow one): never covering what the list needs, and with the list usable or given back. */
async function checkDetails(page: Page, veiled: boolean) {
  const details = await showDetails(page, 'Conference keynote');
  await expect(details).toBeVisible();
  const footer = details.locator('footer').locator('button');
  expect(await oneLine(page, footer), 'the details footer on one line').toBeLessThanOrEqual(2);
  const main = await box(page.getByRole('main', { name: 'Downloads' }));
  await checkNothingCovered(page, veiled);
  if (veiled) {
    // Over a veil, a click beside the panel closes it and gives the list back.
    await page.mouse.click(main.x + 40, main.y + main.height / 2);
    await expect(details).toBeHidden();
    await rows(page)
      .first()
      .click({ position: { x: 40, y: 20 } });
    await page.keyboard.press('Alt+Enter');
  } else {
    const panel = await box(details);
    expect(panel.width, 'details within the content').toBeLessThan(main.width);
    // The list beside the details stays usable.
    await rows(page)
      .first()
      .click({ position: { x: 40, y: 20 } });
  }
  await expect(page.getByRole('dialog', { name: /^ubuntu-24/ })).toBeVisible();
  await checkNothingCovered(page, veiled);
  if (!veiled) await checkLayout(page);
}

/** The text of every cell of the row that lies under its buttons and still shows. */
function figuresUnderButtons(target: ReturnType<Page['locator']>) {
  return target.evaluate((el) => {
    const pill = el.querySelector('button[aria-label="Show details"]')?.parentElement;
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
  test(`${size.width}x${size.height}: a row's buttons take the place of its figures, which move under its name`, async ({
    page,
  }) => {
    await page.setViewportSize(size);
    await open(page, 'capture=0');
    // With the details beside the list too, where the list has dropped its number columns.
    for (const details of [false, true]) {
      if (details) await showDetails(page, 'Conference keynote');
      const target = row(page, 'ubuntu-24');
      await target.hover();
      await expect.poll(() => figuresUnderButtons(target), 'figures under the buttons').toEqual([]);
      const solid = await target
        .getByRole('button', { name: 'Show details' })
        .evaluate((b) => getComputedStyle(b.parentElement ?? b).backgroundColor);
      expect(solid, 'the buttons sit on a solid surface').toMatch(/^rgb\(/);
      const name = target.getByRole('gridcell').nth(1);
      await expect(name, 'the speed under the name').toContainText(/B\/s/, {
        useInnerText: true,
      });
      await expect(name, 'the time left under the name').toContainText(/ left$/, {
        useInnerText: true,
      });
    }
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
  await open(page, 'capture=0');
  const token = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--spacing-row').trim(),
  );
  const rem = /^(\d+(?:\.\d+)?)rem$/.exec(token)?.[1];
  if (rem === undefined) throw new Error(`--spacing-row is not in rem: ${token}`);
  await page.addStyleTag({ content: `html { font-size: ${root}px !important; }` });
  const grid = page.getByRole('grid', { name: 'Downloads' });
  // The page's own stylesheets arrive as modules and settle a moment after the tag.
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

/** The selected row is the last one, drawn, and shown whole. */
async function showsLast(page: Page, grid: ReturnType<Page['locator']>) {
  await settled(grid);
  const total = Number(await grid.getAttribute('aria-rowcount')) - 1;
  const active = await grid.getAttribute('aria-activedescendant');
  expect(active, 'the selected row is drawn').not.toBeNull();
  const last = page.locator(`#${active}`);
  await expect(last).toHaveAttribute('aria-rowindex', String(total + 1));
  await expect(last).toBeInViewport({ ratio: 1 });
}

for (const { root, scale, height } of TEXT_SIZES) {
  test.describe(`at ${scale * 100}% display scale with ${root}px text`, () => {
    test.use({ deviceScaleFactor: scale, viewport: { width: 1440, height } });

    test('rows are placed where they are drawn', async ({ page }) => {
      const grid = await withText(page, root);
      const drawn = await rows(page)
        .first()
        .evaluate((el) => el.getBoundingClientRect().height);
      const total = Number(await grid.getAttribute('aria-rowcount')) - 1;
      const fits = await grid.evaluate(
        (el, { total, drawn }) => {
          const header = el.querySelector('[role="rowgroup"]')?.getBoundingClientRect().height ?? 0;
          const end = parseFloat(getComputedStyle(el).paddingBottom);
          return Math.abs(el.scrollHeight - (header + total * drawn + end)) <= 1;
        },
        { total, drawn },
      );
      expect(fits, 'the list is as tall as its rows').toBe(true);
      // The keyboard reaches the last row and shows it whole.
      await grid.focus();
      await page.keyboard.press('End');
      await showsLast(page, grid);
    });

    // Paging and stepping scroll a little at a time while the list swaps the rows it draws, which is where a browser's own scroll adjustments can undo ours.
    for (const key of ['PageDown', 'ArrowDown']) {
      test(`${key} reaches the last row and shows it whole`, async ({ page }) => {
        const grid = await withText(page, root);
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

      await checkDetails(page, size.width < BESIDE_WIDTH);
      await keepScreenshot(page, `${theme}-layout-${size.width}x${size.height}-selected.png`);
      await page.keyboard.press('Escape');
      await expect(page.getByRole('dialog')).toBeHidden();
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
      expect(await oneLine(page, actions), 'queue actions on one line').toBe(0);
      for (const name of ['Pause all', 'Resume all', 'Details']) {
        const box = await page.getByRole('button', { name, exact: true }).boundingBox();
        expect(box?.width ?? 0, `${name} shows its label`).toBeGreaterThan(60);
      }
      await checkLayout(page);
    });
  }
}
