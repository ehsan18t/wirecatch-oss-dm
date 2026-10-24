/**
 * The app's notes in a web page (D65), in a real browser, which jsdom cannot lay out: the notes
 * preview (`preview/notes.html`) places the real frame over a page with colours of its own, in the
 * browser's light and dark schemes. The frame must sit in the corner exactly as big as its notes,
 * so the page beside it stays clickable; take the page's scheme, since a frame whose scheme differs
 * gets an opaque backdrop; and keep every note inside it, with every word lengthened as a
 * translation may.
 */
import { expect, test, type Frame, type Page } from '@playwright/test';

const SCHEMES = ['light', 'dark'] as const;
const SITES = ['light', 'dark'] as const;

/** The notes frame, once its notes have come in and settled. */
async function notesFrame(page: Page, query: string): Promise<Frame> {
  await page.goto(`/notes.html?${query}`);
  // Playwright sees frames inside a closed shadow root, which the page itself cannot.
  const find = () => page.frames().find((f) => f.url().includes('notes-frame.html'));
  await expect.poll(() => find() !== undefined).toBe(true);
  const frame = find();
  if (!frame) throw new Error('no notes frame');
  await frame.locator('[data-toast]').first().waitFor();
  // The notes have slid in and the frame took their size.
  await frame.evaluate(() =>
    Promise.all(document.getAnimations().map((animation) => animation.finished)),
  );
  return frame;
}

for (const scheme of SCHEMES) {
  for (const site of SITES) {
    test(`the notes sit in the corner at their own size, ${scheme} scheme on a ${site} site`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme });
      const frame = await notesFrame(page, `notes=several&site=${site}`);
      const element = await frame.frameElement();
      const viewport = page.viewportSize();
      if (!viewport) throw new Error('no viewport');

      // Exactly as big as its notes, so the page beside them stays clickable.
      const notes = await frame.locator('body > div').first().boundingBox();
      if (!notes) throw new Error('no notes');
      await expect
        .poll(async () => {
          const box = await element.boundingBox();
          return box ? [Math.round(box.width), Math.round(box.height)] : null;
        })
        .toEqual([Math.ceil(notes.width), Math.ceil(notes.height)]);
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

      await page.screenshot({ path: test.info().outputPath(`notes-${scheme}-on-${site}.png`) });
    });
  }
}

test('Clear all stays at the bottom, below every note, however many there are', async ({
  page,
}) => {
  const frame = await notesFrame(page, 'notes=several');
  const clear = await frame.getByRole('button', { name: 'Clear all' }).boundingBox();
  const notes = await frame
    .locator('[data-toast]')
    .evaluateAll((els) => els.map((el) => el.getBoundingClientRect().bottom));
  if (!clear) throw new Error('no Clear all');
  expect(notes).toHaveLength(3);
  for (const bottom of notes) expect(bottom).toBeLessThanOrEqual(clear.y);
});

test('every note stays inside its frame with every word lengthened', async ({ page }) => {
  const frame = await notesFrame(page, 'notes=several&pseudo');
  const problems = await frame.evaluate(() => {
    const width = document.documentElement.clientWidth;
    return [...document.querySelectorAll('[data-toast], [data-toast] *')]
      .filter((el) => el.getBoundingClientRect().right > width + 0.5)
      .map((el) => `<${el.tagName.toLowerCase()}> "${el.textContent.trim().slice(0, 40)}"`);
  });
  expect(problems).toEqual([]);
});
