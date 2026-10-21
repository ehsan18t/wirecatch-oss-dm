// How a window looks (D36): its theme and accent on the page's root element, where the design
// system's styles read them. `data-scheme` is `light` or `dark` ("system" resolved by the system's
// preference), `data-accent` names the accent, and `--accent-system` holds the system's accent
// colour for the "system" accent. The desktop shell sets all of them before the page's first paint
// with the rules in `first-paint.js`, so nothing flashes; this keeps them in step with the settings, the system's light or dark and the
// system's accent colour from then on, with the same rules, and remembers the look for the next
// first paint.

import type { AppSettings, SystemAppearance } from '@wirecatch/protocol';
import { markAppearanceChange } from '@wirecatch/ui';
import type { Backend, Unsubscribe } from '../backend';
import {
  DARK_QUERY,
  rememberLook,
  resolveScheme,
  shownAccent,
  SYSTEM_ACCENT_PROPERTY,
} from '../first-paint.js';
import { pageStorage } from './page-storage';

export {
  DARK_QUERY,
  LOOK_KEY,
  resolveScheme,
  shownAccent,
  SYSTEM_ACCENT_PROPERTY,
} from '../first-paint.js';

/** The settings a window's look follows. */
export type Look = Pick<AppSettings, 'theme' | 'accent'>;

/** What the look needs from the backend. */
export type AppearanceSource = Pick<Backend, 'systemAppearance' | 'onSystemAppearance'>;

/** The system's light or dark preference as a media query, where the page can ask for one. */
function darkQuery(): MediaQueryList | null {
  return typeof window.matchMedia === 'function' ? window.matchMedia(DARK_QUERY) : null;
}

export class Appearance {
  /** The system is dark now. */
  systemDark = $state(false);
  /** The system's accent colour as `#rrggbb`, when it has one. */
  systemAccent = $state<string | null>(null);

  readonly #root: HTMLElement;
  readonly #query: MediaQueryList | null;
  readonly #storage: Storage | undefined;

  constructor(
    root: HTMLElement = document.documentElement,
    query = darkQuery(),
    storage = pageStorage(),
  ) {
    this.#root = root;
    this.#query = query;
    this.#storage = storage;
    this.systemDark = query?.matches ?? false;
    // What the shell found at first paint, until the backend answers.
    this.systemAccent = root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY).trim() || null;
  }

  /**
   * Puts `look` on the root element, and remembers it for the next first paint. Without settings
   * yet it changes nothing, so the values the shell set before the first paint stay until the real
   * ones arrive.
   */
  apply(look: Look | null | undefined): void {
    if (!look) return;
    const root = this.#root;
    const scheme = resolveScheme(look.theme, this.systemDark);
    const accent = shownAccent(look.accent, this.systemAccent);
    markAppearanceChange(root, { scheme, accent });
    root.dataset.scheme = scheme;
    root.dataset.accent = accent;
    if (this.systemAccent === null) root.style.removeProperty(SYSTEM_ACCENT_PROPERTY);
    else root.style.setProperty(SYSTEM_ACCENT_PROPERTY, this.systemAccent);
    rememberLook(this.#storage, look);
  }

  /**
   * Follows the system's light or dark and its accent colour until the returned function is
   * called. If the backend cannot say, the colour the shell found at first paint stays.
   */
  async connect(source: AppearanceSource): Promise<Unsubscribe> {
    const query = this.#query;
    const onScheme = (e: MediaQueryListEvent) => (this.systemDark = e.matches);
    query?.addEventListener('change', onScheme);
    const update = (system: SystemAppearance) => (this.systemAccent = system.accent);
    let stopAccent: Unsubscribe = () => undefined;
    try {
      stopAccent = await source.onSystemAppearance(update);
      update(await source.systemAppearance());
    } catch {
      // Only the "system" accent depends on it, and it falls back to the first paint's colour.
    }
    return () => {
      query?.removeEventListener('change', onScheme);
      stopAccent();
    };
  }
}

/** Keeps the page's look on `look` as it changes; call it while a component starts. */
export function followLook(appearance: Appearance, look: () => Look | null | undefined): void {
  $effect(() => appearance.apply(look()));
}
