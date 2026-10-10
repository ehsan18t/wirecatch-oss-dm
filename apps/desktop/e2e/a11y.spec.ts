// Acceptance item 3 in a real browser: axe, including color contrast, in both themes, and a
// keyboard-only walk with visible focus.

import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { open, row } from './helpers';

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
    await open(page, 'capture=0', { theme, hints: true });
    await audit(page, 'main');
    await row(page, 'Conference keynote').click();
    await audit(page, 'error selected');
    await row(page, 'android-studio').click();
    await audit(page, 'paused selected');
    await page.keyboard.press('Control+K');
    await audit(page, 'palette');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Add download' }).click();
    await audit(page, 'add dialog');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: /^Settings/ }).click();
    await audit(page, 'settings');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Edit Overnight: name, schedule and limits' }).click();
    await audit(page, 'queue editor');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+K');
    await page.keyboard.type('asks first');
    await page.keyboard.press('Enter');
    await audit(page, 'confirmation');
    await page.getByRole('button', { name: 'Download', exact: true }).click();
    await page.keyboard.press('Control+K');
    await page.keyboard.type('pretend the browser sent a download');
    await page.keyboard.press('Enter');
    await page
      .getByRole('region', { name: 'Notifications' })
      .getByText(/^Downloading/)
      .waitFor();
    await audit(page, 'capture toast');
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
      const shadow = s.boxShadow !== 'none';
      return {
        name: el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 30) ?? el.tagName,
        visible: ring || shadow,
      };
    });
    if (!info) continue;
    seen.push(info.name);
    expect(info.visible, `no visible focus on "${info.name}"`).toBe(true);
  }
  expect(seen.length).toBeGreaterThan(20);
});
