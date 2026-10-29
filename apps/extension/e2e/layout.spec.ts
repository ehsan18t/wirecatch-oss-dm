/**
 * The extension's pages laid out in a real browser, with every word pseudo-translated about 60%
 * longer (preview/pseudo.ts) and the long names real pages have (the `long` sample state): nothing
 * may stick out of the popup or the page, scroll sideways, or squeeze text into a sliver. jsdom
 * does no layout, so only this catches a label that grows in translation or a long host name.
 */
import { expect, type Page } from '@playwright/test';
import { test } from '@wirecatch/ui/testing/playwright';

/** The narrowest a column holding a few words of text may get before it reads one word per line. */
const NARROWEST_TEXT_PX = 96;
/** The share of the popup's width the text of a list item keeps beside its buttons. */
const ITEM_TEXT_SHARE = 0.6;

/**
 * The fonts every check runs in. The design asks for Segoe UI, which only Windows has: elsewhere the
 * system's sans stands in, and DejaVu Sans, Ubuntu's, sets the same words about 14% wider. The
 * page brings its own copy, so every machine checks the layout in it, not only a Linux one.
 */
const FONTS: readonly Font[] = [
  { name: 'the system font', query: '', bundled: null },
  { name: 'DejaVu Sans', query: '&font=dejavu', bundled: 'DejaVu Sans' },
];

interface Font {
  name: string;
  query: string;
  /** The family the page loads itself, or null for the system's. */
  bundled: string | null;
}

async function open(page: Page, query: string, font: Font): Promise<void> {
  await page.goto(`/?${query}${font.query}`);
  await page.locator('main').waitFor();
  // `fonts.ready` waits only for loads already started, which the page may not have asked for yet.
  if (font.bundled !== null)
    await page.evaluate((family) => document.fonts.load(`16px "${family}"`), font.bundled);
  await page.evaluate(() => document.fonts.ready);
  if (font.bundled !== null) {
    // A font that failed to load leaves the system's in its place, and the check would measure that.
    const loaded = await page.evaluate(
      (family) =>
        [...document.fonts].some(
          (face) => face.family.replaceAll('"', '') === family && face.status === 'loaded',
        ),
      font.bundled,
    );
    expect(loaded, `${font.bundled} is loaded`).toBe(true);
  }
  // Two frames: the sample answers, the pseudo-translation and the layout have all settled.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
      }),
  );
}

/** Everything under `selector` that sticks out of it, scrolls sideways, or squeezes its text. */
function layoutProblems(page: Page, selector: string): Promise<string[]> {
  return page.evaluate(
    ({ selector, narrowest }) => {
      const root = document.querySelector(selector);
      if (!(root instanceof HTMLElement)) return [`nothing matches ${selector}`];
      const box = root.getBoundingClientRect();
      const name = (el: Element): string =>
        `<${el.tagName.toLowerCase()}> "${el.textContent.trim().slice(0, 40)}"`;
      const ownWords = (el: Element): number =>
        [...el.childNodes]
          .filter((node) => node.nodeType === Node.TEXT_NODE)
          .map((node) => node.textContent ?? '')
          .join(' ')
          .split(/\s+/)
          .filter((word) => word !== '').length;
      // Visually hidden text (descriptions for screen readers) is clipped to nothing.
      const shown = (el: Element, style: CSSStyleDeclaration): boolean => {
        const rect = el.getBoundingClientRect();
        const clipped = style.position === 'absolute' && style.clipPath !== 'none';
        return rect.width > 0 && rect.height > 0 && !clipped;
      };
      const problemsOf = (el: Element): string[] => {
        const style = getComputedStyle(el);
        if (!shown(el, style)) return [];
        const rect = el.getBoundingClientRect();
        const found: string[] = [];
        if (rect.left < box.left - 0.5 || rect.right > box.right + 0.5) {
          found.push(`${name(el)} sticks out: ${String(Math.round(rect.right - box.right))}px`);
        }
        const scrolls = style.overflowX === 'auto' || style.overflowX === 'scroll';
        if (scrolls && el.scrollWidth > el.clientWidth + 1)
          found.push(`${name(el)} scrolls sideways`);
        // Text is squeezed when a narrow box makes it wrap or cuts it; one line that fits whole is not.
        const wholeLine = style.whiteSpace === 'nowrap' && el.scrollWidth <= el.clientWidth + 1;
        if (ownWords(el) >= 3 && rect.width < narrowest && !wholeLine) {
          found.push(`${name(el)} is squeezed to ${String(Math.round(rect.width))}px`);
        }
        return found;
      };
      const sideways = root.scrollWidth > root.clientWidth ? [`${selector} scrolls sideways`] : [];
      return [...sideways, ...[...root.querySelectorAll('*')].flatMap(problemsOf)];
    },
    { selector, narrowest: NARROWEST_TEXT_PX },
  );
}

/** List items whose text column has less than its share of the popup, beside its buttons. */
function crampedItems(page: Page): Promise<string[]> {
  return page.evaluate((share) => {
    const popup = document.querySelector('.popup-frame > div');
    if (!(popup instanceof HTMLElement)) return ['no popup'];
    const room = popup.getBoundingClientRect().width * share;
    return [...popup.querySelectorAll('main li > div:first-child')]
      .filter((column) => column.getBoundingClientRect().width < room)
      .map((column) => column.textContent.trim().slice(0, 40));
  }, ITEM_TEXT_SHARE);
}

/** The footer's button labels as German makes them, two to three times the English. */
const GERMAN_LABELS: Record<string, string> = {
  'Pause all': 'Alle Downloads anhalten',
  'Resume all': 'Alle Downloads fortsetzen',
};

/** Puts the labels of `labels` in place of the English text of those buttons, as a translation would. */
async function relabel(page: Page, labels: Record<string, string>): Promise<string[]> {
  return page.evaluate((labels) => {
    const placed: string[] = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      if (!(node instanceof Text)) continue;
      const label = labels[node.data.trim()];
      if (label !== undefined && node.parentElement?.closest('button')) {
        node.data = label;
        placed.push(label);
      }
    }
    return placed;
  }, labels);
}

/** Those of the buttons labelled `labels` whose label is cut short, at its end or below its last line. */
async function cutLabels(page: Page, labels: string[]): Promise<string[]> {
  const cut: string[] = [];
  for (const label of labels) {
    const isCut = await page
      .getByRole('button', { name: label, exact: true })
      .evaluate((button) =>
        [button, ...button.querySelectorAll('*')].some(
          (el) =>
            getComputedStyle(el).overflow === 'hidden' &&
            (el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1),
        ),
      );
    if (isCut) cut.push(label);
  }
  return cut;
}

/** Those of `labels` that no tooltip offers in full when a person hovers their button. */
async function untippedLabels(page: Page, labels: string[]): Promise<string[]> {
  const untipped: string[] = [];
  for (const label of labels) {
    await page.getByRole('button', { name: label, exact: true }).hover();
    const tip = page.getByRole('tooltip').filter({ hasText: label });
    const shown = await tip.waitFor({ timeout: 2000 }).then(
      () => true,
      () => false,
    );
    if (!shown) untipped.push(label);
    await page.mouse.move(0, 0);
    await tip.waitFor({ state: 'hidden' });
  }
  return untipped;
}

for (const font of FONTS) {
  test.describe(`in ${font.name}`, () => {
    test.describe('the popup with long text', () => {
      for (const state of ['long', 'many', 'no-access', 'closed', 'capture-off', 'old-app']) {
        test(`keeps everything inside the popup (${state})`, async ({ page }) => {
          await page.setViewportSize({ width: 480, height: 720 });
          await open(page, `state=${state}&pseudo`, font);
          expect(await layoutProblems(page, '.popup-frame > div')).toEqual([]);
          expect(await crampedItems(page)).toEqual([]);
        });
      }

      // The Downloads and Video and audio sections, unfolded (D61).
      for (const state of ['long', 'connected', 'no-media', 'old-app']) {
        test(`keeps everything inside the popup with its sections open (${state})`, async ({
          page,
        }) => {
          await page.setViewportSize({ width: 480, height: 720 });
          await open(page, `state=${state}&open&pseudo`, font);
          expect(await layoutProblems(page, '.popup-frame > div')).toEqual([]);
          expect(await crampedItems(page)).toEqual([]);
        });
      }

      test('keeps translated button labels whole and inside the popup', async ({ page }) => {
        await page.setViewportSize({ width: 480, height: 720 });
        await open(page, 'state=long', font);
        const placed = await relabel(page, GERMAN_LABELS);
        expect(placed).toHaveLength(2);
        expect(await layoutProblems(page, '.popup-frame > div')).toEqual([]);
        expect(await cutLabels(page, placed)).toEqual([]);
      });

      test('offers a button label wider than the popup in a tooltip', async ({ page }) => {
        await page.setViewportSize({ width: 480, height: 720 });
        await open(page, 'state=long', font);
        const overlong = Object.fromEntries(
          Object.entries(GERMAN_LABELS).map(([english, german]) => [
            english,
            `${german}, ${german}`,
          ]),
        );
        const placed = await relabel(page, overlong);
        expect(await layoutProblems(page, '.popup-frame > div')).toEqual([]);
        const cut = await cutLabels(page, placed);
        expect(cut).toHaveLength(2);
        expect(await untippedLabels(page, cut)).toEqual([]);
      });

      test('shows the capture switch without scrolling under the website access alert', async ({
        page,
      }) => {
        await page.setViewportSize({ width: 480, height: 720 });
        await open(page, 'state=no-access', font);
        const main = page.locator('main');
        const capture = page.getByRole('switch', { name: 'Capture downloads' });
        const [mainBox, captureBox] = await Promise.all([
          main.boundingBox(),
          capture.boundingBox(),
        ]);
        expect(captureBox && mainBox && captureBox.y + captureBox.height).toBeLessThanOrEqual(
          (mainBox?.y ?? 0) + (mainBox?.height ?? 0),
        );
      });
    });
  });
}
