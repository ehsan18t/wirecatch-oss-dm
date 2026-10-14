// Colours from the design system's tokens that code outside a page needs as well: the toolbar badge,
// which the browser paints. `tokens.css` in @wirecatch/ui is their one home: build-values.ts reads
// them from it when the extension is built, so a change there reaches the badge too, and a token
// that is missing fails the build instead of the background script.

/**
 * The token of the toolbar badge's background while it shows a message: the warning colour. The
 * badge has one colour for every browser theme, so it takes the light scheme's. The build gives
 * the code its value as `__BADGE_COLOR__`.
 */
export const BADGE_TOKEN = '--wc-warning';

/**
 * Reads the light scheme's colour of a token written as `--name: light-dark(#light, #dark);`, the
 * way tokens.css writes every colour. Throws when the token is missing or its light colour is not
 * a hex colour, so a renamed token fails the build instead of drifting.
 */
export function cssColor(name: string, css: string): string {
  const match = new RegExp(
    `(?:^|[\\s;{])${name}:\\s*light-dark\\(\\s*(#[0-9a-fA-F]{3,8})\\s*,[^;]*;`,
  ).exec(css);
  const value = match?.[1];
  if (value === undefined) throw new Error(`tokens.css has no light colour ${name}`);
  return value;
}
