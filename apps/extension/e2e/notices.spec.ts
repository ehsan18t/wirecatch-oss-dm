/**
 * The app's notices in a web page (D65), in a real browser, which jsdom cannot lay out: the notices
 * preview (`preview/notices.html`) places the real frame over a page with colours of its own, in the
 * browser's light and dark schemes. The frame must sit in the corner exactly as big as its notices,
 * so the page beside it stays clickable; take the page's scheme, since a frame whose scheme differs
 * gets an opaque backdrop; and keep every notice inside it, with every word lengthened as a
 * translation may.
 */
import { expect, type Frame, type Page } from '@playwright/test';
import { test } from '@wirecatch/ui/testing/playwright';

const SCHEMES = ['light', 'dark'] as const;
const SITES = ['light', 'dark'] as const;

/** The notices frame, once its notices have come in and settled. */
async function noticesFrame(page: Page, query: string): Promise<Frame> {
  await page.goto(`/notices.html?${query}`);
  // Playwright sees frames inside a closed shadow root, which the page itself cannot.
  const find = () => page.frames().find((f) => f.url().includes('notices-frame.html'));
  await expect.poll(() => find() !== undefined).toBe(true);
  const frame = find();
  if (!frame) throw new Error('no notices frame');
  await frame.locator('[data-toast]').first().waitFor();
  // The notices have slid in.
  await frame.evaluate(() =>
    Promise.all(document.getAnimations().map((animation) => animation.finished)),
  );
  // The page around sizes the frame from a message the notices send, which can come later still.
  await expect
    .poll(() =>
      frame.evaluate(() => {
        const notices = document.body.firstElementChild;
        const { clientWidth, clientHeight } = document.documentElement;
        return (
          notices instanceof HTMLElement &&
          clientWidth >= notices.offsetWidth &&
          clientHeight >= notices.offsetHeight
        );
      }),
    )
    .toBe(true);
  return frame;
}

for (const scheme of SCHEMES) {
  for (const site of SITES) {
    test(`the notices sit in the corner at their own size, ${scheme} scheme on a ${site} site`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme });
      const frame = await noticesFrame(page, `notices=several&site=${site}`);
      const element = await frame.frameElement();
      const viewport = page.viewportSize();
      if (!viewport) throw new Error('no viewport');

      // Exactly as big as its notices, so the page beside them stays clickable.
      const notices = await frame.locator('body > div').first().boundingBox();
      if (!notices) throw new Error('no notices');
      await expect
        .poll(async () => {
          const box = await element.boundingBox();
          return box ? [Math.round(box.width), Math.round(box.height)] : null;
        })
        .toEqual([Math.ceil(notices.width), Math.ceil(notices.height)]);
      const box = await element.boundingBox();
      if (!box) throw new Error('the frame has no box');
      expect(box.x + box.width).toBeCloseTo(viewport.width, 0);
      expect(box.y + box.height).toBeCloseTo(viewport.height, 0);

      // A frame whose scheme differs from the page around it gets an opaque backdrop.
      const around = await element.evaluate((el) =>
        el instanceof Element ? getComputedStyle(el).colorScheme : null,
      );
      const inside = await frame.evaluate(
        () => getComputedStyle(document.documentElement).colorScheme,
      );
      expect(inside).toBe(around);

      // The website cannot reach the frame: it is not among the page's frames.
      expect(await page.evaluate(() => window.frames.length)).toBe(0);

      // Just beside the frame is the page.
      const beside = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.tagName, {
        x: box.x - 2,
        y: box.y + box.height / 2,
      });
      expect(beside).not.toBe('DIV');

      await page.screenshot({ path: test.info().outputPath(`notices-${scheme}-on-${site}.png`) });
    });
  }
}

test('Clear all stays at the bottom, below every notice, however many there are', async ({
  page,
}) => {
  const frame = await noticesFrame(page, 'notices=several');
  const clear = await frame.getByRole('button', { name: 'Clear all' }).boundingBox();
  const notices = await frame
    .locator('[data-toast]')
    .evaluateAll((els) => els.map((el) => el.getBoundingClientRect().bottom));
  if (!clear) throw new Error('no Clear all');
  expect(notices).toHaveLength(3);
  for (const bottom of notices) expect(bottom).toBeLessThanOrEqual(clear.y);
});

test('every notice stays inside its frame with every word lengthened', async ({ page }) => {
  const frame = await noticesFrame(page, 'notices=several&pseudo');
  const problems = await frame.evaluate(() => {
    const width = document.documentElement.clientWidth;
    return [...document.querySelectorAll('[data-toast], [data-toast] *')]
      .filter((el) => el.getBoundingClientRect().right > width + 0.5)
      .map((el) => `<${el.tagName.toLowerCase()}> "${el.textContent.trim().slice(0, 40)}"`);
  });
  expect(problems).toEqual([]);
});
