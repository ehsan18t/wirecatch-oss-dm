// Byte units, and how sizes and speeds are shown and typed. A kilobyte is 1024 bytes, as file managers count it, everywhere Wirecatch shows a size.

/** Bytes in a kilobyte. */
export const KB = 1024;
/** Bytes in a megabyte. */
export const MB = KB * KB;
/** Bytes in a gigabyte. */
export const GB = MB * KB;

/** Bits in a byte. */
export const BITS_PER_BYTE = 8;

/** Bytes a stream at `kbps` takes for `durationMs`: kilobits per second times milliseconds is bits. */
export function bytesAtBitRate(kbps: number, durationMs: number): number {
  return Math.round((kbps * durationMs) / BITS_PER_BYTE);
}

const SHOWN_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** From this many of a unit on, a size shows no decimals (`820 MB`). */
const WHOLE_FROM = 100;
/** From this many of a unit on, a size shows one decimal (`12.4 MB`); below it, two (`4.70 GB`). */
const ONE_DECIMAL_FROM = 10;
const TWO_DECIMALS = 2;
/** Each decimal place is a factor of ten. */
const DECIMAL_BASE = 10;

/** `value` rounded to `places` decimals. */
function roundTo(value: number, places: number): number {
  const factor = DECIMAL_BASE ** places;
  return Math.round(value * factor) / factor;
}

/** `value` rounded to tenths: limits and size fields show tenths at most. */
function toTenths(value: number): number {
  return roundTo(value, 1);
}

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

/** Decimals for `value` in `unit`: bytes are whole, other units keep three significant digits. */
function placesFor(value: number, unit: number): number {
  return unit === 0 ? 0 : decimals(value);
}

/** A size for people to read, such as `512 B`, `4.70 GB` or `820 MB`: three significant digits at most. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  let { value, unit } = scaled(bytes);
  let places = placesFor(value, unit);
  let shown = roundTo(value, places);
  // Rounding can reach the next step (99.998 KB shows as 100), which needs fewer decimals.
  if (placesFor(shown, unit) < places) {
    places = placesFor(shown, unit);
    shown = roundTo(value, places);
  }
  // Or reach a whole next unit (1023.9 KB shows as 1024), which carries into it.
  if (shown >= KB && unit < SHOWN_UNITS.length - 1) {
    value /= KB;
    unit += 1;
    places = placesFor(value, unit);
    shown = roundTo(value, places);
  }
  return `${shown.toFixed(places)} ${SHOWN_UNITS[unit]}`;
}

/** A speed, such as `12.4 MB/s`; no speed is an empty string. */
export function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond <= 0) return '';
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** A limit someone set, such as `4 MB/s`, `1.5 MB/s` or `512 KB/s`: one decimal at most and no trailing zeros. */
export function formatRate(bytesPerSecond: number): string {
  const { value, unit } = scaled(bytesPerSecond);
  return `${toTenths(value)} ${SHOWN_UNITS[unit]}/s`;
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

/** `bytes` as a number and unit to show in a size field: the largest unit that makes it whole, else kilobytes to two decimals. Nothing shows as `0 MB`. */
export function splitSize(bytes: number): { value: number; unit: SizeUnit } {
  if (bytes <= 0) return { value: 0, unit: 'MB' };
  for (const u of [...SIZE_UNITS].reverse()) {
    if (bytes % u.bytes === 0) return { value: bytes / u.bytes, unit: u.unit };
  }
  return { value: roundTo(bytes / KB, TWO_DECIMALS), unit: 'KB' };
}

/** `bytes` in `unit` to one decimal, as a field in that unit shows it. */
export function inUnit(bytes: number, unit: SizeUnit): number {
  return toTenths(bytes / unitBytes(unit));
}

/** `bytes` for a field that offers kilobytes and megabytes: megabytes from 1 MB on, kilobytes below, to one decimal (`1.5` MB, `512` KB). */
export function splitKbOrMb(bytes: number): { value: number; unit: 'KB' | 'MB' } {
  const unit = bytes >= MB ? 'MB' : 'KB';
  return { value: inUnit(bytes, unit), unit };
}

/** What a person typed in a size field, in bytes; a comma is read as a decimal point, empty is 0, and anything that is not a number of zero or more is null. A number field's value can be passed as it is. */
export function toBytes(value: string | number, unit: SizeUnit): number | null {
  const trimmed = String(value).trim().replace(',', '.');
  if (trimmed === '') return 0;
  const number = Number(trimmed);
  if (!Number.isFinite(number) || number < 0) return null;
  return Math.round(number * unitBytes(unit));
}
