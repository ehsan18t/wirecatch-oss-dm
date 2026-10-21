import { extendTailwindMerge } from 'tailwind-merge';
import { createTV } from 'tailwind-variants';

export type { VariantProps } from 'tailwind-variants';

/**
 * The names tokens.css gives each Tailwind scale, so merging classes knows `text-ui` is a size and not a colour, and that `h-control` replaces `h-8`. cn.test.ts holds this list to tokens.css.
 */
export const THEME_NAMES = {
  text: [
    'badge',
    'caption',
    'small',
    'ui',
    'body',
    'lead',
    'subtitle',
    'heading',
    'headline',
    'title',
    'hero',
  ],
  font: ['sans', 'display', 'mono'],
  tracking: ['display', 'caps'],
  leading: ['none', 'tight', 'normal'],
  spacing: [
    'titlebar',
    'caption',
    'control-xs',
    'control-sm',
    'control',
    'control-lg',
    'rail',
    'nav-item',
    'scrollbar',
    'scrollbar-gap',
    'row',
    'group',
    'icon-xs',
    'icon-sm',
    'icon-md',
    'icon-lg',
    'icon-xl',
    'icon-2xl',
    'icon-3xl',
    'icon-hero',
    'popup',
  ],
  radius: ['xs', 'sm', 'md', 'lg', 'xl'],
  shadow: ['control', 'raised', 'flyout', 'dialog', 'thumb', 'focus-line', 'invalid-line'],
  blur: ['scrim'],
  container: ['menu', 'dialog-sm', 'dialog-md', 'dialog-lg', 'prose', 'page'],
  breakpoint: ['sm', 'md', 'lg', 'nav'],
  ease: ['out', 'in', 'in-out', 'spring'],
  animate: [
    'appearance',
    'fade-in',
    'fade-out',
    'reveal',
    'rise-in',
    'pop-in',
    'highlight',
    'activity',
    'flow',
    'spin',
    'pulse',
  ],
} as const;

/** Scales Tailwind keys outside the theme groups above. */
export const GROUP_NAMES = {
  w: ['sidebar', 'drawer', 'slide-over', 'toast', 'popup'],
  z: [
    'raised',
    'sticky',
    'titlebar',
    'scrim',
    'drawer',
    'slide-over',
    'window',
    'window-front',
    'toast',
    'popover',
  ],
  duration: ['fast', 'normal', 'slow', 'slower', 'highlight', 'activity', 'flow', 'spin'],
  delay: ['reveal', 'settle', 'tooltip', 'linger'],
  opacity: [
    'disabled',
    'busy',
    'quiet',
    'tint',
    'soft',
    'lift',
    'rim',
    'hover',
    'pressed',
    'sheen',
  ],
} as const;

const twMergeConfig = {
  extend: {
    theme: Object.fromEntries(
      Object.entries(THEME_NAMES).map(([group, names]) => [group, [...names]]),
    ),
    classGroups: {
      ...Object.fromEntries(
        Object.entries(GROUP_NAMES).map(([group, names]) => [group, [{ [group]: [...names] }]]),
      ),
      scale: [{ scale: ['pressed'] }],
    },
  },
};

const twMerge = extendTailwindMerge(twMergeConfig);

export type ClassValue = string | false | null | undefined | readonly ClassValue[];

function collect(inputs: readonly ClassValue[], into: string[]): string[] {
  for (const input of inputs) {
    if (typeof input === 'string') into.push(input);
    else if (Array.isArray(input)) collect(input, into);
  }
  return into;
}

/** Joins classes, the later winning where two set the same thing (`cn('h-8', large && 'h-control-lg')`). */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(collect(inputs, []));
}

/** tailwind-variants with the merge rules above. */
export const tv = createTV({ twMergeConfig });
