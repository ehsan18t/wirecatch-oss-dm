// The contract between the theme settings and the tokens: which attributes on `<html>` mean what. tokens.css defines a rule for each value listed here, and tokens.test.ts holds the two to each other.
import { ACCENTS, isAccent, type Accent } from '@wirecatch/protocol';
import { timeToken } from './lib/css-token';

/** The resolved colour schemes: the settings turn "system" into one of these. */
export const SCHEMES = ['light', 'dark'] as const;
export type Scheme = (typeof SCHEMES)[number];

/** Every accent, Wirecatch's cyan first (the default); `system` follows the accent colour the OS reports. The list has one home, `ACCENTS` in `crates/protocol`, which the settings save; tokens.test.ts fails on any accent tokens.css has no rule for. */
export { ACCENTS, isAccent, type Accent };

export const DEFAULT_ACCENT: Accent = 'cyan';

/** The custom property that carries the OS accent colour while the accent is `system`. */
export const SYSTEM_ACCENT_PROPERTY = '--accent-system';

/** Set for one frame while the appearance changes, so colours switch at once instead of each fading on its own (base.css). */
const CHANGING_ATTRIBUTE = 'data-appearance-changing';

export const isScheme = (value: unknown): value is Scheme =>
  SCHEMES.some((scheme) => scheme === value);

export interface Appearance {
  /** Null follows the system scheme through CSS alone. */
  scheme: Scheme | null;
  accent: Accent;
  /** Any CSS colour; read only while `accent` is `system`. Anything else is left out, and the accent falls back to cyan: an invalid colour would leave `--wc-accent` unset, and every accent fill with it. */
  systemAccent?: string | null;
}

function setAttribute(element: HTMLElement, name: string, value: string | null): void {
  if (value === null) element.removeAttribute(name);
  else element.setAttribute(name, value);
}

/** The shortest hold: two frames, so the frozen transitions cover the frame the new colours are drawn in. */
const SHORTEST_HOLD_MS = 32;
const holds = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();

/**
 * Crossfades `element` (`<html>`) into the scheme and accent it is about to take (D46): colour transitions are held off, so every control switches at once rather than fading on its own, and base.css lifts a veil of the new background off the page. Nothing happens on the first paint (no accent set yet) or when nothing changes. Call it just before setting the attributes.
 */
export function markAppearanceChange(
  element: HTMLElement,
  next: { scheme: string | null; accent: string },
): void {
  const now = element.dataset;
  if (now.accent === undefined) return;
  if ((now.scheme ?? null) === next.scheme && now.accent === next.accent) return;
  element.setAttribute(CHANGING_ATTRIBUTE, '');
  clearTimeout(holds.get(element));
  const hold = Math.max(timeToken('--transition-duration-normal'), SHORTEST_HOLD_MS);
  holds.set(
    element,
    setTimeout(() => {
      element.removeAttribute(CHANGING_ATTRIBUTE);
    }, hold),
  );
}

/**
 * Puts `appearance` on `element` (`<html>` for a window) in the form tokens.css reads, crossfading into it.
 */
export function applyAppearance(element: HTMLElement, appearance: Appearance): void {
  markAppearanceChange(element, appearance);
  setAttribute(element, 'data-scheme', appearance.scheme);
  element.setAttribute('data-accent', appearance.accent);
  const asked = appearance.accent === 'system' ? (appearance.systemAccent ?? null) : null;
  const systemAccent = asked !== null && CSS.supports('color', asked) ? asked : null;
  if (systemAccent === null) element.style.removeProperty(SYSTEM_ACCENT_PROPERTY);
  else element.style.setProperty(SYSTEM_ACCENT_PROPERTY, systemAccent);
}
