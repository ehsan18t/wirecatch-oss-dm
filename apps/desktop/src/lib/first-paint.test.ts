// The script every window runs before its page draws (D36), exactly as the desktop shell gives
// it (`first-paint.generated.json`, written by the shell's tests), run against a page that has its
// root element and one that does not have it yet. It must set the look for the first frame, prefer
// the look the page last showed, and keep the shell's material and system colour.

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
  it('sets the scheme, accent, material and system colour the shell knows', () => {
    const { root, run } = page();
    run('dark_green_on_mica');
    expect(Object.fromEntries(Object.entries(root.dataset))).toEqual({
      scheme: 'dark',
      accent: 'green',
      material: 'mica',
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
    windows.run('system_accent_on_acrylic');
    expect(windows.root.dataset.accent).toBe('system');
    expect(windows.root.dataset.material).toBe('acrylic');
    expect(windows.root.style.getPropertyValue('--accent-system')).toBe('#a94dc1');
    const linux = page();
    linux.run('system_accent_on_linux');
    expect(linux.root.dataset.accent).toBe('cyan');
    expect(linux.root.dataset.material).toBeUndefined();
    expect(linux.root.style.getPropertyValue('--accent-system')).toBe('');
  });

  it('waits for the root element when it runs before the parser added it', async () => {
    const { root, run, parse } = page({ rootYet: false });
    run('dark_green_on_mica');
    expect(root.dataset.scheme).toBeUndefined();
    await parse();
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['dark', 'green']);
  });

  it('prefers the look the page last showed, which is newer than the shell values', () => {
    const { root, run } = page({ remembered: JSON.stringify({ theme: 'light', accent: 'pink' }) });
    run('dark_green_on_mica');
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['light', 'pink']);
    expect(root.dataset.material).toBe('mica');
  });

  it('ignores a remembered look this build does not know, or that cannot be read', () => {
    for (const remembered of [
      JSON.stringify({ theme: 'sepia', accent: 'pink' }),
      JSON.stringify({ theme: 'light', accent: 'chartreuse' }),
      '{not json',
      '42',
    ]) {
      const { root, run } = page({ remembered });
      run('dark_green_on_mica');
      expect([root.dataset.scheme, root.dataset.accent], remembered).toEqual(['dark', 'green']);
    }
  });

  it('works without storage', () => {
    expect(rememberedLook(undefined, ['light'], ['cyan'])).toBeNull();
  });
});
