import type { Component } from 'svelte';
import type { SvelteHTMLElements } from 'svelte/elements';

/** An icon as unplugin-icons compiles it: `import Download from '~icons/lucide/download'`. */
export type IconComponent = Component<SvelteHTMLElements['svg']>;

/** Icon sizes, each a `--spacing-icon-*` token. */
export const ICON_SIZES = {
  xs: 'size-icon-xs',
  sm: 'size-icon-sm',
  md: 'size-icon-md',
  lg: 'size-icon-lg',
  xl: 'size-icon-xl',
  '2xl': 'size-icon-2xl',
  '3xl': 'size-icon-3xl',
  hero: 'size-icon-hero',
} as const;

export type IconSize = keyof typeof ICON_SIZES;
