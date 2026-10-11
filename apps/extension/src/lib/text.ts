/**
 * Browser APIs report a missing text value as `undefined` or as an empty string,
 * depending on the API. This turns both into the `null` the extension uses.
 */
export function nonEmpty(value: string | undefined): string | null {
  return value === undefined || value === '' ? null : value;
}
