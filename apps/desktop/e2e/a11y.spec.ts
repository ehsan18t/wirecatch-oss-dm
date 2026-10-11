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
    await row(page, 'LibreOffice').click({ button: 'right' });
    await audit(page, 'context menu');
    await page.keyboard.press('Escape');
    await row(page, 'LibreOffice').click();
    await page.getByRole('button', { name: 'Cancel and discard the partial file' }).click();
    await audit(page, 'cancel confirmation');
    await page.keyboard.press('Escape');
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

  test(`media and add-on dialogs keep the keyboard inside, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    const inside = () =>
      page.evaluate(() => document.activeElement?.closest('dialog[open]') !== null);

    await page.keyboard.press('Control+K');
    await page.keyboard.type('several qualities');
    await page.keyboard.press('Enter');
    const picker = page.getByRole('dialog', { name: 'Pick a quality' });
    await picker.waitFor();
    // The best quality has focus; Tab and Shift+Tab never leave the dialog.
    await expect(picker.getByRole('radio').first()).toBeFocused();
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press('Tab');
      expect(await inside()).toBe(true);
    }
    for (let i = 0; i < 12; i++) {
      await page.keyboard.press('Shift+Tab');
      expect(await inside()).toBe(true);
    }
    // Escape puts it aside; the sidebar note brings it back.
    await page.keyboard.press('Escape');
    await expect(picker).toBeHidden();
    await page.getByRole('button', { name: '1 video waits for you to pick a quality' }).click();
    await expect(picker).toBeVisible();

    // Answered somewhere else: the picker closes by itself and says why.
    await page.keyboard.press('Escape');
    await page.keyboard.press('Control+K');
    await page.keyboard.type('answered somewhere else');
    await page.keyboard.press('Enter');
    await expect(
      page.getByRole('button', { name: '1 video waits for you to pick a quality' }),
    ).toBeHidden();

    await page.keyboard.press('Control+K');
    await page.keyboard.type('needs the media add-on');
    await page.keyboard.press('Enter');
    const prompt = page.getByRole('dialog', { name: /needs the media add-on/ });
    await prompt.waitFor();
    await expect(prompt.getByRole('button', { name: 'Install', exact: true })).toBeFocused();
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('Tab');
      expect(await inside()).toBe(true);
    }
    await page.keyboard.press('Escape');
    await expect(prompt).toBeHidden();
  });

  test(`media and add-on screens pass axe, ${theme} theme`, async ({ page }) => {
    await open(page, 'capture=0', { theme });
    await page.keyboard.press('Control+K');
    await page.keyboard.type('several qualities');
    await page.keyboard.press('Enter');
    await page.getByRole('dialog', { name: 'Pick a quality' }).waitFor();
    await audit(page, 'quality picker');
    await page.getByRole('button', { name: "Don't download" }).click();

    await page.keyboard.press('Control+K');
    await page.keyboard.type('needs the media add-on');
    await page.keyboard.press('Enter');
    await page.getByRole('button', { name: 'Install', exact: true }).waitFor();
    await audit(page, 'add-on prompt');
    await page.getByRole('button', { name: 'Not now' }).click();

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
