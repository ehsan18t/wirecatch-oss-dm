// The Add dialog in a real browser: axe in both themes in each of its states, the whole flow by keyboard, and its layout at 100%, 125% and 150% display scale and with enlarged text: nothing scrolls sideways, every control stays within the window, and the buttons at the bottom stay in view.

import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { open } from './helpers';

const LINKS = [
  'https://download.example.org/releases/app-2.4.1-x64.msi',
  'https://cdn.example.net/media/holiday-video-2026-final-cut.mp4',
  'https://files.example.com/archive/backup-october.zip',
];

async function audit(page: Page, label: string) {
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  const problems = result.violations.map(
    (v) =>
      `${label}: ${v.id} (${v.nodes.length}) ${v.nodes
        .map((n) => n.target.join(' '))
        .slice(0, 3)
        .join(' | ')}`,
  );
  expect(problems, problems.join('\n')).toEqual([]);
}

const addDialog = (page: Page) => page.getByRole('dialog', { name: /^Add/ });
const linksBox = (page: Page) => addDialog(page).getByRole('textbox', { name: 'Links' });

/** Opens the dialog the way the keyboard does, from anywhere in the window. */
async function openAdd(page: Page) {
  await page.keyboard.press('Control+N');
  await expect(linksBox(page)).toBeFocused();
}

for (const theme of ['light', 'dark'] as const) {
  test(`the Add dialog passes axe in each of its states, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    await openAdd(page);
    await audit(page, 'empty');
    await addDialog(page).getByRole('button', { name: 'Download' }).click();
    await expect(linksBox(page)).toHaveAttribute('aria-invalid', 'true');
    await audit(page, 'no link');
    await linksBox(page).fill(`${LINKS.join('\n')}\nmy notes`);
    await addDialog(page).getByRole('button', { name: 'Change' }).click();
    await expect(addDialog(page).getByRole('button', { name: 'Use default' })).toBeVisible();
    await audit(page, 'several links, a folder chosen');
    await addDialog(page).getByRole('combobox', { name: 'Queue' }).click();
    await audit(page, 'queue list open');
  });
}

test('the whole flow by keyboard: open, type, choose, download, and the focus comes back', async ({
  page,
}) => {
  await open(page, 'capture=0');
  const grid = page.getByRole('grid', { name: 'Downloads' });
  const before = Number(await grid.getAttribute('aria-rowcount'));
  await grid.focus();
  await openAdd(page);
  await page.keyboard.type(LINKS[0] ?? '');
  // Tab reaches the file name, the queue, the folder's Change and the two buttons, then wraps.
  const stops: string[] = [];
  for (let i = 0; i < 7; i++) {
    await page.keyboard.press('Tab');
    stops.push(
      await page.evaluate(() => {
        const el = document.activeElement;
        const label = el instanceof HTMLInputElement ? el.labels?.[0]?.textContent.trim() : null;
        return el?.getAttribute('aria-label') ?? label ?? el?.textContent.trim() ?? '';
      }),
    );
  }
  expect(stops.slice(0, 6)).toEqual([
    'File name',
    expect.stringMatching(/\(default\)/),
    'Change',
    'Add paused',
    'Download',
    'Close',
  ]);
  await expect(linksBox(page)).toBeFocused();
  // The queue opens and is chosen from the keyboard.
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Shift+Tab');
  const queue = addDialog(page).getByRole('combobox', { name: 'Queue' });
  await expect(queue).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await page.getByRole('option', { name: 'Overnight' }).waitFor();
  await page.keyboard.type('Over');
  await page.keyboard.press('Enter');
  await expect(queue).toHaveText(/Overnight/);
  await linksBox(page).focus();
  await page.keyboard.press('Control+Enter');
  await expect(addDialog(page)).toBeHidden();
  await expect(grid).toHaveAttribute('aria-rowcount', String(before + 1));
  await expect(grid).toBeFocused();
});

/** Picks folders until the mock's long one is chosen. */
async function chooseLongFolder(page: Page) {
  const saveTo = addDialog(page).getByRole('group', { name: 'Save to' });
  for (let picks = 0; picks < 5; picks++) {
    const before = (await saveTo.textContent()) ?? '';
    if (before.includes('Raw footage')) return;
    await saveTo.getByRole('button', { name: 'Change' }).click();
    await expect(saveTo).not.toHaveText(before);
  }
  await expect(saveTo).toContainText('Raw footage');
}

/** Display scales and text sizes: the browser's default, Windows at 125% and 150%, and a larger default font. */
const SCALES = [
  { scale: 1, root: 16, width: 960, height: 600 },
  { scale: 1.25, root: 16, width: 1152, height: 777 },
  { scale: 1.25, root: 20, width: 1152, height: 777 },
  { scale: 1.5, root: 16, width: 853, height: 480 },
  { scale: 1.5, root: 22, width: 1280, height: 601 },
] as const;

/** The box of `target`, which must be drawn. */
async function box(target: Locator) {
  const found = await target.boundingBox();
  if (!found) throw new Error('not on screen');
  return found;
}

/** Nothing in the dialog runs past its sides, and the dialog stays inside the window. */
async function checkFits(page: Page, label: string) {
  const panel = addDialog(page).locator('section');
  const view = page.viewportSize() ?? { width: 0, height: 0 };
  const outer = await box(panel);
  expect(outer.x, `${label}: dialog left`).toBeGreaterThanOrEqual(0);
  expect(outer.y, `${label}: dialog top`).toBeGreaterThanOrEqual(0);
  expect(outer.x + outer.width, `${label}: dialog right`).toBeLessThanOrEqual(view.width + 0.5);
  expect(outer.y + outer.height, `${label}: dialog bottom`).toBeLessThanOrEqual(view.height + 0.5);
  const spill = await panel.evaluate((el): string[] =>
    [el, ...el.querySelectorAll('div, p, form, footer')]
      // A box a pixel wide is text for screen readers only, clipped on purpose.
      .filter(
        (e) =>
          e.clientWidth > 1 &&
          e.scrollWidth > e.clientWidth + 1 &&
          getComputedStyle(e).overflowX !== 'visible',
      )
      .map((e) => e.getAttribute('class') ?? e.tagName),
  );
  expect(spill, `${label}: nothing scrolls sideways`).toEqual([]);
  // The buttons at the bottom are always in view, whatever the body scrolls.
  const footer = addDialog(page).locator('footer').getByRole('button');
  for (const button of await footer.all()) {
    const b = await box(button);
    expect(b.y + b.height, `${label}: ${await button.innerText()} in view`).toBeLessThanOrEqual(
      outer.y + outer.height + 0.5,
    );
    expect(b.x, `${label}: ${await button.innerText()} inside`).toBeGreaterThanOrEqual(outer.x);
    expect(b.x + b.width, `${label}: ${await button.innerText()} inside`).toBeLessThanOrEqual(
      outer.x + outer.width + 0.5,
    );
  }
  // Nothing drawn in the body runs past the dialog's sides, whether or not its box would scroll.
  const past = await panel.evaluate((el): string[] => {
    const { left, right } = el.getBoundingClientRect();
    return [...el.querySelectorAll('form *')]
      .filter((e) => {
        const r = e.getBoundingClientRect();
        return r.width > 1 && (r.left < left - 0.5 || r.right > right + 0.5);
      })
      .map((e) => `${e.tagName} ${e.textContent.trim().slice(0, 40)}`);
  });
  expect(past, `${label}: nothing past the sides`).toEqual([]);
  // Every control in the body can be scrolled to and lies within the dialog's sides.
  for (const control of await addDialog(page).locator('form').getByRole('button').all()) {
    await control.scrollIntoViewIfNeeded();
    const c = await box(control);
    expect(c.x, `${label}: ${await control.innerText()} inside`).toBeGreaterThanOrEqual(outer.x);
    expect(c.x + c.width, `${label}: ${await control.innerText()} inside`).toBeLessThanOrEqual(
      outer.x + outer.width + 0.5,
    );
  }
  const links = await box(linksBox(page));
  expect(links.width, `${label}: room to read a link`).toBeGreaterThanOrEqual(240);
}

for (const { scale, root, width, height } of SCALES) {
  test.describe(`at ${scale * 100}% display scale with ${root}px text, ${width}x${height}`, () => {
    test.use({ deviceScaleFactor: scale, viewport: { width, height } });

    test('the Add dialog fits, for one link and for many, with a folder chosen', async ({
      page,
    }) => {
      await open(page, 'capture=0', { textSize: root });
      await openAdd(page);
      await checkFits(page, 'empty');
      await linksBox(page).fill(LINKS[0] ?? '');
      await addDialog(page).getByRole('button', { name: 'Change' }).click();
      await expect(addDialog(page).getByRole('button', { name: 'Use default' })).toBeVisible();
      await checkFits(page, 'one link, a folder chosen');
      const many = Array.from(
        { length: 12 },
        (_, i) => `${LINKS[i % LINKS.length] ?? ''}?part=${i}`,
      );
      await linksBox(page).fill(many.join('\n'));
      await expect(addDialog(page)).toHaveAccessibleName('Add 12 downloads');
      await checkFits(page, 'twelve links');
    });

    test('the Add dialog fits with what it says when something is wrong, and a long folder', async ({
      page,
    }) => {
      await open(page, 'capture=0', { textSize: root });
      await openAdd(page);
      await addDialog(page).getByRole('button', { name: 'Download' }).click();
      await expect(linksBox(page)).toHaveAttribute('aria-invalid', 'true');
      await checkFits(page, 'no link');
      // The mock refuses links on the reserved .invalid domain.
      await linksBox(page).fill(`${LINKS.join('\n')}\nhttps://refused.invalid/file.zip`);
      await addDialog(page).getByRole('button', { name: 'Add paused' }).click();
      await expect(
        addDialog(page).getByRole('alert').filter({ hasText: 'Could not add' }),
      ).toBeVisible();
      await expect(linksBox(page)).toHaveValue('https://refused.invalid/file.zip');
      await chooseLongFolder(page);
      await checkFits(page, 'a refused link, a long folder');
    });
  });
}
