// Looks every control shares, so a focus ring or a disabled state means one thing everywhere.

/**
 * The focus ring, for controls that draw their own: an outline, which forced colours keep. Never pair it with `outline-none`: in Tailwind 4 that sets `--tw-outline-style: none`, which the ring's `outline-style` reads, so the ring would never show (built-css.test.ts checks every recipe).
 */
export const FOCUS_RING =
  'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring';

/** A ring drawn inside the control's box, for items in a list or a group where an outside ring would overlap a neighbour. */
export const FOCUS_RING_INSET =
  'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring';

/** Every control dims the same when disabled, by either attribute (aria-disabled keeps it focusable so it can say why). */
export const DISABLED =
  'disabled:cursor-not-allowed disabled:opacity-disabled aria-disabled:cursor-not-allowed aria-disabled:opacity-disabled';

/** A control busy with its own action: dimmed less, still readable. */
export const BUSY = 'cursor-progress opacity-busy';

/** Chrome a person acts on but does not read as content: no text selection while clicking about. */
export const CHROME = 'select-none';

/** Tones a status, a badge or a callout can take. */
export const TONES = ['neutral', 'accent', 'success', 'warning', 'danger', 'info'] as const;
export type Tone = (typeof TONES)[number];

/** Text in a tone, on any surface. */
export const TONE_TEXT: Record<Tone, string> = {
  neutral: 'text-ink-muted',
  accent: 'text-accent',
  success: 'text-success',
  warning: 'text-warning',
  danger: 'text-danger',
  info: 'text-info',
};

/** A soft fill with text in the tone: badges, chips, icon tiles. */
export const TONE_SOFT: Record<Tone, string> = {
  neutral: 'bg-wash-strong text-ink-muted',
  accent: 'bg-accent-soft text-accent',
  success: 'bg-success/soft text-success',
  warning: 'bg-warning/soft text-warning',
  danger: 'bg-danger/soft text-danger',
  info: 'bg-info/soft text-info',
};

/** A solid fill with the text that reads on it. */
export const TONE_SOLID: Record<Tone, string> = {
  neutral: 'bg-ink-muted text-base',
  accent: 'bg-accent text-accent-fg',
  success: 'bg-success text-on-fill',
  warning: 'bg-warning text-on-fill',
  danger: 'bg-danger text-on-fill',
  info: 'bg-info text-on-fill',
};

/** A faint panel with a border in the tone: callouts and banners. */
export const TONE_TINT: Record<Tone, string> = {
  neutral: 'border-line bg-wash',
  accent: 'border-accent/rim bg-accent/tint',
  success: 'border-success/rim bg-success/tint',
  warning: 'border-warning/rim bg-warning/tint',
  danger: 'border-danger/rim bg-danger/tint',
  info: 'border-info/rim bg-info/tint',
};

/** A bare fill in the tone: dots, stripes, bar segments. */
export const TONE_FILL: Record<Tone, string> = {
  neutral: 'bg-ink-subtle',
  accent: 'bg-accent',
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
  info: 'bg-info',
};
