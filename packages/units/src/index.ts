// Byte units, and how sizes and speeds are shown and typed. A kilobyte is 1024 bytes, as file managers count it, everywhere Wirecatch shows a size.

/** Bytes in a kilobyte. */
export const KB = 1024;
/** Bytes in a megabyte. */
export const MB = KB * KB;
/** Bytes in a gigabyte. */
export const GB = MB * KB;

const SHOWN_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** From this many of a unit on, a size shows no decimals (`820 MB`). */
const WHOLE_FROM = 100;
/** From this many of a unit on, a size shows one decimal (`12.4 MB`); below it, two (`4.70 GB`). */
const ONE_DECIMAL_FROM = 10;
const TWO_DECIMALS = 2;

/** `bytes` in the largest unit that keeps it at 1 or more, with that unit's index in `SHOWN_UNITS`. */
function scaled(bytes: number): { value: number; unit: number } {
  let value = bytes;
  let unit = 0;
  while (value >= KB && unit < SHOWN_UNITS.length - 1) {
    value /= KB;
    unit += 1;
  }
  return { value, unit };
}

function decimals(value: number): number {
  if (value >= WHOLE_FROM) return 0;
  return value >= ONE_DECIMAL_FROM ? 1 : TWO_DECIMALS;
}

/** A size for people to read, such as `512 B`, `4.70 GB` or `820 MB`: three significant digits at most. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  const { value, unit } = scaled(bytes);
  if (unit === 0) return `${Math.round(value)} B`;
  return `${value.toFixed(decimals(value))} ${SHOWN_UNITS[unit]}`;
}

/** A speed, such as `12.4 MB/s`; no speed is an empty string. */
export function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond <= 0) return '';
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** A limit someone set, such as `4 MB/s`, `1.5 MB/s` or `512 KB/s`: one decimal at most and no trailing zeros. */
export function formatRate(bytesPerSecond: number): string {
  const { value, unit } = scaled(bytesPerSecond);
  return `${Math.round(value * ONE_DECIMAL_FROM) / ONE_DECIMAL_FROM} ${SHOWN_UNITS[unit]}/s`;
}

/** The units a person can type a size in. */
export const SIZE_UNITS = [
  { unit: 'KB', bytes: KB },
  { unit: 'MB', bytes: MB },
  { unit: 'GB', bytes: GB },
] as const;

export type SizeUnit = (typeof SIZE_UNITS)[number]['unit'];

/** Bytes in `unit`. */
export function unitBytes(unit: SizeUnit): number {
  return SIZE_UNITS.find((u) => u.unit === unit)?.bytes ?? MB;
}

/** Rounds to hundredths, for a size that is not a whole number of any unit. */
const HUNDREDTHS = 100;

/** `bytes` as a number and unit to show in a size field: the largest unit that makes it whole, else kilobytes to two decimals. Nothing shows as `0 MB`. */
export function splitSize(bytes: number): { value: number; unit: SizeUnit } {
  if (bytes <= 0) return { value: 0, unit: 'MB' };
  for (const u of [...SIZE_UNITS].reverse()) {
    if (bytes % u.bytes === 0) return { value: bytes / u.bytes, unit: u.unit };
  }
  return { value: Math.round((bytes / KB) * HUNDREDTHS) / HUNDREDTHS, unit: 'KB' };
}

/** What a person typed in a size field, in bytes; a comma is read as a decimal point, empty is 0, and anything that is not a number of zero or more is null. */
export function toBytes(value: string, unit: SizeUnit): number | null {
  const trimmed = value.trim().replace(',', '.');
  if (trimmed === '') return 0;
  const number = Number(trimmed);
  if (!Number.isFinite(number) || number < 0) return null;
  return Math.round(number * unitBytes(unit));
}
