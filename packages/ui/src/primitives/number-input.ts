export interface NumberRange {
  min?: number | undefined;
  max?: number | undefined;
  step: number;
}

/** How many steps Page Up and Page Down take at once. */
export const PAGE_STEPS = 10;

export function clampToRange(value: number, { min, max }: NumberRange): number {
  let result = value;
  if (min !== undefined) result = Math.max(min, result);
  if (max !== undefined) result = Math.min(max, result);
  return result;
}

/** Rounds to the step's precision, so 0.1 + 0.2 shows as 0.3. */
function tidy(value: number, step: number): number {
  const decimals = (String(step).split('.')[1] ?? '').length;
  return Number(value.toFixed(decimals));
}

/** A typed number as the field keeps it: to the step's precision (whole for a whole step, so a count is never 1.2) and within range. */
export function committedValue(typed: number, range: NumberRange): number {
  return clampToRange(tidy(typed, range.step), range);
}

/** `value` moved by `steps` steps, kept in range; an empty field starts from the minimum, else 0. */
export function stepValue(value: number | null, steps: number, range: NumberRange): number {
  const start = value ?? range.min ?? 0;
  return clampToRange(tidy(start + steps * range.step, range.step), range);
}

/** A typed number, with either `.` or `,` as the decimal mark; null for an empty field, NaN for text that is not a number. */
export function parseNumber(text: string): number | null {
  const trimmed = text.trim().replace(',', '.');
  if (trimmed === '') return null;
  return /^-?\d*\.?\d+$|^-?\d+\.$/.test(trimmed) ? Number(trimmed) : Number.NaN;
}

/** The value a spin button key sets, or null for a key it does not take. */
export function keyValue(key: string, value: number | null, range: NumberRange): number | null {
  switch (key) {
    case 'ArrowUp':
      return stepValue(value, 1, range);
    case 'ArrowDown':
      return stepValue(value, -1, range);
    case 'PageUp':
      return stepValue(value, PAGE_STEPS, range);
    case 'PageDown':
      return stepValue(value, -PAGE_STEPS, range);
    case 'Home':
      return range.min ?? null;
    case 'End':
      return range.max ?? null;
    default:
      return null;
  }
}
