import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { THEME_NAMES } from './lib/cn';

const css = readFileSync(resolve(process.cwd(), 'src/base.css'), 'utf8').replaceAll(
  /\/\*[\s\S]*?\*\//g,
  '',
);

/** The declarations of every rule whose selector list names `element` itself. */
function rulesFor(element: string): string[] {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? '').split(',').map((s) => s.trim());
    if (selectors.includes(element)) found.push(match[2] ?? '');
  }
  return found;
}

describe('base.css', () => {
  it('draws scrollbars without arrow buttons where the engine draws them otherwise', () => {
    expect(rulesFor('::-webkit-scrollbar-button').join('')).toMatch(/display:\s*none/);
    expect(rulesFor('::-webkit-scrollbar').join('')).toMatch(/width:\s*var\(--spacing-scrollbar\)/);
  });

  it("hides a search field's own clear button, since the design system's Input draws one", () => {
    expect(rulesFor("input[type='search']::-webkit-search-cancel-button").join('')).toMatch(
      /appearance:\s*none/,
    );
  });

  it('leaves the root font size alone, so a rem stays the 16px every length in tokens.css is written for', () => {
    const sizes = THEME_NAMES.text.map((name) => new RegExp(String.raw`(^|\s)text-${name}(\s|;)`));
    const root = rulesFor('html');
    expect(root.length).toBeGreaterThan(0);
    for (const declarations of root) {
      for (const size of sizes) expect(declarations).not.toMatch(size);
      expect(declarations).not.toMatch(/font-size/);
    }
    // The page's text size is the body's, which a rem does not follow.
    expect(rulesFor('body').some((d) => /(?:^|\s)text-body(?:\s|;)/.test(d))).toBe(true);
  });
});
