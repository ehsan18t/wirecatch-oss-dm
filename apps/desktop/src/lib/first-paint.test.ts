// The script every window runs before its page draws (D36), exactly as the desktop shell gives
// it (`first-paint.generated.json`, written by the shell's tests), run against a page that has its
// root element and one that does not have it yet. It must set the look for the first frame, prefer
// the look the page last showed, and keep the shell's system colour. Every window is opaque, so
// the page's first background must be the tokens' base and never let anything through.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { runInNewContext } from 'node:vm';
import { describe, expect, it } from 'vitest';
import scripts from './first-paint.generated.json';
import { LOOK_KEY, rememberedLook } from './first-paint.js';

type Case = keyof typeof scripts;

/** A page as the script sees it when it runs, with the system dark or not. */
function page({ dark = false, rootYet = true, remembered = null as string | null } = {}) {
  const doc = document.implementation.createHTMLDocument('page');
  const root = doc.documentElement;
  if (!rootYet) root.remove();
  const storage = new Map<string, string>();
  if (remembered !== null) storage.set(LOOK_KEY, remembered);
  const win = {
    matchMedia: () => ({ matches: dark }),
    localStorage: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => void storage.set(key, value),
    },
  };
  const run = (name: Case): void => {
    runInNewContext(scripts[name], { document: doc, window: win, MutationObserver, JSON });
  };
  /** Lets the parser add the root element, as it does before anything is drawn. */
  const parse = async () => {
    doc.append(root);
    await Promise.resolve();
  };
  return { root, run, parse };
}

describe('the first paint of a window', () => {
  it('sets the scheme, accent and system colour the shell knows, and nothing else', () => {
    const { root, run } = page();
    run('dark_green');
    expect(Object.fromEntries(Object.entries(root.dataset))).toEqual({
      scheme: 'dark',
      accent: 'green',
    });
    expect(root.style.getPropertyValue('--accent-system')).toBe('#0078d4');
  });

  it('resolves the system theme with the system preference', () => {
    const light = page({ dark: false });
    light.run('system_accent_on_linux');
    expect(light.root.dataset.scheme).toBe('light');
    const dark = page({ dark: true });
    dark.run('system_accent_on_linux');
    expect(dark.root.dataset.scheme).toBe('dark');
  });

  it('shows the system accent where the system has one, and cyan where it has none', () => {
    const windows = page();
    windows.run('system_accent_on_windows');
    expect(windows.root.dataset.accent).toBe('system');
    expect(windows.root.style.getPropertyValue('--accent-system')).toBe('#a94dc1');
    const linux = page();
    linux.run('system_accent_on_linux');
    expect(linux.root.dataset.accent).toBe('cyan');
    expect(linux.root.style.getPropertyValue('--accent-system')).toBe('');
  });

  it('waits for the root element when it runs before the parser added it', async () => {
    const { root, run, parse } = page({ rootYet: false });
    run('dark_green');
    expect(root.dataset.scheme).toBeUndefined();
    await parse();
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['dark', 'green']);
  });

  it('prefers the look the page last showed, which is newer than the shell values', () => {
    const { root, run } = page({ remembered: JSON.stringify({ theme: 'light', accent: 'pink' }) });
    run('dark_green');
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['light', 'pink']);
  });

  it('ignores a remembered look this build does not know, or that cannot be read', () => {
    for (const remembered of [
      JSON.stringify({ theme: 'sepia', accent: 'pink' }),
      JSON.stringify({ theme: 'light', accent: 'chartreuse' }),
      '{not json',
      '42',
    ]) {
      const { root, run } = page({ remembered });
      run('dark_green');
      expect([root.dataset.scheme, root.dataset.accent], remembered).toEqual(['dark', 'green']);
    }
  });

  it('works without storage', () => {
    expect(rememberedLook(undefined, ['light'], ['cyan'])).toBeNull();
  });
});

describe('the background a page paints before its styles load', () => {
  // Vitest runs from the app folder. The shell gives each window the same base colour, read from
  // the same tokens (`window_background` in appearance.rs).
  const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8');
  const firstPaint = read('src/pages/first-paint.css');
  const base = /--wc-base:\s*light-dark\((#[\da-f]{6}),\s*(#[\da-f]{6})\)/i.exec(
    read('../../packages/ui/src/tokens.css'),
  );

  it("is the tokens' base in each scheme, and no other colour", () => {
    expect(base).not.toBeNull();
    const [, light, dark] = base!;
    const painted = [...firstPaint.matchAll(/background:\s*([^;]+);/g)].map((m) => m[1]!.trim());
    expect(painted[0]).toBe(light);
    expect(new Set(painted)).toEqual(new Set([light, dark]));
  });

  it('never lets what is behind the window through', () => {
    expect(firstPaint).not.toMatch(/transparent|data-material/);
  });
});
