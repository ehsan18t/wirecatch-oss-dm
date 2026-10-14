import { tv, type VariantProps } from 'tailwind-variants';

/** Hidden on screen but read by screen readers; `focusable` shows it again while it or something in it has focus, as a skip link does. */
export const visuallyHidden = tv({
  base: 'sr-only',
  variants: {
    focusable: { true: 'focus-within:not-sr-only' },
  },
});

export type VisuallyHiddenVariants = VariantProps<typeof visuallyHidden>;
