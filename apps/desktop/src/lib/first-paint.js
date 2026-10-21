// @ts-check
// The look of a window (D36, D45), as one set of rules for both moments that need it. Before a page
// draws, the desktop shell runs this file in every window, with `export` taken off each line that
// starts with it, and calls `paintFirst` with what it knows (`apps/desktop/src-tauri/src/appearance.rs`;
// `first-paint.generated.json` holds the exact scripts, which the tests run). After the page
// loads, its state (`state/appearance.svelte.ts`) uses the same functions. So this file can import
// nothing at run time, and each export is a plain function or constant.

/** @typedef {import('@wirecatch/protocol').Theme} Theme */
/** @typedef {import('@wirecatch/protocol').Accent} Accent */
/** @typedef {{ theme: Theme, accent: Accent }} Look */
/**
 * What the shell knows when it builds a window: the settings (the defaults while the download list
 * is still opening), the system's accent colour, and every theme and accent this build has, to
 * check a remembered look against.
 * @typedef {Look & {
 *   systemAccent: string | null,
 *   themes: readonly Theme[],
 *   accents: readonly Accent[],
 * }} ShellLook
 */

/** Where the page keeps the last look it showed, for the next first paint (a reload, a new window). */
export const LOOK_KEY = 'wirecatch.look';
/** The media query the system's light or dark answers. */
export const DARK_QUERY = '(prefers-color-scheme: dark)';
/** The custom property that holds the system's accent colour. */
export const SYSTEM_ACCENT_PROPERTY = '--accent-system';

/**
 * Light or dark, for the theme setting and whether the system is dark now.
 * @param {Theme} theme
 * @param {boolean} systemDark
 * @returns {'light' | 'dark'}
 */
export function resolveScheme(theme, systemDark) {
  if (theme === 'system') return systemDark ? 'dark' : 'light';
  return theme;
}

/**
 * The accent shown: the system's only when the system has an accent colour, else Wirecatch's cyan.
 * @param {Accent} accent
 * @param {string | null} systemAccent
 * @returns {Accent}
 */
export function shownAccent(accent, systemAccent) {
  return accent === 'system' && systemAccent === null ? 'cyan' : accent;
}

/**
 * Keeps `look` for the next first paint. Storage may be missing or refuse (a private window, a
 * full disk); the look then comes from the shell alone, which is only stale after a change.
 * @param {Storage | undefined} storage
 * @param {Look} look
 */
export function rememberLook(storage, look) {
  try {
    storage?.setItem(LOOK_KEY, JSON.stringify({ theme: look.theme, accent: look.accent }));
  } catch {
    // Nothing to do: the shell's values still paint a right first frame after a restart.
  }
}

/**
 * Whether `value` is one of `list`.
 * @template T
 * @param {readonly T[]} list
 * @param {unknown} value
 * @returns {value is T}
 */
function oneOf(list, value) {
  return list.some((item) => item === value);
}

/**
 * The look the page last showed, when storage has one this build knows.
 * @param {Storage | undefined} storage
 * @param {readonly Theme[]} themes
 * @param {readonly Accent[]} accents
 * @returns {Look | null}
 */
export function rememberedLook(storage, themes, accents) {
  /** @type {unknown} */
  let saved;
  try {
    saved = JSON.parse(storage?.getItem(LOOK_KEY) ?? 'null');
  } catch {
    return null;
  }
  if (typeof saved !== 'object' || saved === null) return null;
  const theme = 'theme' in saved ? saved.theme : undefined;
  const accent = 'accent' in saved ? saved.accent : undefined;
  if (!oneOf(themes, theme) || !oneOf(accents, accent)) return null;
  return { theme, accent };
}

/**
 * Sets the page's look on its root element before anything is drawn: `data-scheme`,
 * `data-accent` and `--accent-system`. The look the page last showed wins over the shell's, which
 * is fixed when the window is built and so is older after a change; the system's colour is the
 * shell's alone. The root element may not exist yet
 * when this runs, so it waits for it; the parser adds it before anything can be drawn.
 * @param {ShellLook} shell
 * @param {Document} doc
 * @param {{ matchMedia?: (query: string) => { matches: boolean }, localStorage?: Storage }} win
 */
export function paintFirst(shell, doc, win) {
  /** @type {Storage | undefined} */
  let storage;
  try {
    storage = win.localStorage;
  } catch {
    storage = undefined;
  }
  const look = rememberedLook(storage, shell.themes, shell.accents) ?? shell;
  const systemDark = win.matchMedia?.(DARK_QUERY).matches ?? false;
  /** @param {HTMLElement} root */
  const apply = (root) => {
    root.dataset.scheme = resolveScheme(look.theme, systemDark);
    root.dataset.accent = shownAccent(look.accent, shell.systemAccent);
    if (shell.systemAccent !== null) {
      root.style.setProperty(SYSTEM_ACCENT_PROPERTY, shell.systemAccent);
    }
  };
  /** @returns {HTMLElement | null} Missing while the parser has not added it yet. */
  const rootNow = () => doc.documentElement;
  const root = rootNow();
  if (root !== null) {
    apply(root);
    return;
  }
  new MutationObserver((_, observer) => {
    const added = rootNow();
    if (added === null) return;
    observer.disconnect();
    apply(added);
  }).observe(doc, { childList: true });
}
