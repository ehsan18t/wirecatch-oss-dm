// The design system in a real engine, built for production (playwright.gallery.config.ts): what jsdom cannot show and the dev server can hide. Each test names the failure it catches.

import { expect, test, type Page } from '@playwright/test';

/** Tab presses before the walk gives up; the gallery has fewer stops than this. */
const MAX_TAB_STOPS = 400;

/** How a keyboard stop shows its focus: its own outline, the outline of the field box it sits in, the field's accent line, or not at all. */
type Shown = 'outline' | 'box outline' | 'accent line' | 'nothing';

interface FocusStop {
  name: string;
  shown: Shown;
}

async function open(page: Page, query: string) {
  await page.goto(`/gallery/?${query}`);
  await expect(page.getByRole('heading', { name: 'Accents and schemes' })).toBeVisible();
}

/** What the page says about the element that has focus: whether it was reached before, its name, and the outlines and shadow that may show its focus. */
interface Focused {
  again: boolean;
  name: string;
  outlined: boolean;
  /** For a text control, its field box (Select's trigger is the box itself); null for other controls. */
  box: { outlined: boolean; shadow: string } | null;
}

function shownBy(focused: Focused): Shown {
  if (focused.outlined) return 'outline';
  if (focused.box?.outlined) return 'box outline';
  // The field's accent line is an inset shadow, drawn only while it has focus.
  if (focused.box && focused.box.shadow !== 'none') return 'accent line';
  return 'nothing';
}

/** Tabs through the whole page, until focus comes back round, and reports how each stop shows its focus. */
async function walkFocus(page: Page): Promise<FocusStop[]> {
  const stops: FocusStop[] = [];
  for (let i = 0; i < MAX_TAB_STOPS; i++) {
    await page.keyboard.press('Tab');
    const focused = await page.evaluate((): Focused | null => {
      const element = document.activeElement;
      if (!(element instanceof HTMLElement) || element === document.body) return null;
      const again = element.dataset.walked === 'yes';
      element.dataset.walked = 'yes';
      const outlined = (target: Element) => {
        const style = getComputedStyle(target);
        return style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0;
      };
      const text = element.matches(
        'input:not([type="checkbox"], [type="radio"], [type="range"]), textarea',
      );
      const boxElement = text ? element.parentElement : element.closest('[role="combobox"]');
      const kind = element.getAttribute('type') ?? element.getAttribute('role') ?? '';
      const label = element.getAttribute('aria-label') ?? element.innerText.trim();
      return {
        again,
        name: `${element.tagName.toLowerCase()} ${kind} "${label.slice(0, 40)}"`,
        outlined: outlined(element),
        box: boxElement && {
          outlined: outlined(boxElement),
          shadow: getComputedStyle(boxElement).boxShadow,
        },
      };
    });
    if (focused === null || focused.again) break;
    stops.push({ name: focused.name, shown: shownBy(focused) });
  }
  return stops;
}

const showing = (stops: FocusStop[], ...ways: Shown[]) =>
  stops.filter((stop) => !ways.includes(stop.shown)).map((stop) => `${stop.name}: ${stop.shown}`);

for (const scheme of ['light', 'dark'] as const) {
  // Catches: Tailwind 4's `outline-none` turning the ring's `outline-style` into `none` (UI2 review C1).
  test(`every keyboard stop shows focus, ${scheme} scheme`, async ({ page }) => {
    await open(page, `scheme=${scheme}`);
    const stops = await walkFocus(page);
    expect(stops.length).toBeGreaterThan(30);
    expect(showing(stops, 'outline', 'accent line')).toEqual([]);
  });
}

// Catches: fields (and Select, which has no caret) showing focus only through a shadow and a border colour, both of which forced colours drop (UI2 review H3).
test('every keyboard stop shows an outline in forced colours', async ({ page }) => {
  await page.emulateMedia({ forcedColors: 'active' });
  await open(page, 'scheme=light');
  const stops = await walkFocus(page);
  expect(stops.length).toBeGreaterThan(30);
  expect(showing(stops, 'outline', 'box outline')).toEqual([]);
});

/** The colour an element's background resolves to. */
const background = (page: Page, selector: string) =>
  page
    .locator(selector)
    .first()
    .evaluate((element) => getComputedStyle(element).backgroundColor);

for (const pageScheme of ['light', 'dark'] as const) {
  // Catches: colours declared on `:root` only, which the production build resolves there, so a subtree's `data-scheme` changed nothing (UI2 review H1). The dev server keeps `light-dark()` native and hides it.
  test(`a subtree with its own scheme takes it, on a ${pageScheme} page`, async ({ page }) => {
    await open(page, `scheme=${pageScheme}`);
    expect(await background(page, '[data-scheme="light"].bg-base')).toBe('rgb(236, 238, 242)');
    expect(await background(page, '[data-scheme="dark"].bg-base')).toBe('rgb(18, 21, 27)');
    const ink = (selector: string) =>
      page
        .locator(selector)
        .first()
        .evaluate((element) => getComputedStyle(element).color);
    expect(await ink('.bg-base[data-scheme="light"] p')).not.toBe(
      await ink('.bg-base[data-scheme="dark"] p'),
    );
  });
}

test('a subtree with its own accent takes it, in both schemes', async ({ page }) => {
  for (const [scheme, amber] of [
    ['light', 'rgb(140, 90, 0)'],
    ['dark', 'rgb(245, 185, 66)'],
  ] as const) {
    await open(page, `scheme=${scheme}&accent=cyan`);
    const button = page.locator('[data-accent="amber"]').getByRole('button', { name: 'Download' });
    expect(await button.evaluate((element) => getComputedStyle(element).backgroundColor)).toBe(
      amber,
    );
  }
});

test.describe('the slide-over over a veil', () => {
  // Catches: keys from the panel not reaching the dialog that owns them, and the browser then closing the dialog itself on a second Escape so it could not open again (UI2 review H2).
  test('closes on Escape, gives focus back and opens again', async ({ page }) => {
    await open(page, 'scheme=light');
    const opener = page.getByRole('button', { name: 'Show details over a veil' });
    const panel = page.getByRole('dialog', { name: 'installer.msi' });
    for (let round = 0; round < 2; round++) {
      await opener.click();
      await expect(panel).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(panel).toBeHidden();
      await expect(opener).toBeFocused();
    }
  });

  test('keeps Tab inside', async ({ page }) => {
    await open(page, 'scheme=dark');
    await page.getByRole('button', { name: 'Show details over a veil' }).click();
    const panel = page.getByRole('dialog', { name: 'installer.msi' });
    await expect(panel).toBeVisible();
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press(i % 3 === 2 ? 'Shift+Tab' : 'Tab');
      expect(await panel.evaluate((dialog) => dialog.contains(document.activeElement))).toBe(true);
    }
  });
});
