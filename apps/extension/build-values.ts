/**
 * Values read from their sources when the extension is built and baked into its code. The build
 * (wxt.config.ts) and the tests (vitest.config.ts) both take them from here, so the code sees the
 * same values in both, and a source that is missing stops either at once.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { BADGE_TOKEN, cssColor } from './src/lib/theme';

/** The toolbar badge's color, read from the popup's stylesheet; a missing token stops the build here. */
const badgeColor = cssColor(
  BADGE_TOKEN,
  readFileSync(fileURLToPath(new URL('src/entrypoints/popup/style.css', import.meta.url)), 'utf8'),
);

/** The globals `src/env.d.ts` declares, as Vite's `define` takes them. */
export function buildDefines(): Record<string, string> {
  return {
    __BADGE_COLOR__: JSON.stringify(badgeColor),
  };
}
