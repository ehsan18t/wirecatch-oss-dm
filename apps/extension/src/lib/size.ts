/** Units offered for the minimum file size, in bytes (1 KB = 1024 bytes, as file managers show it). */
export const SIZE_UNITS = [
  { unit: 'KB', bytes: 1024 },
  { unit: 'MB', bytes: 1024 ** 2 },
  { unit: 'GB', bytes: 1024 ** 3 },
] as const;

export type SizeUnit = (typeof SIZE_UNITS)[number]['unit'];

export function unitBytes(unit: SizeUnit): number {
  return SIZE_UNITS.find((u) => u.unit === unit)?.bytes ?? 1024 ** 2;
}

/** Picks the largest unit that shows `bytes` as a whole number; 0 shows as "0 MB". */
export function splitSize(bytes: number): { value: number; unit: SizeUnit } {
  if (bytes <= 0) return { value: 0, unit: 'MB' };
  for (let i = SIZE_UNITS.length - 1; i >= 0; i -= 1) {
    const u = SIZE_UNITS[i];
    if (u !== undefined && bytes % u.bytes === 0) return { value: bytes / u.bytes, unit: u.unit };
  }
  return { value: Math.round((bytes / 1024) * 100) / 100, unit: 'KB' };
}

/** Converts what a person typed into bytes; null for anything that is not a number of zero or more. */
export function toBytes(value: string, unit: SizeUnit): number | null {
  const trimmed = value.trim().replace(',', '.');
  if (trimmed === '') return 0;
  const number = Number(trimmed);
  if (!Number.isFinite(number) || number < 0) return null;
  return Math.round(number * unitBytes(unit));
}

/** A size for people to read, such as "820 KB" or "1.4 GB" (1 KB = 1024 bytes, as elsewhere). */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const shown = value >= 100 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${shown} ${units[unit] ?? 'TB'}`;
}
