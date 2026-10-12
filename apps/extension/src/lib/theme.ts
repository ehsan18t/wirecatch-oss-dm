// Colors the popup's stylesheet owns that code outside the popup needs as well. popup/style.css is
// their one home: wxt.config.ts reads them from it when it builds, so a change there reaches the
// code too, and a token that is missing fails the build instead of the background script.

/**
 * The token of the toolbar badge's background while it shows a message: the popup's warning
 * color. The badge has one color for every browser theme, so it takes the light one. The build
 * gives the code its value as `__BADGE_COLOR__`.
 */
export const BADGE_TOKEN = '--warn';

/**
 * Reads a hex color token such as `--warn: #b45309;` from the light theme, the first `:root`
 * block of a stylesheet. Throws when the token is missing or not a hex color, so a renamed token
 * fails the build instead of drifting.
 */
export function cssColor(name: string, css: string): string {
  const light = /:root\s*\{([^}]*)\}/.exec(css)?.[1] ?? '';
  const match = new RegExp(`(?:^|[\\s;])${name}:\\s*(#[0-9a-fA-F]{3,8})\\s*;`).exec(light);
  const value = match?.[1];
  if (value === undefined) throw new Error(`popup/style.css has no light color ${name}`);
  return value;
}
