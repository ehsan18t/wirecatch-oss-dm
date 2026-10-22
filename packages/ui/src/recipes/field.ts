import { tv, type VariantProps } from '../lib/cn';
import { DISABLED } from './control';

/**
 * The box every text control shares: Input, TextArea, NumberInput and Select's trigger. Windows 11's fields: a soft fill, a hairline outline, and an accent line along the bottom while focused.
 */
export const field = tv({
  base: [
    // One even outline all round: a stronger bottom edge alone (Fluent's) reads as a lit, uneven rim on the dark theme.
    'relative flex w-full min-w-0 items-center gap-2 rounded-sm border border-line-strong/quiet bg-field text-ink',
    'transition hover:bg-control-hover',
    // Focused, the outline turns the accent with a soft halo, drawn as a shadow so the box does not move. It is the field's focus indicator, so the box draws no ring of its own (Select's trigger is the box).
    'outline-none focus-within:border-accent focus-within:bg-raised focus-within:shadow-focus-field',
    // Forced colours drop the shadow and paint every border alike, so there the box shows focus with an outline. `outline-solid` because `outline-none` above set the style the width utility reads.
    'forced-colors:focus-within:outline-2 forced-colors:focus-within:outline-solid forced-colors:focus-within:outline-sys-highlight',
    // A disabled control inside the box dims it; so does a box that is the control itself (Select's trigger).
    'has-disabled:pointer-events-none has-disabled:opacity-disabled disabled:pointer-events-none disabled:opacity-disabled',
    'forced-colors:border-sys-text',
  ],
  variants: {
    size: {
      sm: 'h-control-sm px-2 text-ui',
      md: 'h-control px-2.5 text-ui',
      lg: 'h-control-lg px-3 text-body',
    },
    invalid: {
      true: 'border-danger focus-within:border-danger focus-within:shadow-invalid-field',
    },
    multiline: {
      true: 'h-auto items-stretch py-1.5',
    },
  },
  defaultVariants: { size: 'md' },
});

export type FieldVariants = VariantProps<typeof field>;
export type FieldSize = NonNullable<FieldVariants['size']>;

/** The bare input inside a field box. */
export const FIELD_INPUT = [
  'h-full min-w-0 flex-1 bg-transparent text-inherit outline-none placeholder:text-ink-subtle',
  DISABLED,
].join(' ');
