/**
 * @vitest-environment node
 *
 * Reads wxt.config.ts, which reads files next to it, so these run in Node rather than jsdom.
 */
import { describe, expect, it } from 'vitest';
import config from '../wxt.config';
import popupCss from '../src/entrypoints/popup/style.css?raw';
import { BADGE_TOKEN, cssColor } from '../src/lib/theme';

const SHEET = `
:root {
  --ok: #15803d;
  --warn: #b45309;
}

@media (prefers-color-scheme: dark) {
  :root {
    --warn: #fbbf24;
  }
}
`;

describe('popup colors', () => {
  it('reads a color from the light theme, not the dark one', () => {
    expect(cssColor('--warn', SHEET)).toBe('#b45309');
  });

  it('fails loudly for a token that is not there', () => {
    expect(() => cssColor('--missing', SHEET)).toThrow(/no light color --missing/);
    // A longer name that ends the same is a different token.
    expect(() => cssColor('-ok', SHEET)).toThrow();
  });

  it('gives the toolbar badge the popup warning color, which the build injects', async () => {
    const color = cssColor(BADGE_TOKEN, popupCss);
    expect(color).toMatch(/^#[0-9a-f]{6}$/i);
    const vite = await config.vite?.({
      browser: 'chrome',
      mode: 'production',
      command: 'build',
      manifestVersion: 3,
    });
    expect(vite?.define?.__BADGE_COLOR__).toBe(JSON.stringify(color));
  });
});
