// Checks every text and control color pair the interface uses against WCAG AA, in both themes,
// straight from the tokens in app.css.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const css = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8');

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`missing ${selector}`);
  const open = css.indexOf('{', start);
  const close = css.indexOf('}', open);
  const tokens: Record<string, string> = {};
  for (const m of css.slice(open + 1, close).matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)) {
    tokens[m[1]!] = m[2]!.toLowerCase();
  }
  return tokens;
}

const light = block(':root {');
const dark = block(":root[data-scheme='dark'] {");
const darkMedia = block(":root:not([data-scheme='light']) {");

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

// [foreground, background, minimum]. 4.5 for text, 3 for focus rings, borders and icons.
const TEXT = 4.5;
const UI = 3;
const PAIRS: [string, string, number][] = [
  ...['bg', 'surface', 'surface-2', 'surface-3', 'accent-soft'].flatMap(
    (bg): [string, string, number][] => [
      ['ink', bg, TEXT],
      ['ink-2', bg, TEXT],
    ],
  ),
  ['ink-3', 'bg', TEXT],
  ['ink-3', 'surface', TEXT],
  ['ink-3', 'surface-2', TEXT],
  ['accent-text', 'surface', TEXT],
  ['accent-text', 'surface-2', TEXT],
  ['accent-text', 'bg', TEXT],
  ['accent-text', 'accent-soft', TEXT],
  ['accent-ink', 'accent', TEXT],
  ['warn', 'surface', TEXT],
  ['warn', 'surface-2', TEXT],
  ['warn', 'accent-soft', TEXT],
  ['danger', 'surface', TEXT],
  ['danger', 'surface-2', TEXT],
  ['danger', 'bg', TEXT],
  ['danger', 'accent-soft', TEXT],
  ['ink', 'danger-soft', TEXT],
  ['focus', 'surface', UI],
  ['focus', 'bg', UI],
  ['focus', 'surface-2', UI],
  ['line-strong', 'surface', UI],
  ['line-strong', 'bg', UI],
  ['accent', 'wire-track', UI],
];

describe.each([
  ['light', light],
  ['dark', dark],
])('%s theme contrast', (_name, tokens) => {
  it.each(PAIRS)('%s on %s', (fg, bg, min) => {
    const a = tokens[fg];
    const b = tokens[bg];
    expect(a, `token --${fg}`).toBeDefined();
    expect(b, `token --${bg}`).toBeDefined();
    expect(contrast(a!, b!)).toBeGreaterThanOrEqual(min);
  });
});

describe('dark theme tokens', () => {
  it('are the same whether chosen in settings or by the system', () => {
    expect(darkMedia).toEqual(dark);
  });
});
