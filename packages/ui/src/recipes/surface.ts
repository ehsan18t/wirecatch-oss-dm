import { tv, type VariantProps } from '../lib/cn';

/** A solid surface that floats above the page: menus, listboxes, popovers, toasts. */
export const FLYOUT_SURFACE =
  'rounded-md border border-line bg-overlay text-ink shadow-flyout forced-colors:border-sys-text';

/** A dialog's panel: the same surface, raised further. */
export const DIALOG_SURFACE =
  'rounded-lg border border-line bg-overlay text-ink shadow-dialog forced-colors:border-sys-text';

/** A panel on the edge of the window: the slide-over and the sidebar drawer. */
export const PANEL_SURFACE =
  'border-line bg-overlay text-ink shadow-dialog forced-colors:border-sys-text';

/** The veil behind a dialog, and the app's one backdrop blur (D46). */
export const SCRIM = 'bg-scrim backdrop-blur-scrim';

/** Popover elements reset the browser's own popover box; their position is set from script. */
export const TOP_LAYER = 'fixed inset-auto m-0 overflow-visible p-0';

export const card = tv({
  base: 'border text-ink',
  variants: {
    variant: {
      /** On the content layer, lifted by a shadow. */
      raised: 'border-line bg-raised shadow-raised',
      /** Outlined only, for groups that should not compete with the content. */
      outline: 'border-line bg-transparent',
    },
    padding: {
      none: '',
      sm: 'p-3',
      md: 'p-4',
      lg: 'p-6',
    },
    radius: {
      md: 'rounded-md',
      lg: 'rounded-lg',
    },
    interactive: {
      true: 'transition hover:bg-control-hover active:scale-pressed',
    },
  },
  defaultVariants: { variant: 'raised', padding: 'md', radius: 'lg' },
});

export type CardVariants = VariantProps<typeof card>;
