import { tv, type VariantProps } from '../lib/cn';
import { CHROME, DISABLED, FOCUS_RING } from './control';

// `enabled:hover:`, not `hover:` with a reset: a disabled control never reacts (see the `enabled` variant in tokens.css).
const QUIET =
  'bg-transparent text-ink-muted enabled:hover:bg-wash enabled:hover:text-ink enabled:active:bg-wash-strong';

export const button = tv({
  base: [
    'relative inline-flex shrink-0 items-center justify-center gap-2 rounded-sm border font-medium whitespace-nowrap',
    // Transparent borders show in forced colours, so every button keeps its outline there.
    'border-transparent enabled:active:scale-pressed',
    FOCUS_RING,
    DISABLED,
    CHROME,
  ],
  variants: {
    // Intent names: `danger` is a destructive action, not a red button.
    variant: {
      primary:
        'bg-accent text-accent-fg shadow-control enabled:hover:bg-accent-hover enabled:active:bg-accent-pressed',
      secondary:
        'border-line bg-control text-ink shadow-control enabled:hover:bg-control-hover enabled:active:bg-wash-strong',
      ghost: QUIET,
      danger:
        'bg-danger text-on-fill shadow-control enabled:hover:bg-danger/hover enabled:active:bg-danger/pressed',
      /** Text that acts, inline with other text. */
      link: 'h-auto border-0 bg-transparent px-0 text-accent underline-offset-4 enabled:hover:underline',
    },
    size: {
      sm: 'h-control-sm px-2.5 text-ui',
      md: 'h-control px-3.5 text-ui',
      lg: 'h-control-lg px-5 text-body',
    },
    block: {
      true: 'w-full',
    },
  },
  compoundVariants: [{ variant: 'link', class: 'h-auto px-0' }],
  defaultVariants: { variant: 'secondary', size: 'md' },
});

export type ButtonVariants = VariantProps<typeof button>;
export type ButtonVariant = NonNullable<ButtonVariants['variant']>;
export type ButtonSize = NonNullable<ButtonVariants['size']>;

export const iconButton = tv({
  base: [
    'relative inline-flex shrink-0 items-center justify-center rounded-sm border border-transparent enabled:active:scale-pressed',
    FOCUS_RING,
    DISABLED,
    CHROME,
  ],
  variants: {
    variant: {
      ghost: QUIET,
      secondary:
        'border-line bg-control text-ink shadow-control enabled:hover:bg-control-hover enabled:active:bg-wash-strong',
      primary:
        'bg-accent text-accent-fg enabled:hover:bg-accent-hover enabled:active:bg-accent-pressed',
    },
    size: {
      xs: 'size-control-xs rounded-xs',
      sm: 'size-control-sm',
      md: 'size-control',
      lg: 'size-control-lg',
    },
    /** On: an open panel, a pressed toggle. Pair with aria-pressed or aria-expanded. */
    active: {
      true: 'bg-accent-soft text-accent enabled:hover:bg-accent-soft enabled:hover:text-accent',
    },
  },
  defaultVariants: { variant: 'ghost', size: 'md' },
});

export type IconButtonVariants = VariantProps<typeof iconButton>;
export type IconButtonSize = NonNullable<IconButtonVariants['size']>;
