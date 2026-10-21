// The contract between the theme settings and the tokens: which attributes on `<html>` mean what. tokens.css defines a rule for each value listed here, and tokens.test.ts holds the two to each other.
import { ACCENTS, isAccent, type Accent } from '@wirecatch/protocol';

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

/**
 * Puts `appearance` on `element` (`<html>` for a window) in the form tokens.css reads. Colour transitions are held off until the next frame has painted, so a scheme change does not fade every control separately.
 */
export function applyAppearance(element: HTMLElement, appearance: Appearance): void {
  element.setAttribute(CHANGING_ATTRIBUTE, '');
  setAttribute(element, 'data-scheme', appearance.scheme);
  element.setAttribute('data-accent', appearance.accent);
  const asked = appearance.accent === 'system' ? (appearance.systemAccent ?? null) : null;
  const systemAccent = asked !== null && CSS.supports('color', asked) ? asked : null;
  if (systemAccent === null) element.style.removeProperty(SYSTEM_ACCENT_PROPERTY);
  else element.style.setProperty(SYSTEM_ACCENT_PROPERTY, systemAccent);
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      element.removeAttribute(CHANGING_ATTRIBUTE);
    }),
  );
}
