/** `value` as a 0 to 1 share of `max`, clamped; 0 for an empty range. */
export const fraction = (value: number, max: number): number =>
  max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;

/** A whole bar in percent, the scale progress bars report on. */
export const PERCENT = 100;

/** A fraction as a whole percentage, floored as the taskbar's is: a bar never reads 100% before the work is done. */
export const wholePercent = (share: number): number => Math.floor(share * PERCENT);
