/**
 * Values read from their sources when the extension is built and baked into its code. The build
 * (wxt.config.ts) and the tests (vitest.config.ts) both take them from here, so the code sees the
 * same values in both, and a source that is missing stops either at once.
 */
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { BADGE_TOKEN, cssColor, mediaButtonColors } from './src/lib/theme';

/** The design system's tokens, where every colour of the extension comes from. */
export const TOKENS_CSS = createRequire(import.meta.url).resolve('@wirecatch/ui/tokens.css');

/** The toolbar badge's colour, read from the tokens; a missing token stops the build here. */
const tokens = readFileSync(TOKENS_CSS, 'utf8');
const badgeColor = cssColor(BADGE_TOKEN, tokens);
const mediaButton = mediaButtonColors(tokens);

/** The globals `src/env.d.ts` declares, as Vite's `define` takes them. */
export function buildDefines(): Record<string, string> {
  return {
    __BADGE_COLOR__: JSON.stringify(badgeColor),
    __MEDIA_BUTTON_COLORS__: JSON.stringify(mediaButton),
  };
}
