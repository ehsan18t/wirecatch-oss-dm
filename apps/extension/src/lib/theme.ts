// Colors the popup's stylesheet owns that code outside the popup needs as well. popup/style.css is
// their one home: this reads them from its text, which the build bundles, so a change there
// reaches the code too.
import popupCss from '../entrypoints/popup/style.css?raw';

/**
 * Reads a hex color token such as `--warn: #b45309;` from the light theme, the first `:root`
 * block of a stylesheet (popup/style.css unless another is given). Throws when the token is
 * missing or not a hex color, so a renamed token fails in every test instead of drifting.
 */
export function cssColor(name: string, css: string = popupCss): string {
  const light = /:root\s*\{([^}]*)\}/.exec(css)?.[1] ?? '';
  const match = new RegExp(`(?:^|[\\s;])${name}:\\s*(#[0-9a-fA-F]{3,8})\\s*;`).exec(light);
  const value = match?.[1];
  if (value === undefined) throw new Error(`popup/style.css has no light color ${name}`);
  return value;
}

/**
 * The toolbar badge's background while it shows a message: the popup's warning color. The badge
 * has one color for every browser theme, so it takes the light one.
 */
export const BADGE_COLOR = cssColor('--warn');
