// The download window in a real browser, as the sample app shows it (inside the page, as wide as a real one and as tall as its content): axe in both themes in each of its views, the file question by keyboard, and its layout at 100%, 125% and 150% display scale and with enlarged text: nothing scrolls sideways, every control stays within the window, the buttons at the bottom stay in view, and a window with room to spare is as tall as its content, title bar included.

import AxeBuilder from '@axe-core/playwright';
import { DOWNLOAD_WINDOW_MAX_HEIGHT } from '@wirecatch/protocol';
import { expect, test, type Locator, type Page } from '@playwright/test';
import { open, settled } from './helpers';

const windows = (page: Page) => page.getByRole('region', { name: /^Wirecatch download window/ });

async function audit(page: Page, label: string) {
  for (const window of await windows(page).all()) await settled(window);
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

/** Runs one of the sample app's palette actions, which play the browser's part. The palette is the main window's, so the focus leaves any download window first: keys typed in one stay there. */
async function pretend(page: Page, words: string) {
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  await page.keyboard.press('Control+Shift+P');
  await page.keyboard.type(words);
  await page.keyboard.press('Enter');
}

/** The newest download window showing `heading`. */
async function windowWith(page: Page, heading: string | RegExp): Promise<Locator> {
  const showing = windows(page).filter({ has: page.getByRole('heading', { name: heading }) });
  await showing.waitFor();
  // Held by its label, which stays while the window moves on to its next view.
  const label = await showing.getAttribute('data-label');
  return page.locator(`[data-label="${label ?? ''}"]`);
}

/** A browser file waiting for its answer, in its own window. */
async function fileQuestion(page: Page) {
  await pretend(page, 'download that asks first');
  return windowWith(page, 'Download this file?');
}

/** A video's qualities, in its own window. */
async function qualityQuestion(page: Page) {
  await pretend(page, 'several qualities');
  return windowWith(page, 'Pick a quality');
}

for (const theme of ['light', 'dark'] as const) {
  test(`the download window passes axe in each of its views, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    const file = await fileQuestion(page);
    await audit(page, 'file question');
    await file.getByRole('combobox', { name: 'Queue' }).click();
    await audit(page, 'file question, queue list open');
    await page.keyboard.press('Escape');
    await file.getByRole('button', { name: 'Download now' }).click();
    await expect(file.getByRole('progressbar')).toBeVisible();
    await audit(page, 'progress');
    await file.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(file.getByRole('button', { name: 'Keep downloading' })).toBeFocused();
    await audit(page, 'progress, asking before cancelling');
    await file.getByRole('button', { name: 'Hide' }).click();
    await expect(file).toBeHidden();

    const quality = await qualityQuestion(page);
    await audit(page, 'quality question');
    await quality.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(quality).toBeHidden();

    await pretend(page, 'takes a while to read');
    const reading = await windowWith(page, 'Reading the video');
    await audit(page, 'reading a video');
    await reading.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(reading).toBeHidden();

    await pretend(page, 'needs the media add-on');
    const addon = await windowWith(page, /Install the media add-on/);
    await audit(page, 'add-on offer');
    await addon.getByRole('button', { name: /^Install \(/ }).click();
    await expect(addon.getByRole('progressbar')).toBeVisible();
    await audit(page, 'add-on installing');
    // The install may finish meanwhile and the window move on, so it goes by its title bar's Close.
    await addon.getByRole('button', { name: 'Close', exact: true }).first().click();
    await expect(addon).toBeHidden();

    await fileQuestion(page);
    await pretend(page, 'answered somewhere else');
    await windowWith(page, 'Nothing to do here');
    await audit(page, 'answered somewhere else');
  });
}

test('the file question by keyboard: the order of its controls, Enter to download, Escape to hide', async ({
  page,
}) => {
  await open(page, 'capture=0');
  const window = await fileQuestion(page);
  await expect(window.getByRole('button', { name: 'Download now' })).toBeFocused();
  // Back through the window from its main button: every control once, in reading order.
  const stops: string[] = [];
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press('Shift+Tab');
    stops.push(
      await page.evaluate(() => {
        const el = document.activeElement;
        if (!(el instanceof HTMLElement)) return '';
        const labelledBy = el.getAttribute('aria-labelledby');
        const label = labelledBy
          ? document.getElementById(labelledBy)?.textContent
          : el instanceof HTMLInputElement
            ? el.labels?.[0]?.textContent
            : null;
        return (el.getAttribute('aria-label') ?? label ?? el.innerText).trim();
      }),
    );
  }
  expect(stops).toEqual([
    'Download later',
    'Cancel',
    'Keep the window open to show progress',
    'Don’t ask again for files from the browser',
    'Main',
    'Change',
    'File name',
    'Close',
  ]);
  // The queue opens from the keyboard instead of downloading.
  const queue = window.getByRole('combobox', { name: 'Queue' });
  await queue.focus();
  await page.keyboard.press('Enter');
  await expect(queue).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  await expect(queue).toHaveAttribute('aria-expanded', 'false');
  await expect(window).toBeVisible();
  // Enter in the file name downloads it under the name typed.
  const name = window.getByRole('textbox', { name: 'File name' });
  await name.fill('renamed-setup.exe');
  await page.keyboard.press('Enter');
  await expect(window.getByRole('heading', { name: 'renamed-setup.exe' })).toBeVisible();
  await expect(window.getByRole('progressbar')).toBeVisible();
  // The main button follows the download, so the focus stays on it from Pause to Resume.
  await window.getByRole('button', { name: 'Pause' }).focus();
  await page.keyboard.press('Enter');
  await expect(window.getByRole('button', { name: 'Resume' })).toBeFocused();
  // Keeping the download gives the focus back to the Cancel that asked.
  const cancel = window.getByRole('button', { name: 'Cancel', exact: true });
  await cancel.focus();
  await page.keyboard.press('Enter');
  await expect(window.getByRole('button', { name: 'Keep downloading' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(cancel).toBeFocused();
  // The focus never left the window, so Escape still hides it.
  await page.keyboard.press('Escape');
  await expect(window).toBeHidden();
});

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

/** The window lies in the page, nothing in it runs past its sides, its buttons stay in view, and it is as tall as its content where it has the room. */
async function checkFits(page: Page, window: Locator, label: string) {
  await settled(window);
  const view = page.viewportSize() ?? { width: 0, height: 0 };
  const outer = await box(window);
  expect(outer.x + outer.width, `${label}: window right`).toBeLessThanOrEqual(view.width + 0.5);
  expect(outer.y + outer.height, `${label}: window bottom`).toBeLessThanOrEqual(view.height + 0.5);
  const inside = async (target: Locator, what: string) => {
    const b = await box(target);
    expect(b.x, `${label}: ${what} left`).toBeGreaterThanOrEqual(outer.x - 0.5);
    expect(b.x + b.width, `${label}: ${what} right`).toBeLessThanOrEqual(
      outer.x + outer.width + 0.5,
    );
    expect(b.y, `${label}: ${what} top`).toBeGreaterThanOrEqual(outer.y - 0.5);
    expect(b.y + b.height, `${label}: ${what} bottom`).toBeLessThanOrEqual(
      outer.y + outer.height + 0.5,
    );
  };
  const spill = await window.evaluate((el): string[] =>
    [el, ...el.querySelectorAll('div, p, form, fieldset, dl, section, header')]
      .filter(
        (e) =>
          e.scrollWidth > e.clientWidth + 1 &&
          getComputedStyle(e).overflowX !== 'visible' &&
          // A line cut short with an ellipsis on purpose, its whole text in a tooltip.
          getComputedStyle(e).textOverflow !== 'ellipsis',
      )
      .map((e) => e.getAttribute('class') ?? e.tagName),
  );
  expect(spill, `${label}: nothing scrolls sideways`).toEqual([]);
  await inside(window.getByRole('button', { name: 'Close', exact: true }).first(), 'Close');
  const group = window.getByRole('group').first();
  // The buttons at the bottom of the frame, which stay in view however the body scrolls.
  const footer = group.locator(':scope > div').last().getByRole('button');
  for (const button of await footer.all()) await inside(button, await button.innerText());
  const body = group.locator(':scope > div.overflow-y-auto');
  for (const control of await body.getByRole('button').all()) {
    await control.scrollIntoViewIfNeeded();
    const c = await box(control);
    expect(c.x, `${label}: ${await control.innerText()} left`).toBeGreaterThanOrEqual(outer.x);
    expect(c.x + c.width, `${label}: ${await control.innerText()} right`).toBeLessThanOrEqual(
      outer.x + outer.width + 0.5,
    );
  }
  // The window grows to its content until it meets the page's edge or the most a window takes; only then does the body scroll.
  const room = Math.min(view.height - outer.y, DOWNLOAD_WINDOW_MAX_HEIGHT);
  const scrolls = await body.evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  if (outer.height < room - 32)
    expect(scrolls, `${label}: no scrolling with room to spare`).toBe(false);
  // A body that scrolls is a tab stop of its own, so the keyboard can scroll it (axe's scrollable-region-focusable); at the larger scales the bodies do scroll.
  await audit(page, label);
}

for (const { scale, root, width, height } of SCALES) {
  test.describe(`at ${scale * 100}% display scale with ${root}px text, ${width}x${height}`, () => {
    test.use({ deviceScaleFactor: scale, viewport: { width, height } });

    test('the download window fits each of its views', async ({ page }) => {
      await open(page, 'capture=0', { textSize: root });
      const file = await fileQuestion(page);
      await checkFits(page, file, 'file question');
      // A larger text size while the window is open: the title bar grows as well as the frame, and the window with them until it meets the page's edge or the most a window takes.
      const before = (await box(file)).height;
      await page.addStyleTag({ content: `html { font-size: ${root + 2}px !important; }` });
      await expect
        .poll(async () => {
          const now = await box(file);
          return (
            now.height > before ||
            now.height >= DOWNLOAD_WINDOW_MAX_HEIGHT - 1 ||
            now.y + now.height >= (page.viewportSize()?.height ?? 0) - 32
          );
        })
        .toBe(true);
      await checkFits(page, file, 'file question, text made larger');
      await file.getByRole('button', { name: 'Change' }).click();
      await expect(file.getByRole('button', { name: 'Use default' })).toBeVisible();
      await checkFits(page, file, 'file question, a folder chosen');
      await file.getByRole('button', { name: 'Download now' }).click();
      await expect(file.getByRole('progressbar')).toBeVisible();
      // The sample's queue is full, so the download waits its turn. Measured once the window has heard where it stands: that adds a line a moment after the view appears, and the window follows it.
      await file.getByText(/in line in Main/).waitFor();
      await checkFits(page, file, 'progress');
      await file.getByRole('button', { name: 'Cancel', exact: true }).click();
      await checkFits(page, file, 'progress, asking before cancelling');
      await file.getByRole('button', { name: 'Hide' }).click();
      await expect(file).toBeHidden();

      const quality = await qualityQuestion(page);
      await checkFits(page, quality, 'quality question');
      await quality.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(quality).toBeHidden();

      await pretend(page, 'needs the media add-on');
      const addon = await windowWith(page, /Install the media add-on/);
      await checkFits(page, addon, 'add-on offer');
    });
  });
}
