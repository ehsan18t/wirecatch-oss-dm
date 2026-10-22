// Acceptance item 3 in a real browser: axe, including color contrast, in both themes, and a
// keyboard-only walk with visible focus.

import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { open, row, showDetails } from './helpers';

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

for (const theme of ['light', 'dark'] as const) {
  test(`axe finds no WCAG AA problems, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    await audit(page, 'main');
    await showDetails(page, 'Conference keynote');
    await audit(page, 'error in the details');
    await page.keyboard.press('Escape');
    await showDetails(page, 'android-studio');
    await audit(page, 'paused in the details');
    await page.keyboard.press('Escape');
    await row(page, 'LibreOffice').click({ button: 'right' });
    await audit(page, 'context menu');
    await page.keyboard.press('Escape');
    const details = await showDetails(page, 'LibreOffice');
    await details.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Cancel download' }).click();
    await audit(page, 'cancel confirmation');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+Shift+P');
    await audit(page, 'palette');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'New download' }).click();
    await audit(page, 'add dialog');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: /^Settings/ }).click();
    await audit(page, 'settings');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: /^Overnight/ }).hover();
    await page.getByRole('button', { name: 'Actions for Overnight' }).click();
    await page.getByRole('menuitem', { name: 'Edit name, hours and limits' }).click();
    await audit(page, 'queue editor');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('asks first');
    await page.keyboard.press('Enter');
    await audit(page, 'confirmation');
    await page.getByRole('button', { name: 'Download now', exact: true }).click();
    // The window now shows progress; Escape closes it and the main window has the keys again.
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+Shift+P');
    // Files then start at once, with the toast.
    await page.keyboard.type('start browser downloads without asking');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('pretend the browser sent a download');
    await page.keyboard.press('Enter');
    await page
      .getByRole('region', { name: 'Notifications' })
      .getByText(/^Downloading/)
      .waitFor();
    await audit(page, 'capture toast');
  });

  test(`media and add-on windows work with the keyboard, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    const windows = page.getByRole('region', { name: /^Wirecatch download window/ });

    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('several qualities');
    await page.keyboard.press('Enter');
    const picker = windows.filter({ hasText: 'Pick a quality' });
    await picker.waitFor();
    // The main button has the focus, so Enter downloads; it says the add-on comes first.
    await expect(picker.getByRole('button', { name: 'Install and download' })).toBeFocused();
    // Escape closes the window without answering; Waiting for you brings it back.
    await page.keyboard.press('Escape');
    await expect(picker).toBeHidden();
    await page.getByRole('button', { name: /waiting for you/i }).click();
    await expect(picker).toBeVisible();

    // Answered somewhere else: the window says so and the count goes.
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('answered somewhere else');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('button', { name: /waiting for you/i })).toBeHidden();

    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('needs the media add-on');
    await page.keyboard.press('Enter');
    const prompt = windows.filter({ hasText: /Install the media add-on/ });
    await prompt.waitFor();
    await expect(prompt.getByRole('button', { name: /^Install/ })).toBeFocused();
    // Enter on Install: the buttons change, and the focus moves to the new main button.
    await page.keyboard.press('Enter');
    await expect(prompt.getByRole('button', { name: 'Hide' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(prompt).toBeHidden();
  });

  test(`media and add-on screens pass axe, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('several qualities');
    await page.keyboard.press('Enter');
    await page.getByRole('heading', { name: 'Pick a quality' }).waitFor();
    await audit(page, 'quality picker');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();

    await page.keyboard.press('Control+Shift+P');
    await page.keyboard.type('needs the media add-on');
    await page.keyboard.press('Enter');
    await page.getByRole('button', { name: /^Install \(/ }).waitFor();
    await audit(page, 'add-on prompt');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();

    await page.getByRole('button', { name: /^Settings/ }).click();
    await page.getByRole('searchbox', { name: 'Search settings' }).fill('add-on');
    await audit(page, 'add-ons settings');
  });
}

test('keyboard only: every stop shows a visible focus ring', async ({ page }) => {
  await open(page, 'capture=0');
  const seen: string[] = [];
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press('Tab');
    const info = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return null;
      const s = getComputedStyle(el);
      const ring = s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) >= 2;
      // A text field of the design system shows focus on its box, with an accent line (as e2e/gallery.spec.ts reads it).
      const box = el.matches('input, textarea') ? el.parentElement : null;
      const shadow =
        s.boxShadow !== 'none' || (box !== null && getComputedStyle(box).boxShadow !== 'none');
      return {
        name: el.getAttribute('aria-label') ?? (el.textContent.trim().slice(0, 30) || el.tagName),
        visible: ring || shadow,
      };
    });
    if (!info) continue;
    seen.push(info.name);
    expect(info.visible, `no visible focus on "${info.name}"`).toBe(true);
  }
  expect(seen.length).toBeGreaterThan(20);
});

test('keyboard only: the navigation is two Tab stops, so the list is near the top', async ({
  page,
}) => {
  await open(page, 'capture=0');
  const grid = page.getByRole('grid', { name: 'Downloads' });
  const inNav: string[] = [];
  let presses = 0;
  while (!(await grid.evaluate((el) => el === document.activeElement)) && presses < 40) {
    await page.keyboard.press('Tab');
    presses++;
    const name = await page.evaluate(() => {
      const el = document.activeElement;
      return el?.closest('nav') ? el.textContent.trim() : null;
    });
    if (name !== null) inNav.push(name);
  }
  // The place shown; the arrows reach the rest, settings and the shortcuts at its foot too.
  expect(inNav, 'Tab stops in the navigation').toEqual([expect.stringMatching(/^All downloads/)]);
  // The title bar's controls, those two, and the list's tools.
  expect(presses, 'Tab stops before the list').toBeLessThanOrEqual(14);
});
