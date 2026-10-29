// Colours from the design system's tokens that code outside a page needs as well: the toolbar badge,
// which the browser paints, and the media button, drawn inside the website's page. `tokens.css` in @wirecatch/ui is their one home: build-values.ts reads
// them from it when the extension is built, so a change there reaches the badge too, and a token
// that is missing fails the build instead of the background script.

/**
 * The token of the toolbar badge's background while it shows a message: the warning colour. The
 * badge has one colour for every browser theme, so it takes the light scheme's. The build gives
 * the code its value as `__BADGE_COLOR__`.
 */
export const BADGE_TOKEN = '--wc-warning';

/**
 * Reads one scheme's colour of a token written as `--name: light-dark(#light, #dark);`, the
 * way tokens.css writes most colours. Throws when the token is missing or either colour is not
 * a hex colour, so a renamed token fails the build instead of drifting.
 */
export function cssColor(name: string, css: string, scheme: 'light' | 'dark' = 'light'): string {
  const hex = '(#[0-9a-fA-F]{3,8})';
  const match = new RegExp(
    String.raw`(?:^|[\s;{])${name}:\s*light-dark\(\s*${hex}\s*,\s*${hex}\s*\);`,
  ).exec(css);
  const value = match?.[scheme === 'light' ? 1 : 2];
  if (value === undefined) throw new Error(`tokens.css has no ${scheme} colour ${name}`);
  return value;
}

/** Reads a token written as one hex colour for both schemes, `--name: #hex;`, such as an accent's tone for one scheme. */
export function cssHex(name: string, css: string): string {
  const match = new RegExp(String.raw`(?:^|[\s;{])${name}:\s*(#[0-9a-fA-F]{3,8});`).exec(css);
  const value = match?.[1];
  if (value === undefined) throw new Error(`tokens.css has no colour ${name}`);
  return value;
}

/** The media button's colours: the dark scheme's graphite and text, which read over any video, Wirecatch's own cyan when it is pointed at, and the light scheme's danger under white text for a failure. The build gives them as `__MEDIA_BUTTON_COLORS__`. */
export interface MediaButtonColors {
  surface: string;
  ink: string;
  accent: string;
  danger: string;
}

export function mediaButtonColors(css: string): MediaButtonColors {
  return {
    surface: cssColor('--wc-base', css, 'dark'),
    ink: cssColor('--wc-ink', css, 'dark'),
    accent: cssHex('--wc-accent-dark', css),
    danger: cssColor('--wc-danger', css, 'light'),
  };
}
