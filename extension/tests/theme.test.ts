/**
 * @vitest-environment node
 *
 * Reads wxt.config.ts, which reads files next to it, so these run in Node rather than jsdom.
 */
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import config from '../wxt.config';
import { TOKENS_CSS } from '../build-values';
import { BADGE_TOKEN, cssColor, cssHex, mediaButtonColors } from '../src/lib/theme';

const SHEET = `
:root, [data-scheme] {
  --wc-success: light-dark(#15803d, #4ade80);
  --wc-warning: light-dark(#875200, #f0b54a);
  --wc-ink: light-dark(rgb(0 0 0), #fff);
  --wc-accent-dark: #4fd1c7;
}
`;

describe('badge colors', () => {
  it("reads a token's light colour, not its dark one", () => {
    expect(cssColor('--wc-warning', SHEET)).toBe('#875200');
  });

  it('fails loudly for a token that is not there or not written as hex', () => {
    expect(() => cssColor('--missing', SHEET)).toThrow(/no light colour --missing/);
    // A longer name that ends the same is a different token.
    expect(() => cssColor('-warning', SHEET)).toThrow();
    expect(() => cssColor('--wc-ink', SHEET)).toThrow();
  });

  it("gives the toolbar badge the design system's warning colour, which the build injects", async () => {
    const color = cssColor(BADGE_TOKEN, readFileSync(TOKENS_CSS, 'utf8'));
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

describe('media button colors', () => {
  it("reads a token's dark colour, and an accent written as one colour", () => {
    expect(cssColor('--wc-warning', SHEET, 'dark')).toBe('#f0b54a');
    expect(cssHex('--wc-accent-dark', SHEET)).toBe('#4fd1c7');
    expect(() => cssHex('--wc-warning', SHEET)).toThrow(/no colour --wc-warning/);
  });

  it("takes the design system's colours, six-digit so the button can add an alpha, and the build injects them", async () => {
    const colors = mediaButtonColors(readFileSync(TOKENS_CSS, 'utf8'));
    for (const color of Object.values(colors)) expect(color).toMatch(/^#[0-9a-f]{6}$/i);
    const vite = await config.vite?.({
      browser: 'chrome',
      mode: 'production',
      command: 'build',
      manifestVersion: 3,
    });
    expect(vite?.define?.__MEDIA_BUTTON_COLORS__).toBe(JSON.stringify(colors));
  });
});
