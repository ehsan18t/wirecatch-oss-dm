/**
 * @vitest-environment node
 *
 * Reads wxt.config.ts, which reads files next to it, so these run in Node rather than jsdom.
 */
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import config from '../wxt.config';
import { TOKENS_CSS } from '../build-values';
import { BADGE_TOKEN, cssColor } from '../src/lib/theme';

const SHEET = `
:root, [data-scheme] {
  --wc-success: light-dark(#15803d, #4ade80);
  --wc-warning: light-dark(#875200, #f0b54a);
  --wc-ink: light-dark(rgb(0 0 0), #fff);
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
