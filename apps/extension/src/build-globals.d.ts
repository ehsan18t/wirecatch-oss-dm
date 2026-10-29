import type { MediaButtonColors } from './lib/theme';

declare global {
  /** The media button's colours from the design system's tokens (`mediaButtonColors`); `build-values.ts` defines them for the build and the tests. */
  const __MEDIA_BUTTON_COLORS__: MediaButtonColors;
}
