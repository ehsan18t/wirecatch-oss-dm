// Layout lengths that both the styles and the code need. app.css is their one home: this reads
// them from its text, which the build bundles, so a change there reaches the code as well.
import appCss from '../app.css?raw';

/**
 * Reads the pixel value of a length token such as `--row-h: 44px;` from a stylesheet (app.css
 * unless another is given). Throws when the token is missing or not in pixels, so a renamed or
 * reworded token fails loudly at startup and in every test instead of silently drifting.
 */
export function cssPixels(name: string, css: string = appCss): number {
  const match = new RegExp(`(?:^|[\\s;{])${name}:\\s*(\\d+(?:\\.\\d+)?)px\\s*;`).exec(css);
  const value = match?.[1];
  if (value === undefined) throw new Error(`app.css has no pixel length ${name}`);
  return Number(value);
}

/** The height of one download row (`--row-h`). */
export const ROW_HEIGHT = cssPixels('--row-h');

/**
 * The narrowest window in which the detail panel takes a column of its own: the sidebar, the
 * panel, and the narrowest list that still shows Name, Size, Done and Status.
 */
export const DOCK_WIDTH =
  cssPixels('--sidebar-width') + cssPixels('--detail-width') + cssPixels('--list-min-width');
