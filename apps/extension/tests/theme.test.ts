import { describe, expect, it } from 'vitest';
import popupCss from '../src/entrypoints/popup/style.css?raw';
import { BADGE_COLOR, cssColor } from '../src/lib/theme';

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

  it('gives the toolbar badge the popup warning color', () => {
    expect(BADGE_COLOR).toMatch(/^#[0-9a-f]{6}$/i);
    expect(BADGE_COLOR).toBe(cssColor('--warn', popupCss));
  });
});
